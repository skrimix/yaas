use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use forensic_adb::{DirectoryTransferProgress, UnixFileStatus, UnixPath, UnixPathBuf};
use tokio::{
    fs::{self, File},
    io::BufReader,
    sync::mpsc::UnboundedSender,
};
use tracing::{debug, instrument, trace};

use super::AdbDevice;

impl AdbDevice {
    /// Resolves the effective remote destination path for a push operation.
    ///
    /// Behavior:
    /// - If `dest` exists on the device and is a directory, the source file or directory name
    ///   is appended (push into that directory).
    /// - If `dest` exists and is a regular file, then:
    ///   - pushing a local file overwrites that remote file path;
    ///   - pushing a local directory is rejected (cannot push a dir to an existing file).
    /// - If `dest` does not exist but its parent exists, use `dest` as-is (the caller will create
    ///   directories/files as needed during the transfer).
    /// - If neither `dest` nor its parent exists, returns an error.
    ///
    /// This mirrors common `adb push` conventions and ensures we never silently place content
    /// at an unexpected path.
    #[instrument(level = "debug", ret, err)]
    pub(super) async fn resolve_push_dest_path(
        &self,
        source: &Path,
        dest: &UnixPath,
    ) -> Result<UnixPathBuf> {
        let source_name = source
            .file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")?;

        // Check if destination exists
        let dest_stat = self.inner.stat(dest).await;

        if let Ok(stat) = dest_stat {
            if stat.file_mode == UnixFileStatus::Directory {
                // If destination is a directory, append source file name
                Ok(UnixPathBuf::from(dest).join(source_name))
            } else if source.is_dir() {
                // Can't push directory to existing file
                bail!(
                    "Cannot push directory '{}' to existing file '{}'",
                    source.display(),
                    dest.display()
                )
            } else {
                // Use destination path as is
                Ok(UnixPathBuf::from(dest))
            }
        } else if let Some(parent) = dest.parent() {
            if self.inner.stat(parent).await.is_ok() {
                Ok(UnixPathBuf::from(dest))
            } else {
                bail!("Parent directory '{}' does not exist", parent.display())
            }
        } else {
            bail!("Invalid destination path: no parent directory")
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
    pub(super) async fn resolve_pull_dest_path(
        &self,
        source: &UnixPath,
        dest: &Path,
    ) -> Result<PathBuf> {
        let source_name = source
            .file_name()
            .context("Source path has no file name")?
            .to_str()
            .context("Source file name is not valid UTF-8")?;

        if dest.exists() {
            if dest.is_dir() {
                // If destination is a directory, append source file name
                Ok(dest.join(source_name))
            } else {
                // Can't pull to existing file if source is directory
                let source_is_dir = matches!(self.inner.stat(source).await, Ok(stat) if stat.file_mode == UnixFileStatus::Directory);
                if source_is_dir {
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

        let dest_path = self.resolve_push_dest_path(source_file, dest_file).await?;
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
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
        if overwrite {
            debug!(path = %dest_path.display(), "Cleaning up destination directory");
            self.shell(&format!("rm -rf '{}'", dest_path.display())).await?;
        }
        debug!(source = %source.display(), dest = %dest_path.display(), "Pushing directory");
        self.inner.push_dir(source, &dest_path, 0o777).await.context("Failed to push directory")
    }

    /// Pushes a directory to the device (with progress)
    ///
    /// # Arguments
    /// * `source` - Local directory path to push
    /// * `dest_dir` - Destination directory path on device
    /// * `overwrite` - Whether to clean up destination directory before pushing
    /// * `progress_sender` - Sender for progress updates
    #[instrument(level = "debug", skip(self, progress_sender), err)]
    pub(super) async fn push_dir_with_progress(
        &self,
        source: &Path,
        dest: &UnixPath,
        overwrite: bool,
        progress_sender: UnboundedSender<DirectoryTransferProgress>,
    ) -> Result<()> {
        ensure!(
            source.is_dir(),
            "Source path does not exist or is not a directory: {}",
            source.display()
        );

        let dest_path = self.resolve_push_dest_path(source, dest).await?;
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

        let dest_path = self.resolve_pull_dest_path(source_file, dest_file).await?;
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

        let dest_path = self.resolve_pull_dest_path(source, dest).await?;
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

        match stat.file_mode {
            UnixFileStatus::Directory => {
                // If destination exists and is a regular file, this is an error.
                if local_path.exists() && local_path.is_file() {
                    bail!(
                        "Cannot pull directory '{}' to existing file '{}'",
                        remote_path.display(),
                        local_path.display()
                    );
                }
                // `pull_dir` will ensure the destination directory exists (create_dir_all).
                self.pull_dir(remote_path, local_path).await?;
            }
            UnixFileStatus::RegularFile => {
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
                self.pull(remote_path, local_path).await?;
            }
            other => bail!("Unsupported file type: {:?}", other),
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
