use std::collections::BTreeMap;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use filetime::FileTime;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::container::{
    FinishedYarc, PayloadKind, YarcHeader, YarcReader, YarcWriteSummary, YarcWriter,
};
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ReleaseManifest {
    pub version: u8,
    pub release_key: String,
    pub package_name: String,
    pub yarc_id: String,
    pub yarc_size: u64,
    pub plaintext_size: u64,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind")]
pub enum ManifestEntry {
    #[serde(rename = "dir")]
    Directory { path: String, mtime: u64 },
    #[serde(rename = "file")]
    File { path: String, size: u64, mtime: u64 },
}

impl ReleaseManifest {
    pub fn to_json_bytes(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|err| {
            io::Error::new(ErrorKind::InvalidData, format!("invalid manifest: {err}"))
        })
    }

    pub fn from_json_bytes(bytes: &[u8]) -> io::Result<Self> {
        serde_json::from_slice(bytes).map_err(|err| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("failed to decode manifest json: {err}"),
            )
        })
    }

    pub async fn to_yarc<W: AsyncWrite + Unpin>(
        &self,
        out: W,
        key_bytes: [u8; 32],
        chunk_log2: u8,
    ) -> io::Result<FinishedYarc<W>> {
        let bytes = self.to_json_bytes()?;
        let writer = YarcWriter::new(key_bytes, chunk_log2);
        writer
            .encrypt_bytes(PayloadKind::Manifest, &bytes, out)
            .await
    }

    pub async fn from_yarc<R: AsyncRead + Unpin>(
        input: R,
        key_bytes: [u8; 32],
    ) -> io::Result<(Self, YarcHeader)> {
        let reader = YarcReader::new(key_bytes);
        let (bytes, header) = reader
            .decrypt_to_bytes(input, PayloadKind::Manifest)
            .await?;
        let manifest = Self::from_json_bytes(&bytes)?;
        Ok((manifest, header))
    }

    pub async fn build(
        release_key: impl Into<String>,
        package_name: impl Into<String>,
        directory: impl AsRef<Path>,
        yarc_summary: &YarcWriteSummary,
    ) -> io::Result<Self> {
        require_directory_yarc(yarc_summary)?;
        let entries = collect_manifest_entries(directory.as_ref()).await?;

        Ok(Self {
            version: 1,
            release_key: release_key.into(),
            package_name: package_name.into(),
            yarc_id: hex::encode(yarc_summary.plaintext_hash),
            yarc_size: yarc_summary.container_len,
            plaintext_size: yarc_summary.plaintext_len,
            entries,
        })
    }

    pub async fn verify_directory(&self, directory: impl AsRef<Path>) -> io::Result<bool> {
        self.verify_directory_ignoring_paths(directory, &[]).await
    }

    pub async fn verify_directory_ignoring_paths(
        &self,
        directory: impl AsRef<Path>,
        ignored_paths: &[&str],
    ) -> io::Result<bool> {
        self.validate()?;
        let mut actual_entries = collect_manifest_entries(directory.as_ref()).await?;
        if !ignored_paths.is_empty() {
            actual_entries.retain(|entry| !ignored_paths.contains(&entry_path(entry)));
        }

        let expected = entries_by_path(&self.entries)?;
        let actual = entries_by_path(&actual_entries)?;

        Ok(actual == expected)
    }

    pub async fn apply_metadata_to_directory(&self, directory: impl AsRef<Path>) -> io::Result<()> {
        self.validate()?;
        let root = directory.as_ref().to_path_buf();

        let mut directories = Vec::new();
        for entry in &self.entries {
            match entry {
                ManifestEntry::File { path, mtime, .. } => {
                    apply_entry_mtime(root.join(path), *mtime).await?;
                }
                ManifestEntry::Directory { path, mtime } => {
                    directories.push((root.join(path), *mtime));
                }
            }
        }

        directories.sort_by(|left, right| {
            right
                .0
                .components()
                .count()
                .cmp(&left.0.components().count())
                .then_with(|| left.0.cmp(&right.0))
        });

        for (path, mtime) in directories {
            apply_entry_mtime(path, mtime).await?;
        }

        Ok(())
    }

    fn validate(&self) -> io::Result<()> {
        if self.version != 1 {
            return Err(invalid_data("unsupported manifest version"));
        }

        if self.release_key.is_empty() {
            return Err(invalid_data("manifest release_key must not be empty"));
        }

        if self.package_name.is_empty() {
            return Err(invalid_data("manifest package_name must not be empty"));
        }

        if self.yarc_id.len() != 64 || !self.yarc_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid_data(
                "manifest yarc_id must be a lowercase hex BLAKE3 digest",
            ));
        }

        let _ = entries_by_path(&self.entries)?;
        Ok(())
    }
}

async fn collect_manifest_entries(root: &Path) -> io::Result<Vec<ManifestEntry>> {
    let metadata = fs::metadata(root).await?;
    if !metadata.is_dir() {
        return Err(invalid_input("manifest root must be a directory"));
    }

    let mut entries = Vec::new();
    Box::pin(collect_manifest_entries_recursive(
        root,
        PathBuf::new(),
        &mut entries,
    ))
    .await?;
    Ok(entries)
}

async fn collect_manifest_entries_recursive(
    root: &Path,
    relative_dir: PathBuf,
    entries: &mut Vec<ManifestEntry>,
) -> io::Result<()> {
    let directory = root.join(&relative_dir);
    let mut read_dir = fs::read_dir(&directory).await?;
    let mut children = Vec::new();

    while let Some(entry) = read_dir.next_entry().await? {
        children.push(entry);
    }

    children.sort_by_key(|left| left.file_name());

    for child in children {
        let path = child.path();
        let metadata = child.metadata().await?;
        let relative_path = relative_dir.join(child.file_name());
        let manifest_path = normalize_relative_path(&relative_path)?;

        if metadata.is_dir() {
            entries.push(ManifestEntry::Directory {
                path: manifest_path,
                mtime: mtime_secs(&metadata)?,
            });
            Box::pin(collect_manifest_entries_recursive(
                root,
                relative_path,
                entries,
            ))
            .await?;
            continue;
        }

        if metadata.is_file() {
            entries.push(ManifestEntry::File {
                path: manifest_path,
                size: metadata.len(),
                mtime: mtime_secs(&metadata)?,
            });
            continue;
        }

        return Err(invalid_data(&format!(
            "unsupported manifest entry type: {}",
            path.display()
        )));
    }

    Ok(())
}

fn entries_by_path(entries: &[ManifestEntry]) -> io::Result<BTreeMap<String, ManifestEntry>> {
    let mut by_path = BTreeMap::new();

    for entry in entries {
        let path = entry_path(entry);
        validate_manifest_path(path)?;

        if by_path.insert(path.to_owned(), entry.clone()).is_some() {
            return Err(invalid_data(&format!(
                "duplicate manifest entry path: {path}"
            )));
        }
    }

    Ok(by_path)
}

fn entry_path(entry: &ManifestEntry) -> &str {
    match entry {
        ManifestEntry::Directory { path, .. } | ManifestEntry::File { path, .. } => path,
    }
}

fn validate_manifest_path(path: &str) -> io::Result<()> {
    if path.is_empty() {
        return Err(invalid_data("manifest entry path must not be empty"));
    }

    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(invalid_data(
                    "manifest entry path must be normalized and relative",
                ));
            }
        }
    }

    Ok(())
}

fn normalize_relative_path(path: &Path) -> io::Result<String> {
    let mut normalized = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| invalid_data("manifest entry path is not valid utf-8"))?;
                normalized.push(part);
            }
            _ => {
                return Err(invalid_data(
                    "manifest entry path must be normalized and relative",
                ));
            }
        }
    }

    if normalized.is_empty() {
        return Err(invalid_data("manifest entry path must not be empty"));
    }

    Ok(normalized.join("/"))
}

fn mtime_secs(metadata: &std::fs::Metadata) -> io::Result<u64> {
    metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| invalid_data("manifest mtime must be after the unix epoch"))
}

fn require_directory_yarc(yarc_summary: &YarcWriteSummary) -> io::Result<()> {
    if yarc_summary.header.kind() != PayloadKind::DirectoryTar {
        return Err(invalid_input(
            "manifest YARC summary must describe a directory tar YARC",
        ));
    }
    Ok(())
}

async fn apply_entry_mtime(path: PathBuf, mtime: u64) -> io::Result<()> {
    let file_time = FileTime::from_unix_time(
        i64::try_from(mtime).map_err(|_| invalid_data("manifest mtime does not fit in i64"))?,
        0,
    );

    tokio::task::spawn_blocking(move || filetime::set_file_mtime(&path, file_time))
        .await
        .map_err(|err| io::Error::other(format!("failed to join mtime task: {err}")))??;

    Ok(())
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use filetime::{FileTime, set_file_mtime};

    use super::*;

    #[tokio::test]
    async fn builds_and_verifies_manifest_for_directory_yarc() -> io::Result<()> {
        let root = unique_test_path("manifest-build-verify");
        fs::create_dir_all(root.join("subdir")).await?;
        fs::write(root.join("alpha.txt"), b"alpha").await?;
        fs::write(root.join("subdir").join("beta.txt"), b"beta").await?;
        normalize_test_tree_timestamps(&root)?;

        let writer = YarcWriter::new([3; 32], 12);
        let yarc = writer.archive_directory(&root, Vec::new()).await?;
        let manifest = ReleaseManifest::build("release-key", "pkg", &root, &yarc.summary).await?;

        assert!(manifest.verify_directory(&root).await?);

        assert_eq!(manifest.yarc_id, hex::encode(yarc.summary.plaintext_hash));
        assert_eq!(
            manifest.entries,
            vec![
                ManifestEntry::File {
                    path: "alpha.txt".to_string(),
                    size: 5,
                    mtime: 1_700_000_000,
                },
                ManifestEntry::Directory {
                    path: "subdir".to_string(),
                    mtime: 1_700_000_000,
                },
                ManifestEntry::File {
                    path: "subdir/beta.txt".to_string(),
                    size: 4,
                    mtime: 1_700_000_000,
                },
            ]
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn verification_rejects_directory_drift() -> io::Result<()> {
        let root = unique_test_path("manifest-directory-drift");
        fs::create_dir_all(&root).await?;
        fs::write(root.join("alpha.txt"), b"alpha").await?;
        normalize_test_tree_timestamps(&root)?;

        let writer = YarcWriter::new([4; 32], 12);
        let yarc = writer.archive_directory(&root, Vec::new()).await?;
        let manifest = ReleaseManifest::build("release-key", "pkg", &root, &yarc.summary).await?;

        fs::write(root.join("alpha.txt"), b"changed").await?;
        assert!(!manifest.verify_directory(&root).await?);

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn manifest_round_trips_through_yarc() -> io::Result<()> {
        let root = unique_test_path("manifest-yarc-roundtrip");
        fs::create_dir_all(&root).await?;
        fs::write(root.join("alpha.txt"), b"alpha").await?;
        normalize_test_tree_timestamps(&root)?;

        let writer = YarcWriter::new([5; 32], 12);
        let directory_yarc = writer.archive_directory(&root, Vec::new()).await?;
        let manifest =
            ReleaseManifest::build("release-key", "pkg", &root, &directory_yarc.summary).await?;

        let manifest_yarc = manifest.to_yarc(Vec::new(), [6; 32], 12).await?;
        let (decoded, header) =
            ReleaseManifest::from_yarc(manifest_yarc.out.as_slice(), [6; 32]).await?;

        assert_eq!(header.kind(), PayloadKind::Manifest);
        assert_eq!(decoded, manifest);

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn apply_metadata_restores_extracted_directory_timestamps() -> io::Result<()> {
        let source = unique_test_path("manifest-apply-metadata-source");
        let target = unique_test_path("manifest-apply-metadata-target");
        fs::create_dir_all(source.join("subdir")).await?;
        fs::write(source.join("alpha.txt"), b"alpha").await?;
        fs::write(source.join("subdir").join("beta.txt"), b"beta").await?;
        normalize_test_tree_timestamps(&source)?;

        let writer = YarcWriter::new([33; 32], 12);
        let directory_yarc = writer.archive_directory(&source, Vec::new()).await?;
        let manifest =
            ReleaseManifest::build("release-key", "pkg", &source, &directory_yarc.summary).await?;

        YarcReader::new([33; 32])
            .extract_to_directory(std::io::Cursor::new(directory_yarc.out), &target)
            .await?;

        assert!(!manifest.verify_directory(&target).await?);

        manifest.apply_metadata_to_directory(&target).await?;
        assert!(manifest.verify_directory(&target).await?);

        let _ = fs::remove_dir_all(&source).await;
        let _ = fs::remove_dir_all(&target).await;
        Ok(())
    }

    #[tokio::test]
    async fn manifest_hash_is_stable_across_build_times() -> io::Result<()> {
        let root = unique_test_path("manifest-stable-hash");
        fs::create_dir_all(&root).await?;
        fs::write(root.join("alpha.txt"), b"alpha").await?;
        normalize_test_tree_timestamps(&root)?;

        let writer = YarcWriter::new([7; 32], 12);
        let directory_yarc = writer.archive_directory(&root, Vec::new()).await?;
        let first =
            ReleaseManifest::build("release-key", "pkg", &root, &directory_yarc.summary).await?;
        let second =
            ReleaseManifest::build("release-key", "pkg", &root, &directory_yarc.summary).await?;

        assert_eq!(first, second);
        assert_eq!(first.to_json_bytes()?, second.to_json_bytes()?);

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn build_rejects_non_directory_yarc_summary() -> io::Result<()> {
        let root = unique_test_path("manifest-wrong-yarc-kind");
        fs::create_dir_all(&root).await?;
        normalize_test_tree_timestamps(&root)?;

        let manifest_yarc = ReleaseManifest {
            version: 1,
            release_key: "release-key".to_string(),
            package_name: "pkg".to_string(),
            yarc_id: "0".repeat(64),
            yarc_size: 0,
            plaintext_size: 0,
            entries: Vec::new(),
        }
        .to_yarc(Vec::new(), [8; 32], 12)
        .await?;

        let err = ReleaseManifest::build("release-key", "pkg", &root, &manifest_yarc.summary)
            .await
            .expect_err("non-directory YARC summary should be rejected");

        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            err.to_string(),
            "manifest YARC summary must describe a directory tar YARC"
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn verify_directory_rejects_invalid_manifest_paths() -> io::Result<()> {
        let root = unique_test_path("manifest-invalid-path");
        fs::create_dir_all(&root).await?;

        let manifest = ReleaseManifest {
            version: 1,
            release_key: "release-key".to_string(),
            package_name: "pkg".to_string(),
            yarc_id: "0".repeat(64),
            yarc_size: 0,
            plaintext_size: 0,
            entries: vec![ManifestEntry::File {
                path: "../alpha.txt".to_string(),
                size: 5,
                mtime: 1_700_000_000,
            }],
        };

        let err = manifest
            .verify_directory(&root)
            .await
            .expect_err("invalid manifest path should be rejected");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(
            err.to_string(),
            "manifest entry path must be normalized and relative"
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn verify_directory_rejects_duplicate_manifest_paths() -> io::Result<()> {
        let root = unique_test_path("manifest-duplicate-path");
        fs::create_dir_all(&root).await?;

        let manifest = ReleaseManifest {
            version: 1,
            release_key: "release-key".to_string(),
            package_name: "pkg".to_string(),
            yarc_id: "0".repeat(64),
            yarc_size: 0,
            plaintext_size: 0,
            entries: vec![
                ManifestEntry::File {
                    path: "alpha.txt".to_string(),
                    size: 5,
                    mtime: 1_700_000_000,
                },
                ManifestEntry::Directory {
                    path: "alpha.txt".to_string(),
                    mtime: 1_700_000_000,
                },
            ],
        };

        let err = manifest
            .verify_directory(&root)
            .await
            .expect_err("duplicate manifest paths should be rejected");

        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "duplicate manifest entry path: alpha.txt");

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    #[tokio::test]
    async fn verify_directory_can_ignore_local_metadata_files() -> io::Result<()> {
        let root = unique_test_path("manifest-ignore-local-metadata");
        fs::create_dir_all(root.join("subdir")).await?;
        fs::write(root.join("alpha.txt"), b"alpha").await?;
        fs::write(root.join("subdir").join("beta.txt"), b"beta").await?;
        normalize_test_tree_timestamps(&root)?;

        let writer = YarcWriter::new([9; 32], 12);
        let yarc = writer.archive_directory(&root, Vec::new()).await?;
        let manifest = ReleaseManifest::build("release-key", "pkg", &root, &yarc.summary).await?;

        fs::write(root.join("metadata.json"), b"{}").await?;
        fs::write(root.join("release.json"), b"{}").await?;

        assert!(!manifest.verify_directory(&root).await?);
        assert!(
            manifest
                .verify_directory_ignoring_paths(&root, &["metadata.json", "release.json"])
                .await?
        );

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }

    fn normalize_test_tree_timestamps(root: &Path) -> io::Result<()> {
        let timestamp = FileTime::from_unix_time(1_700_000_000, 0);
        set_file_mtime(root, timestamp)?;

        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory)? {
                let entry = entry?;
                let path = entry.path();
                set_file_mtime(&path, timestamp)?;
                if entry.file_type()?.is_dir() {
                    stack.push(path);
                }
            }
        }

        Ok(())
    }

    fn unique_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}
