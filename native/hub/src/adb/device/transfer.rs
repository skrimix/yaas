use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use forensic_adb::{DeviceError, DirectoryTransferProgress, UnixFileStatus, UnixPath, UnixPathBuf};
use tokio::{
    fs::{self, File},
    io::BufReader,
    sync::mpsc::UnboundedSender,
};
use tracing::{debug, instrument, trace};

use super::AdbDevice;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferKind {
    File,
    Directory,
}

impl TransferKind {
    fn from_remote_status(status: UnixFileStatus) -> Option<Self> {
        match status {
            UnixFileStatus::RegularFile => Some(Self::File),
            UnixFileStatus::Directory => Some(Self::Directory),
            _ => None,
        }
    }

    fn source_name(self, path: &Path) -> Result<&str> {
        path.file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")
    }

    fn remote_source_name(self, path: &UnixPath) -> Result<&str> {
        path.file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectoryPushDestination {
    AdbCompatible,
    ExactPath,
}

impl AdbDevice {
    #[instrument(level = "debug", ret, err)]
    async fn remote_entry_kind(&self, path: &UnixPath) -> Result<Option<TransferKind>> {
        match self.inner.stat(path).await {
            Ok(stat) => Ok(TransferKind::from_remote_status(stat.file_mode)),
            Err(DeviceError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Resolves the effective remote destination path for a push operation.
    ///
    /// Behavior:
    /// - If `dest` exists on the device and is a directory, the source file or directory name
    ///   is appended unless `dir_dest` requests an exact directory path.
    /// - If `dest` exists and is a regular file, then:
    ///   - pushing a local file overwrites that remote file path;
    ///   - pushing a local directory is rejected (cannot push a dir to an existing file).
    /// - If `dest` does not exist, use `dest` as-is. The device-side sync service will create
    ///   missing parent directories for file and directory pushes, matching `adb push` behavior.
    ///
    /// This mirrors common `adb push` conventions while allowing callers to opt into exact-path
    /// directory restores.
    async fn resolve_push_dest_path(
        &self,
        source: &Path,
        source_kind: TransferKind,
        dest: &UnixPath,
        dir_dest: DirectoryPushDestination,
    ) -> Result<UnixPathBuf> {
        let source_name = source_kind.source_name(source)?;

        match self.remote_entry_kind(dest).await? {
            Some(TransferKind::Directory) => match (source_kind, dir_dest) {
                (TransferKind::Directory, DirectoryPushDestination::ExactPath) => {
                    Ok(UnixPathBuf::from(dest))
                }
                _ => Ok(UnixPathBuf::from(dest).join(source_name)),
            },
            Some(TransferKind::File) => {
                if source_kind == TransferKind::Directory {
                    bail!(
                        "Cannot push directory '{}' to existing file '{}'",
                        source.display(),
                        dest.display()
                    )
                } else {
                    Ok(UnixPathBuf::from(dest))
                }
            }
            None => Ok(UnixPathBuf::from(dest)),
        }
    }

    /// Resolves the effective local destination path for a pull operation.
    ///
    /// Behavior:
    /// - If `dest` exists locally and is a directory, append the source file/dir name and pull
    ///   into that directory.
    /// - If `dest` exists and is a regular file, then pulling a remote directory is rejected,
    ///   otherwise the remote file is saved to that file path.
    /// - If `dest` does not exist but its parent exists locally, use `dest` as-is (callers may
    ///   create intermediate directories as needed).
    /// - If `dest` has no existing parent directory, returns an error to avoid surprising
    ///   filesystem writes.
    ///
    /// This keeps pull semantics predictable and prevents accidental directory creation outside
    /// intended locations.
    #[instrument(level = "debug", ret, err)]
    fn resolve_pull_dest_path(
        source: &UnixPath,
        source_kind: TransferKind,
        dest: &Path,
    ) -> Result<PathBuf> {
        let source_name = source_kind.remote_source_name(source)?;

        if dest.exists() {
            if dest.is_dir() {
                // If destination is a directory, append source file name
                Ok(dest.join(source_name))
            } else {
                // Can't pull to existing file if source is directory
                if source_kind == TransferKind::Directory {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        source.display(),
                        dest.display()
                    )
                } else {
                    Ok(dest.to_path_buf())
                }
            }
        } else if let Some(parent) = dest.parent() {
            if parent.exists() {
                // Parent exists, use destination path as is
                Ok(dest.to_path_buf())
            } else {
                bail!("Parent directory '{}' does not exist", parent.display())
            }
        } else {
            bail!("Invalid destination path: no parent directory")
        }
    }

    /// Pushes a file to the device
    ///
    /// # Arguments
    /// * `source_file` - Local path of the file to push
    /// * `dest_file` - Destination path on the device
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn push(&self, source_file: &Path, dest_file: &UnixPath) -> Result<()> {
        ensure!(
            source_file.is_file(),
            "Path does not exist or is not a file: {}",
            source_file.display()
        );

        let dest_path = self
            .resolve_push_dest_path(
                source_file,
                TransferKind::File,
                dest_file,
                DirectoryPushDestination::AdbCompatible,
            )
            .await?;
        debug!(source = %source_file.display(), dest = %dest_path.display(), "Pushing file");
        let mut file = BufReader::new(File::open(source_file).await?);
        self.inner.push(&mut file, &dest_path, 0o777).await.context("Failed to push file")
    }

    /// Pushes a directory to the device
    ///
    /// # Arguments
    /// * `source` - Local path of the directory to push
    /// * `dest` - Destination path on the device
    /// * `overwrite` - Whether to remove existing destination before pushing
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn push_dir(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
    ) -> Result<()> {
        self.push_dir_with_mode(source, dest, overwrite, DirectoryPushDestination::AdbCompatible)
            .await
    }

    /// Pushes a directory to an exact path on the device.
    ///
    /// If `dest` already exists as a directory, the directory contents are pushed into that
    /// directory instead of appending the local source directory name again.
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn push_dir_to_path(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
    ) -> Result<()> {
        self.push_dir_with_mode(source, dest, overwrite, DirectoryPushDestination::ExactPath).await
    }

    async fn push_dir_with_mode(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
        dir_dest: DirectoryPushDestination,
    ) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path =
            self.resolve_push_dest_path(source, TransferKind::Directory, dest, dir_dest).await?;
        if overwrite {
            debug!(path = %dest_path.display(), "Cleaning up destination directory");
            self.shell(&format!("rm -rf '{}'", dest_path.display())).await?;
        }
        debug!(source = %source.display(), dest = %dest_path.display(), "Pushing directory");
        self.inner.push_dir(source, &dest_path, 0o777).await.context("Failed to push directory")
    }

    /// Pushes a directory to an exact path on the device (with progress).
    ///
    /// # Arguments
    /// * `source` - Local directory path to push
    /// * `dest` - Exact destination directory path on device
    /// * `overwrite` - Whether to clean up destination directory before pushing
    /// * `progress_sender` - Sender for progress updates
    #[instrument(level = "debug", skip(self, progress_sender), err)]
    pub(super) async fn push_dir_to_path_with_progress(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
        progress_sender: UnboundedSender<DirectoryTransferProgress>,
    ) -> Result<()> {
        self.push_dir_with_progress_mode(
            source,
            dest,
            overwrite,
            progress_sender,
            DirectoryPushDestination::ExactPath,
        )
        .await
    }

    async fn push_dir_with_progress_mode(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
        progress_sender: UnboundedSender<DirectoryTransferProgress>,
        dir_dest: DirectoryPushDestination,
    ) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path =
            self.resolve_push_dest_path(source, TransferKind::Directory, dest, dir_dest).await?;
        if overwrite {
            debug!(path = %dest_path.display(), "Cleaning up destination directory");
            self.shell(&format!("rm -rf '{}'", dest_path.display())).await?;
        }
        self.inner
            .push_dir_with_progress(source, &dest_path, 0o777, progress_sender)
            .await
            .context("Failed to push directory")
    }

    /// Pushes raw bytes to a file on the device
    #[instrument(level = "debug", skip(self, bytes), fields(len = bytes.len()), err)]
    pub(super) async fn push_bytes(&self, mut bytes: &[u8], remote_path: &UnixPath) -> Result<()> {
        self.inner.push(&mut bytes, remote_path, 0o777).await.context("Failed to push bytes")
    }

    /// Pulls a file from the device
    ///
    /// # Arguments
    /// * `source_file` - Source path on the device
    /// * `dest_file` - Local path to save the file
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn pull(&self, source_file: &UnixPath, dest_file: &Path) -> Result<PathBuf> {
        let source_stat =
            self.inner.stat(source_file).await.context("Failed to stat source file")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::RegularFile,
            "Source path is not a regular file: {}",
            source_file.display()
        );

        self.pull_file_with_kind(source_file, dest_file, TransferKind::File).await
    }

    async fn pull_file_with_kind(
        &self,
        source_file: &UnixPath,
        dest_file: &Path,
        source_kind: TransferKind,
    ) -> Result<PathBuf> {
        let dest_path = Self::resolve_pull_dest_path(source_file, source_kind, dest_file)?;
        let mut file = File::create(&dest_path).await?;
        self.inner.pull(source_file, &mut file).await?;
        Ok(dest_path)
    }

    /// Pulls a directory from the device
    ///
    /// # Arguments
    /// * `source` - Source path on the device
    /// * `dest` - Local path to save the directory
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn pull_dir(&self, source: &UnixPath, dest: &Path) -> Result<PathBuf> {
        let source_stat =
            self.inner.stat(source).await.context("Failed to stat source directory")?;
        ensure!(
            source_stat.file_mode == UnixFileStatus::Directory,
            "Source path is not a directory: {}",
            source.display()
        );

        self.pull_dir_with_kind(source, dest, TransferKind::Directory).await
    }

    async fn pull_dir_with_kind(
        &self,
        source: &UnixPath,
        dest: &Path,
        source_kind: TransferKind,
    ) -> Result<PathBuf> {
        let dest_path = Self::resolve_pull_dest_path(source, source_kind, dest)?;
        // Ensure the destination directory exists before pulling
        // For directory pulls, it's convenient to create the destination path automatically.
        // This mirrors typical `adb pull` behavior when targeting a new directory path.
        fs::create_dir_all(&dest_path).await.with_context(|| {
            format!("Failed to create destination directory: {}", dest_path.display())
        })?;
        self.inner.pull_dir(source, &dest_path).await.context("Failed to pull directory")?;
        Ok(dest_path)
    }

    /// Pulls an item from the device.
    #[instrument(level = "debug", skip(self, remote_path, local_path))]
    pub(super) async fn pull_any(&self, remote_path: &UnixPath, local_path: &Path) -> Result<()> {
        let stat = self.inner.stat(remote_path).await.context("Stat command failed")?;
        let source_kind = TransferKind::from_remote_status(stat.file_mode)
            .with_context(|| format!("Unsupported file type: {:?}", stat.file_mode))?;

        match source_kind {
            TransferKind::Directory => {
                // If destination exists and is a regular file, this is an error.
                if local_path.exists() && local_path.is_file() {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        remote_path.display(),
                        local_path.display()
                    );
                }
                // `pull_dir` will ensure the destination directory exists (create_dir_all).
                self.pull_dir_with_kind(remote_path, local_path, source_kind).await?;
            }
            TransferKind::File => {
                // For files, allow non-existent destination paths as long as the parent exists.
                if let Some(parent) = local_path.parent() {
                    ensure!(
                        parent.exists(),
                        "Parent directory '{}' does not exist",
                        parent.display()
                    );
                }
                // If destination is a directory, `pull` will place the file inside it via
                // `resolve_pull_dest_path`. Otherwise it writes to the given file path.
                self.pull_file_with_kind(remote_path, local_path, source_kind).await?;
            }
        }
        Ok(())
    }

    /// Pushes an item to the device
    #[instrument(level = "debug", skip(self, source, dest), err)]
    pub(super) async fn push_any(&self, source: &Path, dest: &UnixPath) -> Result<()> {
        ensure!(source.exists(), "Source path does not exist: {}", source.display());
        if source.is_dir() {
            self.push_dir(source, dest, false).await?;
        } else if source.is_file() {
            self.push(source, dest).await?;
        } else {
            bail!("Unsupported source file type: {}", source.display());
        }
        Ok(())
    }

    /// Returns true if a directory exists on the device
    #[instrument(level = "debug", skip(self), err)]
    pub(super) async fn dir_exists(&self, path: &UnixPath) -> Result<bool> {
        match self.inner.stat(path).await {
            Ok(stat) => Ok(stat.file_mode == UnixFileStatus::Directory),
            Err(e) => {
                trace!(error = &e as &dyn std::error::Error, path = %path.display(), "stat failed");
                Ok(false)
            }
        }
    }
}
