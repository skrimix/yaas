//! Encrypted YARC format helpers.
//!
//! This module defines YAAS Archive Container (YARC), a small framed format
//! used to persist either raw bytes or a tar archive of a directory.

use std::fmt;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use async_compression::tokio::{bufread::ZstdDecoder, write::ZstdEncoder};
use async_tar::{Archive as TarArchive, Builder as TarBuilder, HeaderMode};
use blake3::Hasher;
use chacha20poly1305::{
    Key, XChaCha20Poly1305,
    aead::stream::{DecryptorBE32, EncryptorBE32, Nonce, StreamBE32},
};
use tokio::fs;
use tokio::io::{self as tokio_io, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_stream::StreamExt;
use zerocopy::{FromBytes, IntoBytes};
use zerocopy_derive::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

const YARC_MAGIC: [u8; 8] = *b"YAASYARC";
const FRAME_LEN: usize = 4;
const LAST_CHUNK_FLAG: u32 = 1 << 31;
/// Default chunk size exponent used for framing (`1 << 22`, or 4 MiB).
pub const DEFAULT_CHUNK_LOG2: u8 = 22;

type XChaChaStreamNonce = Nonce<XChaCha20Poly1305, StreamBE32<XChaCha20Poly1305>>;

#[repr(C)]
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, IntoBytes, FromBytes, KnownLayout, Immutable, Unaligned,
)]
struct YarcHeaderRaw {
    magic: [u8; 8],
    version: u8,
    encryption_scheme: u8,
    kind: u8,
    chunk_log2: u8,
    nonce_prefix: [u8; 19],
    compression_scheme: u8,
    reserved: [u8; 8],
}

const HEADER_LEN: usize = std::mem::size_of::<YarcHeaderRaw>();

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadKind {
    /// Directory tar archive.
    DirectoryTar = 1,
    /// Package manifest.
    Manifest = 2,
    /// App list.
    AppList = 3,
}

impl PayloadKind {
    /// Parses `PayloadKind` from the YARC header.
    pub fn from_byte(byte: u8) -> io::Result<Self> {
        match byte {
            1 => Ok(Self::DirectoryTar),
            2 => Ok(Self::Manifest),
            3 => Ok(Self::AppList),
            _ => Err(invalid_data("unsupported payload kind")),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptionScheme {
    /// XChaCha20-Poly1305-Chunked encryption scheme.
    XChaCha20Poly1305Chunked = 1,
}

impl EncryptionScheme {
    /// Parses a serialized encryption scheme from the header.
    pub fn from_byte(byte: u8) -> io::Result<Self> {
        match byte {
            1 => Ok(Self::XChaCha20Poly1305Chunked),
            _ => Err(invalid_data("unsupported encryption scheme")),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionScheme {
    /// No compression.
    None = 0,
    /// Zstandard compression.
    Zstd = 1,
}

impl CompressionScheme {
    /// Parses a serialized compression scheme from the header.
    pub fn from_byte(byte: u8) -> io::Result<Self> {
        match byte {
            0 => Ok(Self::None),
            1 => Ok(Self::Zstd),
            _ => Err(invalid_data("unsupported compression scheme")),
        }
    }

    /// Returns the stable lowercase name used in configuration and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd => "zstd",
        }
    }
}

impl fmt::Display for CompressionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarcHeader {
    /// Version of the container format.
    version: u8,
    /// Encryption scheme used for the container.
    encryption_scheme: EncryptionScheme,
    /// Kind of the container payload.
    kind: PayloadKind,
    /// Log base 2 of the chunk size.
    chunk_log2: u8,
    /// Nonce prefix used for encryption (chunk nonce = nonce_prefix || chunk_index).
    nonce_prefix: [u8; 19],
    /// Compression scheme applied before encryption.
    compression_scheme: CompressionScheme,
}

impl YarcHeader {
    /// Builds a header for the configured payload and compression scheme.
    pub fn new(
        kind: PayloadKind,
        encryption_scheme: EncryptionScheme,
        chunk_log2: u8,
        compression_scheme: CompressionScheme,
        nonce_prefix: [u8; 19],
    ) -> Self {
        Self {
            version: 2,
            encryption_scheme,
            kind,
            chunk_log2,
            nonce_prefix,
            compression_scheme,
        }
    }

    /// Returns the container format version stored in the header.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Returns the logical container payload kind.
    pub fn kind(&self) -> PayloadKind {
        self.kind
    }

    /// Returns the configured plaintext chunk size in bytes.
    pub fn chunk_size(&self) -> io::Result<usize> {
        if !(self.chunk_log2 >= 12 && self.chunk_log2 <= 30) {
            return Err(invalid_data("chunk_log2 must be between 12 and 30"));
        }
        Ok(1 << self.chunk_log2)
    }

    /// Returns the configured plaintext chunk size exponent.
    pub fn chunk_log2(&self) -> u8 {
        self.chunk_log2
    }

    /// Returns the 19-byte nonce prefix stored in the header.
    pub fn nonce_prefix(&self) -> [u8; 19] {
        self.nonce_prefix
    }

    /// Returns the compression scheme used for the container payload.
    pub fn compression_scheme(&self) -> CompressionScheme {
        self.compression_scheme
    }

    /// Returns the encryption scheme used for the container.
    pub fn encryption_scheme(&self) -> EncryptionScheme {
        self.encryption_scheme
    }

    /// Serializes the header exactly as written at the start of each container.
    fn encode(self) -> [u8; HEADER_LEN] {
        YarcHeaderRaw::from(self)
            .as_bytes()
            .try_into()
            .expect("container header encoding must match header length")
    }

    /// Parses and validates a serialized container header.
    fn decode(buf: &[u8; HEADER_LEN]) -> io::Result<Self> {
        let raw = YarcHeaderRaw::ref_from_bytes(buf)
            .map_err(|_| invalid_data("invalid container header layout"))?;

        if raw.magic != YARC_MAGIC {
            return Err(invalid_data("invalid container magic"));
        }

        let compression_scheme = match raw.version {
            1 => CompressionScheme::None,
            2 => CompressionScheme::from_byte(raw.compression_scheme)?,
            _ => return Err(invalid_data("unsupported container version")),
        };

        let header = Self {
            version: raw.version,
            encryption_scheme: EncryptionScheme::from_byte(raw.encryption_scheme)?,
            kind: PayloadKind::from_byte(raw.kind)?,
            chunk_log2: raw.chunk_log2,
            nonce_prefix: raw.nonce_prefix,
            compression_scheme,
        };

        header.chunk_size()?;
        Ok(header)
    }

    /// Reads and validates a header from the start of a reader.
    pub async fn from_reader<R: AsyncRead + Unpin>(mut input: R) -> io::Result<Self> {
        let mut header_bytes = [0_u8; HEADER_LEN];
        input.read_exact(&mut header_bytes).await?;
        Self::decode(&header_bytes)
    }
}

impl From<YarcHeader> for YarcHeaderRaw {
    fn from(header: YarcHeader) -> Self {
        Self {
            magic: YARC_MAGIC,
            version: header.version,
            encryption_scheme: header.encryption_scheme as u8,
            kind: header.kind as u8,
            chunk_log2: header.chunk_log2,
            nonce_prefix: header.nonce_prefix,
            compression_scheme: header.compression_scheme as u8,
            reserved: [0; 8],
        }
    }
}

#[derive(Clone, Copy)]
struct PayloadSummary {
    plaintext_len: u64,
    plaintext_hash: [u8; 32],
}

#[derive(Debug)]
pub struct YarcWriteSummary {
    /// Container header.
    pub header: YarcHeader,
    /// Length of the logical plaintext payload before optional compression.
    pub plaintext_len: u64,
    /// BLAKE3 hash of the logical plaintext payload before optional compression.
    pub plaintext_hash: [u8; 32],
    /// Length of the payload after optional compression and before encryption.
    pub compressed_len: u64,
    /// Length of the container.
    pub container_len: u64,
    /// BLAKE3 hash of the container.
    pub container_hash: [u8; 32],
}

pub struct FinishedYarc<W> {
    /// The output writer returned after the container has been fully written.
    pub out: W,
    /// Aggregate information about the produced YARC and its plaintext.
    pub summary: YarcWriteSummary,
}

/// Streaming YARC encoder.
///
/// A `YarcWriter` owns the symmetric key and chunk size used to frame and
/// encrypt outgoing YARCs.
pub struct YarcWriter {
    key: [u8; 32],
    chunk_log2: u8,
}

impl YarcWriter {
    /// Creates a writer for containers encrypted with `key`.
    pub fn new(key: [u8; 32], chunk_log2: u8) -> Self {
        Self { key, chunk_log2 }
    }

    /// Tars a directory and writes it as an encrypted `DirectoryTar` container.
    pub async fn archive_directory<W: AsyncWrite + Unpin + Send + 'static>(
        &self,
        directory: impl AsRef<Path>,
        out: W,
    ) -> io::Result<FinishedYarc<W>> {
        self.archive_directory_with_compression(directory, out, CompressionScheme::None)
            .await
    }

    /// Tars a directory and writes it as an encrypted `DirectoryTar` container,
    /// optionally compressing the tar stream before encryption.
    pub async fn archive_directory_with_compression<W: AsyncWrite + Unpin + Send + 'static>(
        &self,
        directory: impl AsRef<Path>,
        out: W,
        compression_scheme: CompressionScheme,
    ) -> io::Result<FinishedYarc<W>> {
        let directory = directory.as_ref().to_path_buf();
        let metadata = tokio::fs::metadata(&directory).await?;
        if !metadata.is_dir() {
            return Err(invalid_input("directory must be a directory"));
        }

        let nonce_prefix = Self::generate_nonce_prefix()?;

        let header = YarcHeader::new(
            PayloadKind::DirectoryTar,
            EncryptionScheme::XChaCha20Poly1305Chunked,
            self.chunk_log2,
            compression_scheme,
            nonce_prefix,
        );
        let pipe_capacity = header.chunk_size()?;
        let (tar_reader, tar_writer) = tokio::io::duplex(pipe_capacity);

        // Build the tar stream concurrently so compression/encryption can consume
        // it as the archive is produced.
        let tar_task = tokio::spawn(async move {
            let mut builder = TarBuilder::new(tar_writer);
            builder.mode(HeaderMode::Deterministic);
            builder.follow_symlinks(false);
            append_directory_tree_canonical(&mut builder, &directory).await?;
            let mut writer = builder.into_inner().await?;
            writer.shutdown().await
        });

        match compression_scheme {
            CompressionScheme::None => {
                let encrypt_result = self.encrypt_reader(tar_reader, out, header).await;
                let tar_result = join_io_task(tar_task).await;

                match (encrypt_result, tar_result) {
                    (Err(err), _) => Err(err),
                    (Ok(_), Err(err)) => Err(err),
                    (Ok(finished), Ok(())) => Ok(finished),
                }
            }
            CompressionScheme::Zstd => {
                let (compressed_reader, compressed_writer) = tokio::io::duplex(pipe_capacity);
                let compression_task = tokio::spawn(async move {
                    let mut tar_reader = tar_reader;
                    let mut encoder = ZstdEncoder::new(compressed_writer);
                    let mut plaintext_hasher = Hasher::new();
                    let mut plaintext_len = 0_u64;
                    let mut buf = vec![0; pipe_capacity];

                    loop {
                        let read = tar_reader.read(&mut buf).await?;
                        if read == 0 {
                            break;
                        }

                        plaintext_len += read as u64;
                        plaintext_hasher.update(&buf[..read]);
                        encoder.write_all(&buf[..read]).await?;
                    }

                    encoder.shutdown().await?;
                    Ok(PayloadSummary {
                        plaintext_len,
                        plaintext_hash: *plaintext_hasher.finalize().as_bytes(),
                    })
                });

                let encrypt_result = self.encrypt_reader(compressed_reader, out, header).await;
                let compression_result = join_io_task(compression_task).await;
                let tar_result = join_io_task(tar_task).await;

                match (encrypt_result, compression_result, tar_result) {
                    (Err(err), _, _) => Err(err),
                    (Ok(_), Err(err), _) => Err(err),
                    (Ok(_), Ok(_), Err(err)) => Err(err),
                    (Ok(finished), Ok(payload_summary), Ok(())) => {
                        Ok(rewrite_plaintext_summary(finished, payload_summary))
                    }
                }
            }
        }
    }

    /// Encrypts an in-memory byte slice as a container of `kind`.
    pub async fn encrypt_bytes<W: AsyncWrite + Unpin>(
        &self,
        kind: PayloadKind,
        bytes: &[u8],
        out: W,
    ) -> io::Result<FinishedYarc<W>> {
        self.encrypt_bytes_with_compression(kind, bytes, out, CompressionScheme::None)
            .await
    }

    /// Encrypts an in-memory byte slice as a container of `kind`, optionally
    /// compressing the logical payload first.
    pub async fn encrypt_bytes_with_compression<W: AsyncWrite + Unpin>(
        &self,
        kind: PayloadKind,
        bytes: &[u8],
        out: W,
        compression_scheme: CompressionScheme,
    ) -> io::Result<FinishedYarc<W>> {
        let nonce_prefix = Self::generate_nonce_prefix()?;
        let header = YarcHeader::new(
            kind,
            EncryptionScheme::XChaCha20Poly1305Chunked,
            self.chunk_log2,
            compression_scheme,
            nonce_prefix,
        );

        match compression_scheme {
            CompressionScheme::None => {
                let reader = SliceAsyncReader::new(bytes);
                self.encrypt_reader(reader, out, header).await
            }
            CompressionScheme::Zstd => {
                let payload_summary = payload_summary(bytes);
                let mut encoder = ZstdEncoder::new(VecAsyncWriter::with_capacity(bytes.len()));
                encoder.write_all(bytes).await?;
                encoder.shutdown().await?;

                let compressed = encoder.into_inner().inner;
                let reader = SliceAsyncReader::new(&compressed);
                let finished = self.encrypt_reader(reader, out, header).await?;
                Ok(rewrite_plaintext_summary(finished, payload_summary))
            }
        }
    }

    async fn encrypt_reader<R: AsyncRead + Unpin, O: AsyncWrite + Unpin>(
        &self,
        mut input: R,
        mut out: O,
        header: YarcHeader,
    ) -> io::Result<FinishedYarc<O>> {
        let chunk_size = header.chunk_size()?;
        let header_bytes = header.encode();
        let nonce_prefix = header.nonce_prefix();

        out.write_all(&header_bytes).await?;

        let mut encryptor: EncryptorBE32<XChaCha20Poly1305> = EncryptorBE32::new(
            Key::from_slice(&self.key),
            XChaChaStreamNonce::from_slice(&nonce_prefix),
        );

        let mut yarc_hasher = Hasher::new();
        yarc_hasher.update(&header_bytes);
        let mut plaintext_hasher = Hasher::new();

        let mut plaintext_len = 0_u64;
        let mut yarc_len = HEADER_LEN as u64;
        let mut chunk_buf = vec![0; chunk_size];

        loop {
            // Each frame carries one AEAD-encrypted plaintext chunk. The last
            // frame is marked in-band in the frame header.
            let is_last = self
                .read_plaintext_chunk(&mut input, &mut chunk_buf, chunk_size)
                .await?;
            plaintext_len += chunk_buf.len() as u64;
            plaintext_hasher.update(&chunk_buf);

            if is_last {
                encryptor
                    .encrypt_last_in_place(&header_bytes, &mut chunk_buf)
                    .map_err(aead_err)?;
                Self::write_frame(
                    &mut out,
                    &mut yarc_hasher,
                    &mut yarc_len,
                    is_last,
                    &chunk_buf,
                )
                .await?;
                break;
            }

            encryptor
                .encrypt_next_in_place(&header_bytes, &mut chunk_buf)
                .map_err(aead_err)?;
            Self::write_frame(
                &mut out,
                &mut yarc_hasher,
                &mut yarc_len,
                is_last,
                &chunk_buf,
            )
            .await?;
        }

        Ok(FinishedYarc {
            out,
            summary: YarcWriteSummary {
                header,
                plaintext_len,
                plaintext_hash: *plaintext_hasher.finalize().as_bytes(),
                compressed_len: plaintext_len,
                container_len: yarc_len,
                container_hash: *yarc_hasher.finalize().as_bytes(),
            },
        })
    }

    async fn read_plaintext_chunk<R: AsyncRead + Unpin>(
        &self,
        input: &mut R,
        chunk_buf: &mut Vec<u8>,
        chunk_size: usize,
    ) -> io::Result<bool> {
        chunk_buf.resize(chunk_size, 0);

        let mut filled = 0;
        while filled < chunk_size {
            let read = input.read(&mut chunk_buf[filled..chunk_size]).await?;
            if read == 0 {
                break;
            }
            filled += read;
        }

        chunk_buf.truncate(filled);
        Ok(filled < chunk_size)
    }

    /// Writes a frame header followed by the encrypted chunk bytes.
    async fn write_frame<W: AsyncWrite + Unpin>(
        out: &mut W,
        hasher: &mut Hasher,
        yarc_len: &mut u64,
        is_last: bool,
        ciphertext: &[u8],
    ) -> io::Result<()> {
        let len = u32::try_from(ciphertext.len())
            .map_err(|_| invalid_input("ciphertext chunk too large"))?;
        if len & LAST_CHUNK_FLAG != 0 {
            return Err(invalid_input("ciphertext chunk exceeds frame limit"));
        }

        let frame = if is_last { len | LAST_CHUNK_FLAG } else { len };
        let frame_bytes = frame.to_be_bytes();

        out.write_all(&frame_bytes).await?;
        out.write_all(ciphertext).await?;

        hasher.update(&frame_bytes);
        hasher.update(ciphertext);
        *yarc_len += FRAME_LEN as u64 + u64::from(len);
        Ok(())
    }

    fn generate_nonce_prefix() -> io::Result<[u8; 19]> {
        let mut nonce_prefix = [0_u8; 19];
        getrandom::fill(&mut nonce_prefix)
            .map_err(|err| io::Error::other(format!("failed to generate nonce prefix: {err}")))?;
        Ok(nonce_prefix)
    }
}

struct YarcFrameHeader {
    len: usize,
    is_last: bool,
}

#[derive(Clone)]
/// Streaming YARC decoder.
///
/// `YarcReader` validates YARC headers and incrementally decrypts chunked payloads
/// using the shared symmetric key.
pub struct YarcReader {
    key: [u8; 32],
}

impl YarcReader {
    /// Creates a reader for YARCs encrypted with `key`.
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    async fn read_header<R: AsyncRead + Unpin>(
        input: &mut R,
    ) -> io::Result<([u8; HEADER_LEN], YarcHeader)> {
        let mut header_bytes = [0_u8; HEADER_LEN];
        input.read_exact(&mut header_bytes).await?;
        let header = YarcHeader::decode(&header_bytes)?;
        Ok((header_bytes, header))
    }

    /// Decrypts a `DirectoryTar` container and unpacks it into `directory`.
    pub async fn extract_to_directory<R: AsyncRead + Unpin + Send + 'static>(
        &self,
        mut input: R,
        directory: impl AsRef<Path>,
    ) -> io::Result<YarcHeader> {
        let target_directory = directory.as_ref().to_path_buf();
        let (header_bytes, header) = Self::read_header(&mut input).await?;
        require_payload_kind(header, PayloadKind::DirectoryTar)?;
        let tar_pipe_capacity = header.chunk_size()?;
        match header.compression_scheme() {
            CompressionScheme::None => {
                let (tar_writer, tar_reader) = tokio_io::duplex(tar_pipe_capacity);
                // Decrypt into the pipe while the tar archive is unpacked on the other
                // end, keeping memory usage bounded by the chunk size.
                let decrypt_task = tokio::spawn({
                    let reader = self.clone();
                    async move {
                        reader
                            .decrypt_payload_to_writer(input, tar_writer, header_bytes, header)
                            .await
                    }
                });

                let unpack_result = TarArchive::new(tar_reader).unpack(&target_directory).await;
                let decrypt_result = join_io_task(decrypt_task).await;

                match (unpack_result, decrypt_result) {
                    (Err(err), _) => Err(err),
                    (Ok(()), Err(err)) => Err(err),
                    (Ok(()), Ok((_writer, header))) => Ok(header),
                }
            }
            CompressionScheme::Zstd => {
                let (compressed_writer, compressed_reader) = tokio_io::duplex(tar_pipe_capacity);
                let (tar_writer, tar_reader) = tokio_io::duplex(tar_pipe_capacity);
                let decrypt_task = tokio::spawn({
                    let reader = self.clone();
                    async move {
                        reader
                            .decrypt_payload_to_writer(
                                input,
                                compressed_writer,
                                header_bytes,
                                header,
                            )
                            .await
                    }
                });
                let decompress_task = tokio::spawn(async move {
                    let mut decoder = ZstdDecoder::new(tokio_io::BufReader::new(compressed_reader));
                    let mut tar_writer = tar_writer;
                    tokio_io::copy(&mut decoder, &mut tar_writer)
                        .await
                        .map_err(zstd_decode_err)?;
                    tar_writer.shutdown().await?;
                    Ok(tar_writer)
                });

                let unpack_result = TarArchive::new(tar_reader).unpack(&target_directory).await;
                let decrypt_result = join_io_task(decrypt_task).await;
                let decompress_result = join_io_task(decompress_task).await;

                match (unpack_result, decrypt_result, decompress_result) {
                    (Err(err), _, _) => Err(err),
                    (Ok(()), Err(err), _) => Err(err),
                    (Ok(()), Ok(_), Err(err)) => Err(err),
                    (Ok(()), Ok((_writer, header)), Ok(_)) => Ok(header),
                }
            }
        }
    }

    /// Decrypts a YARC into memory, returning the plaintext and parsed header.
    pub async fn decrypt_to_bytes<R: AsyncRead + Unpin>(
        &self,
        mut input: R,
        expected_kind: PayloadKind,
    ) -> io::Result<(Vec<u8>, YarcHeader)> {
        let (header_bytes, header) = Self::read_header(&mut input).await?;
        require_payload_kind(header, expected_kind)?;
        match header.compression_scheme() {
            CompressionScheme::None => {
                let (writer, header) = self
                    .decrypt_payload_to_writer(input, VecAsyncWriter::new(), header_bytes, header)
                    .await?;
                Ok((writer.inner, header))
            }
            CompressionScheme::Zstd => {
                let (compressed_writer, header) = self
                    .decrypt_payload_to_writer(input, VecAsyncWriter::new(), header_bytes, header)
                    .await?;
                let compressed = compressed_writer.inner;
                let mut decoder =
                    ZstdDecoder::new(tokio_io::BufReader::new(SliceAsyncReader::new(&compressed)));
                let mut decoded = Vec::new();
                decoder
                    .read_to_end(&mut decoded)
                    .await
                    .map_err(zstd_decode_err)?;
                Ok((decoded, header))
            }
        }
    }

    async fn decrypt_payload_to_writer<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self,
        mut input: R,
        mut out: W,
        header_bytes: [u8; HEADER_LEN],
        header: YarcHeader,
    ) -> io::Result<(W, YarcHeader)> {
        let chunk_size = header.chunk_size()?;
        let mut decryptor: DecryptorBE32<XChaCha20Poly1305> = DecryptorBE32::new(
            Key::from_slice(&self.key),
            XChaChaStreamNonce::from_slice(&header.nonce_prefix()),
        );
        // `+ 16` accounts for the AEAD authentication tag appended to each
        // encrypted chunk.
        let mut ciphertext_buf = Vec::with_capacity(chunk_size + 16);

        loop {
            let frame = Self::read_frame_header(&mut input, chunk_size).await?;
            Self::read_frame_ciphertext(&mut input, &mut ciphertext_buf, frame.len).await?;

            if frame.is_last {
                decryptor
                    .decrypt_last_in_place(&header_bytes, &mut ciphertext_buf)
                    .map_err(aead_err)?;
                out.write_all(&ciphertext_buf).await?;
                out.shutdown().await?;
                return Ok((out, header));
            }

            decryptor
                .decrypt_next_in_place(&header_bytes, &mut ciphertext_buf)
                .map_err(aead_err)?;
            out.write_all(&ciphertext_buf).await?;
        }
    }

    async fn read_frame_ciphertext<R: AsyncRead + Unpin>(
        input: &mut R,
        ciphertext_buf: &mut Vec<u8>,
        len: usize,
    ) -> io::Result<()> {
        ciphertext_buf.resize(len, 0);

        if let Err(err) = input.read_exact(ciphertext_buf.as_mut_slice()).await {
            ciphertext_buf.clear();
            return Err(err);
        }

        Ok(())
    }

    async fn read_frame_header<R: AsyncRead + Unpin>(
        input: &mut R,
        chunk_size: usize,
    ) -> io::Result<YarcFrameHeader> {
        let mut frame_buf = [0_u8; FRAME_LEN];
        input.read_exact(&mut frame_buf).await?;

        let frame = u32::from_be_bytes(frame_buf);
        let len = (frame & !LAST_CHUNK_FLAG) as usize;
        if len > chunk_size + 16 {
            return Err(invalid_data(
                "ciphertext chunk exceeds configured chunk size",
            ));
        }

        Ok(YarcFrameHeader {
            len,
            is_last: frame & LAST_CHUNK_FLAG != 0,
        })
    }
}

/// Minimal `AsyncRead` adapter over an immutable byte slice.
struct SliceAsyncReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SliceAsyncReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl AsyncRead for SliceAsyncReader<'_> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let remaining = &self.bytes[self.offset..];
        if remaining.is_empty() {
            return std::task::Poll::Ready(Ok(()));
        }

        let to_copy = remaining.len().min(buf.remaining());
        buf.put_slice(&remaining[..to_copy]);
        self.offset += to_copy;
        std::task::Poll::Ready(Ok(()))
    }
}

/// Minimal `AsyncWrite` adapter that accumulates bytes into a `Vec<u8>`.
struct VecAsyncWriter {
    inner: Vec<u8>,
}

impl VecAsyncWriter {
    fn new() -> Self {
        Self { inner: Vec::new() }
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }
}

impl AsyncWrite for VecAsyncWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        self.inner.extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

struct CanonicalDirEntry {
    src_path: PathBuf,
    archive_path: PathBuf,
    is_dir: bool,
}

async fn append_directory_tree_canonical<W: AsyncWrite + Unpin + Send + Sync>(
    builder: &mut TarBuilder<W>,
    root: &Path,
) -> io::Result<()> {
    builder.append_dir(".", root).await?;
    append_directory_children_canonical(builder, root, Path::new(".")).await
}

async fn append_directory_children_canonical<W: AsyncWrite + Unpin + Send + Sync>(
    builder: &mut TarBuilder<W>,
    src_dir: &Path,
    archive_dir: &Path,
) -> io::Result<()> {
    let mut entries = read_directory_entries_canonical(src_dir, archive_dir).await?;
    entries.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));

    for entry in entries {
        builder
            .append_path_with_name(&entry.src_path, &entry.archive_path)
            .await?;
        if entry.is_dir {
            // TODO: Replace this boxed async recursion with an explicit stack to
            // avoid per-directory future allocations on large directory trees.
            Box::pin(append_directory_children_canonical(
                builder,
                &entry.src_path,
                &entry.archive_path,
            ))
            .await?;
        }
    }

    Ok(())
}

async fn read_directory_entries_canonical(
    src_dir: &Path,
    archive_dir: &Path,
) -> io::Result<Vec<CanonicalDirEntry>> {
    let mut entries = tokio_stream::wrappers::ReadDirStream::new(fs::read_dir(src_dir).await?);
    let mut canonical_entries = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let file_type = entry.file_type().await?;
        canonical_entries.push(CanonicalDirEntry {
            archive_path: archive_dir.join(entry.file_name()),
            src_path: entry.path(),
            is_dir: file_type.is_dir(),
        });
    }

    Ok(canonical_entries)
}

fn payload_summary(bytes: &[u8]) -> PayloadSummary {
    PayloadSummary {
        plaintext_len: bytes.len() as u64,
        plaintext_hash: *blake3::hash(bytes).as_bytes(),
    }
}

fn rewrite_plaintext_summary<W>(
    mut finished: FinishedYarc<W>,
    payload_summary: PayloadSummary,
) -> FinishedYarc<W> {
    finished.summary.plaintext_len = payload_summary.plaintext_len;
    finished.summary.plaintext_hash = payload_summary.plaintext_hash;
    finished
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

fn aead_err(_: chacha20poly1305::Error) -> io::Error {
    // The error type is opaque, so we can't provide any more information.
    invalid_data("AEAD error")
}

fn zstd_decode_err(err: io::Error) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        format!("failed to decode zstd payload: {err}"),
    )
}

/// Awaits a spawned I/O task and normalizes join failures into `io::Error`.
async fn join_io_task<T>(handle: tokio::task::JoinHandle<io::Result<T>>) -> io::Result<T> {
    handle
        .await
        .map_err(|err| io::Error::other(format!("background task failed: {err}")))?
}

/// Ensures the decoded header matches the caller's expected payload kind.
fn require_payload_kind(header: YarcHeader, expected_kind: PayloadKind) -> io::Result<()> {
    if header.kind() != expected_kind {
        return Err(invalid_data("unexpected payload kind"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use filetime::{FileTime, set_file_mtime};

    use super::*;

    #[tokio::test]
    async fn directory_plaintext_hash_is_stable_across_creation_order() -> io::Result<()> {
        let root = unique_test_path("yarc-canonical-order");
        let first = root.join("first");
        let second = root.join("second");

        fs::create_dir_all(&first).await?;
        fs::create_dir_all(&second).await?;

        fs::write(first.join("b.txt"), b"second").await?;
        fs::write(first.join("a.txt"), b"first").await?;

        fs::write(second.join("a.txt"), b"first").await?;
        fs::write(second.join("b.txt"), b"second").await?;

        normalize_test_tree_timestamps(&first)?;
        normalize_test_tree_timestamps(&second)?;

        let writer = YarcWriter::new([7; 32], 12);
        let first_yarc = writer.archive_directory(&first, Vec::new()).await?;
        let second_yarc = writer.archive_directory(&second, Vec::new()).await?;

        assert_eq!(
            first_yarc.summary.plaintext_hash,
            second_yarc.summary.plaintext_hash
        );

        let reader = YarcReader::new([7; 32]);
        let (tar_bytes, _) = reader
            .decrypt_to_bytes(
                std::io::Cursor::new(first_yarc.out),
                PayloadKind::DirectoryTar,
            )
            .await?;
        assert_eq!(
            parse_tar_entry_paths(&tar_bytes)?,
            vec![".", "a.txt", "b.txt"]
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn archives_nested_directories_in_canonical_depth_first_order() -> io::Result<()> {
        let root = unique_test_path("yarc-canonical-nested-order");
        let source = root.join("source");

        fs::create_dir_all(source.join("nested")).await?;
        fs::write(source.join("z.txt"), b"root-last").await?;
        fs::write(source.join("a.txt"), b"root-first").await?;
        fs::write(source.join("nested").join("b.txt"), b"nested-last").await?;
        fs::write(source.join("nested").join("a.txt"), b"nested-first").await?;

        let writer = YarcWriter::new([8; 32], 12);
        let yarc = writer.archive_directory(&source, Vec::new()).await?;

        let reader = YarcReader::new([8; 32]);
        let (tar_bytes, _) = reader
            .decrypt_to_bytes(std::io::Cursor::new(yarc.out), PayloadKind::DirectoryTar)
            .await?;
        assert_eq!(
            parse_tar_entry_paths(&tar_bytes)?,
            vec![
                ".",
                "a.txt",
                "nested",
                "nested/a.txt",
                "nested/b.txt",
                "z.txt"
            ]
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn compressed_directory_archive_preserves_plaintext_summary() -> io::Result<()> {
        let root = unique_test_path("yarc-compressed-directory-summary");
        let source = root.join("source");

        fs::create_dir_all(source.join("nested")).await?;
        fs::write(source.join("root.txt"), b"root").await?;
        fs::write(source.join("nested").join("child.txt"), b"child").await?;

        let writer = YarcWriter::new([18; 32], 12);
        let uncompressed = writer.archive_directory(&source, Vec::new()).await?;
        let compressed = writer
            .archive_directory_with_compression(&source, Vec::new(), CompressionScheme::Zstd)
            .await?;

        assert_eq!(compressed.summary.header.kind(), PayloadKind::DirectoryTar);
        assert_eq!(
            compressed.summary.header.compression_scheme(),
            CompressionScheme::Zstd
        );
        assert_eq!(
            compressed.summary.plaintext_len,
            uncompressed.summary.plaintext_len
        );
        assert_eq!(
            compressed.summary.plaintext_hash,
            uncompressed.summary.plaintext_hash
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn archive_directory_with_zstd_compression_extracts_to_directory() -> io::Result<()> {
        let source = unique_test_path("yarc-compressed-dir-source");
        let target = unique_test_path("yarc-compressed-dir-target");

        fs::create_dir_all(source.join("nested")).await?;
        fs::create_dir_all(&target).await?;
        fs::write(source.join("root.txt"), b"root").await?;
        fs::write(source.join("nested").join("child.txt"), b"child").await?;

        let writer = YarcWriter::new([25; 32], 12);
        let yarc = writer
            .archive_directory_with_compression(&source, Vec::new(), CompressionScheme::Zstd)
            .await?;

        let header = YarcReader::new([25; 32])
            .extract_to_directory(std::io::Cursor::new(yarc.out), &target)
            .await?;

        assert_eq!(header.kind(), PayloadKind::DirectoryTar);
        assert_eq!(header.version(), 2);
        assert_eq!(header.compression_scheme(), CompressionScheme::Zstd);
        assert_eq!(fs::read(target.join("root.txt")).await?, b"root");
        assert_eq!(
            fs::read(target.join("nested").join("child.txt")).await?,
            b"child"
        );

        let _ = fs::remove_dir_all(&source).await;
        let _ = fs::remove_dir_all(&target).await;
        Ok(())
    }

    #[tokio::test]
    async fn encrypt_and_decrypt_empty_manifest_yarc() -> io::Result<()> {
        let writer = YarcWriter::new([9; 32], 12);
        let yarc = writer
            .encrypt_bytes(PayloadKind::Manifest, &[], Vec::new())
            .await?;

        assert_eq!(yarc.summary.plaintext_len, 0);

        let reader = YarcReader::new([9; 32]);
        let (bytes, header) = reader
            .decrypt_to_bytes(std::io::Cursor::new(yarc.out), PayloadKind::Manifest)
            .await?;

        assert!(bytes.is_empty());
        assert_eq!(header.kind(), PayloadKind::Manifest);
        assert_eq!(header.version(), 2);
        assert_eq!(header.compression_scheme(), CompressionScheme::None);
        Ok(())
    }

    #[tokio::test]
    async fn encrypt_and_decrypt_zstd_compressed_manifest_yarc() -> io::Result<()> {
        let writer = YarcWriter::new([19; 32], 12);
        let yarc = writer
            .encrypt_bytes_with_compression(
                PayloadKind::Manifest,
                b"payload",
                Vec::new(),
                CompressionScheme::Zstd,
            )
            .await?;

        assert_eq!(yarc.summary.plaintext_len, 7);
        assert_eq!(
            yarc.summary.plaintext_hash,
            *blake3::hash(b"payload").as_bytes()
        );

        let reader = YarcReader::new([19; 32]);
        let (bytes, header) = reader
            .decrypt_to_bytes(std::io::Cursor::new(yarc.out), PayloadKind::Manifest)
            .await?;

        assert_eq!(bytes, b"payload");
        assert_eq!(header.kind(), PayloadKind::Manifest);
        assert_eq!(header.version(), 2);
        assert_eq!(header.compression_scheme(), CompressionScheme::Zstd);
        Ok(())
    }

    #[tokio::test]
    async fn encrypt_and_decrypt_multiframe_zstd_compressed_manifest_yarc() -> io::Result<()> {
        let mut payload = Vec::new();
        for index in 0_u32..512 {
            payload.extend_from_slice(blake3::hash(&index.to_le_bytes()).as_bytes());
        }

        let writer = YarcWriter::new([24; 32], 12);
        let yarc = writer
            .encrypt_bytes_with_compression(
                PayloadKind::Manifest,
                &payload,
                Vec::new(),
                CompressionScheme::Zstd,
            )
            .await?;

        assert!(count_yarc_frames(&yarc.out)? > 1);
        assert_eq!(yarc.summary.plaintext_len, payload.len() as u64);
        assert_eq!(
            yarc.summary.plaintext_hash,
            *blake3::hash(&payload).as_bytes()
        );

        let reader = YarcReader::new([24; 32]);
        let (bytes, header) = reader
            .decrypt_to_bytes(std::io::Cursor::new(yarc.out), PayloadKind::Manifest)
            .await?;

        assert_eq!(bytes, payload);
        assert_eq!(header.kind(), PayloadKind::Manifest);
        assert_eq!(header.compression_scheme(), CompressionScheme::Zstd);
        Ok(())
    }

    #[tokio::test]
    async fn extracts_zstd_compressed_directory_tar() -> io::Result<()> {
        let source = unique_test_path("yarc-zstd-dir-source");
        let target = unique_test_path("yarc-zstd-dir-target");

        fs::create_dir_all(source.join("nested")).await?;
        fs::create_dir_all(&target).await?;
        fs::write(source.join("root.txt"), b"root").await?;
        fs::write(source.join("nested").join("child.txt"), b"child").await?;

        let tar_bytes = tar_directory_bytes(&source).await?;
        let writer = YarcWriter::new([20; 32], 12);
        let yarc = writer
            .encrypt_bytes_with_compression(
                PayloadKind::DirectoryTar,
                &tar_bytes,
                Vec::new(),
                CompressionScheme::Zstd,
            )
            .await?;

        let header = YarcReader::new([20; 32])
            .extract_to_directory(std::io::Cursor::new(yarc.out), &target)
            .await?;

        assert_eq!(header.kind(), PayloadKind::DirectoryTar);
        assert_eq!(header.version(), 2);
        assert_eq!(header.compression_scheme(), CompressionScheme::Zstd);
        assert_eq!(fs::read(target.join("root.txt")).await?, b"root");
        assert_eq!(
            fs::read(target.join("nested").join("child.txt")).await?,
            b"child"
        );

        let _ = fs::remove_dir_all(&source).await;
        let _ = fs::remove_dir_all(&target).await;
        Ok(())
    }

    #[tokio::test]
    async fn decrypt_rejects_invalid_zstd_payload() -> io::Result<()> {
        let writer = YarcWriter::new([21; 32], 12);
        let header = YarcHeader::new(
            PayloadKind::Manifest,
            EncryptionScheme::XChaCha20Poly1305Chunked,
            12,
            CompressionScheme::Zstd,
            YarcWriter::generate_nonce_prefix()?,
        );
        let yarc = writer
            .encrypt_reader(
                SliceAsyncReader::new(b"not a zstd stream"),
                VecAsyncWriter::new(),
                header,
            )
            .await?
            .out
            .inner;

        let err = YarcReader::new([21; 32])
            .decrypt_to_bytes(std::io::Cursor::new(yarc), PayloadKind::Manifest)
            .await
            .expect_err("invalid zstd payload should fail decoding");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert!(
            err.to_string()
                .starts_with("failed to decode zstd payload:"),
            "unexpected zstd decode error: {err}"
        );
        Ok(())
    }

    #[test]
    fn version_1_header_decodes_as_uncompressed() -> io::Result<()> {
        let raw = YarcHeaderRaw {
            magic: YARC_MAGIC,
            version: 1,
            encryption_scheme: EncryptionScheme::XChaCha20Poly1305Chunked as u8,
            kind: PayloadKind::Manifest as u8,
            chunk_log2: 12,
            nonce_prefix: [22; 19],
            compression_scheme: CompressionScheme::Zstd as u8,
            reserved: [0; 8],
        };
        let header_bytes = raw
            .as_bytes()
            .try_into()
            .expect("header bytes should match encoded header length");

        let header = YarcHeader::decode(&header_bytes)?;

        assert_eq!(header.version(), 1);
        assert_eq!(header.kind(), PayloadKind::Manifest);
        assert_eq!(header.compression_scheme(), CompressionScheme::None);
        Ok(())
    }

    #[test]
    fn version_2_header_rejects_unknown_compression_scheme() {
        let raw = YarcHeaderRaw {
            magic: YARC_MAGIC,
            version: 2,
            encryption_scheme: EncryptionScheme::XChaCha20Poly1305Chunked as u8,
            kind: PayloadKind::Manifest as u8,
            chunk_log2: 12,
            nonce_prefix: [23; 19],
            compression_scheme: 99,
            reserved: [0; 8],
        };
        let header_bytes = raw
            .as_bytes()
            .try_into()
            .expect("header bytes should match encoded header length");

        let err = YarcHeader::decode(&header_bytes)
            .expect_err("unknown compression scheme should fail to decode");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "unsupported compression scheme");
    }

    #[tokio::test]
    async fn decrypt_rejects_wrong_expected_kind() -> io::Result<()> {
        let writer = YarcWriter::new([10; 32], 12);
        let yarc = writer
            .encrypt_bytes(PayloadKind::AppList, b"payload", Vec::new())
            .await?;

        let err = YarcReader::new([10; 32])
            .decrypt_to_bytes(std::io::Cursor::new(yarc.out), PayloadKind::Manifest)
            .await
            .expect_err("payload kind mismatch should fail");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "unexpected payload kind");
        Ok(())
    }

    #[tokio::test]
    async fn decrypt_rejects_tampered_ciphertext() -> io::Result<()> {
        let writer = YarcWriter::new([11; 32], 12);
        let mut yarc = writer
            .encrypt_bytes(PayloadKind::Manifest, b"payload", Vec::new())
            .await?
            .out;

        let frame_offset = HEADER_LEN + FRAME_LEN;
        yarc[frame_offset] ^= 1;

        let err = YarcReader::new([11; 32])
            .decrypt_to_bytes(std::io::Cursor::new(yarc), PayloadKind::Manifest)
            .await
            .expect_err("tampered ciphertext should fail authentication");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "AEAD error");
        Ok(())
    }

    fn normalize_test_tree_timestamps(root: &Path) -> io::Result<()> {
        let timestamp = FileTime::from_unix_time(1_700_000_000, 0);
        for path in [root.join("a.txt"), root.join("b.txt"), root.to_path_buf()] {
            set_file_mtime(&path, timestamp)?;
        }
        Ok(())
    }

    async fn tar_directory_bytes(root: &Path) -> io::Result<Vec<u8>> {
        let mut builder = TarBuilder::new(VecAsyncWriter::new());
        builder.mode(HeaderMode::Deterministic);
        builder.follow_symlinks(false);
        append_directory_tree_canonical(&mut builder, root).await?;
        Ok(builder.into_inner().await?.inner)
    }

    fn parse_tar_entry_paths(bytes: &[u8]) -> io::Result<Vec<String>> {
        let mut offset = 0;
        let mut paths = Vec::new();

        while offset + 512 <= bytes.len() {
            let header = &bytes[offset..offset + 512];
            if header.iter().all(|byte| *byte == 0) {
                break;
            }

            let path_end = header[..100]
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(100);
            let path = std::str::from_utf8(&header[..path_end])
                .map_err(|_| invalid_data("tar path is not valid utf-8"))?;
            paths.push(path.to_string());

            let size_end = header[124..136]
                .iter()
                .position(|byte| *byte == 0 || *byte == b' ')
                .unwrap_or(12);
            let size_text = std::str::from_utf8(&header[124..124 + size_end])
                .map_err(|_| invalid_data("tar size is not valid utf-8"))?
                .trim();
            let size = if size_text.is_empty() {
                0
            } else {
                u64::from_str_radix(size_text, 8)
                    .map_err(|_| invalid_data("tar size is not valid octal"))?
            };

            let data_len = size.div_ceil(512) * 512;
            offset +=
                512 + usize::try_from(data_len).map_err(|_| invalid_data("tar entry too large"))?;
        }

        Ok(paths)
    }

    fn count_yarc_frames(bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() < HEADER_LEN {
            return Err(invalid_data("container shorter than header"));
        }

        let mut offset = HEADER_LEN;
        let mut frames = 0;

        while offset + FRAME_LEN <= bytes.len() {
            let frame = u32::from_be_bytes(
                bytes[offset..offset + FRAME_LEN]
                    .try_into()
                    .expect("frame header slice should have fixed length"),
            );
            let len = (frame & !LAST_CHUNK_FLAG) as usize;
            offset += FRAME_LEN;

            if offset + len > bytes.len() {
                return Err(invalid_data("truncated frame ciphertext"));
            }

            frames += 1;
            offset += len;

            if frame & LAST_CHUNK_FLAG != 0 {
                break;
            }
        }

        if frames == 0 || offset != bytes.len() {
            return Err(invalid_data("invalid frame layout"));
        }

        Ok(frames)
    }

    fn unique_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}
