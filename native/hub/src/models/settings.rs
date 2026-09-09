use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use rinf::SignalPiece;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemePreference {
    #[default]
    Dark,
    Light,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NavigationRailLabelVisibility {
    #[default]
    Selected,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum ConnectionKind {
    #[default]
    Usb,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PopularityRange {
    Day1,
    #[default]
    Day7,
    Day30,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum DownloadCleanupPolicy {
    #[default]
    DeleteAfterInstall,
    KeepOneVersion,
    KeepTwoVersions,
    KeepAllVersions,
}

impl DownloadCleanupPolicy {
    pub(crate) fn applies_at(
        self,
        timing: DownloadCleanupTiming,
        completed: DownloadCleanupTiming,
    ) -> bool {
        match self {
            Self::DeleteAfterInstall => completed == DownloadCleanupTiming::AfterInstall,
            Self::KeepOneVersion | Self::KeepTwoVersions => timing == completed,
            Self::KeepAllVersions => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum DownloadCleanupTiming {
    #[default]
    AfterInstall,
    AfterDownload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece, Default)]
pub(crate) enum DownloadMode {
    Streamed,
    #[default]
    Staged,
}

/// Release channel used for application update checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateChannel {
    Stable,
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        if env!("YAAS_RELEASE_CHANNEL") == "nightly" { Self::Nightly } else { Self::Stable }
    }
}

impl UpdateChannel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SignalPiece)]
#[serde(default)]
pub(crate) struct Settings {
    pub installation_id: String,
    pub update_channel: UpdateChannel,
    pub check_updates_on_startup: bool,
    pub active_downloader_config_id: String,
    pub rclone_remote_name: String,
    pub adb_path: String,
    pub preferred_connection_type: ConnectionKind,
    downloads_location: String,
    backups_location: String,
    pub bandwidth_limit: String,
    pub cleanup_policy: DownloadCleanupPolicy,
    pub cleanup_timing: DownloadCleanupTiming,
    pub download_mode: DownloadMode,
    /// Also write legacy release.json metadata alongside download.json
    pub write_legacy_release_json: bool,
    /// Locale code (language) for the UI
    locale_code: String,
    navigation_rail_label_visibility: NavigationRailLabelVisibility,
    startup_page_key: String,
    /// Whether to use system/dynamic color when available
    use_system_color: bool,
    /// Seed color key from a fixed palette (e.g. "deep_purple")
    seed_color_key: String,
    /// Preferred theme mode (dark is default)
    theme_preference: ThemePreference,
    /// List of favorited apps (by true package name)
    favorite_packages: Vec<String>,
    /// Discover and auto-connect ADB over Wi‑Fi devices via mDNS
    pub mdns_auto_connect: bool,
    /// Popularity display range
    popularity_range: PopularityRange,
    /// Auto reinstall app on incompatible update or downgrade (requires debuggable app for data backup)
    pub auto_reinstall_on_conflict: bool,
    /// Back up available app data before explicit uninstalls.
    pub auto_backup_on_uninstall: bool,
    /// Enable the experimental native casting feature
    pub experimental_native_cast: bool,
}

impl Default for Settings {
    /// For serde only. Use `Settings::new` instead.
    fn default() -> Self {
        Self {
            installation_id: Uuid::new_v4().to_string(),
            update_channel: UpdateChannel::default(),
            check_updates_on_startup: true,
            active_downloader_config_id: String::new(),
            rclone_remote_name: "FFA-90".to_string(),
            adb_path: "adb".to_string(),
            preferred_connection_type: ConnectionKind::default(),
            downloads_location: dirs::download_dir()
                .map(|dir| dir.join("YAAS"))
                .unwrap_or_else(|| crate::resolve_app_dir(false).join("downloads"))
                .to_string_lossy()
                .to_string(),
            backups_location: dirs::document_dir()
                .map(|dir| dir.join("YAAS_backups"))
                .unwrap_or_else(|| crate::resolve_app_dir(false).join("backups"))
                .to_string_lossy()
                .to_string(),
            bandwidth_limit: String::new(),
            cleanup_policy: DownloadCleanupPolicy::default(),
            cleanup_timing: DownloadCleanupTiming::default(),
            download_mode: DownloadMode::default(),
            write_legacy_release_json: false,
            locale_code: "system".to_string(),
            navigation_rail_label_visibility: NavigationRailLabelVisibility::default(),
            startup_page_key: "home".to_string(),
            use_system_color: false,
            seed_color_key: "deep_purple".to_string(),
            theme_preference: ThemePreference::Dark,
            favorite_packages: Vec::new(),
            mdns_auto_connect: true,
            popularity_range: PopularityRange::default(),
            auto_reinstall_on_conflict: true,
            auto_backup_on_uninstall: true,
            experimental_native_cast: false,
        }
    }
}

impl Settings {
    pub(crate) fn new(portable_mode: bool) -> Self {
        let mut settings = Settings::default();

        if portable_mode {
            settings.downloads_location = "downloads".to_string();
            settings.backups_location = "backups".to_string();
        }

        settings
    }

    pub(crate) fn load_from_file(settings_file: &Path, portable_mode: bool) -> Result<Self> {
        let file_content =
            fs::read_to_string(settings_file).context("Failed to read settings file")?;

        let mut settings: Settings =
            serde_json::from_str(&file_content).context("Failed to parse settings file")?;

        let original = settings.clone();
        let app_dir = settings_file.parent().context("Failed to get settings directory")?;
        if let Err(error) = settings.prepare_directories(app_dir, portable_mode) {
            warn!(error = %format!("{error:#}"), "Failed to prepare settings directories");
        }
        if settings != original
            && let Err(error) = settings.save_to_file(settings_file)
        {
            warn!(error = %format!("{error:#}"), "Failed to save updated settings paths");
        }

        Ok(settings)
    }

    pub(crate) fn prepare_directories(
        &mut self,
        app_dir: &Path,
        portable_mode: bool,
    ) -> Result<()> {
        let defaults = Self::new(portable_mode);
        let downloads_fallback = app_dir.join("downloads");
        let backups_fallback = app_dir.join("backups");
        prepare_directory(
            &mut self.downloads_location,
            &defaults.downloads_location,
            (!portable_mode).then_some(downloads_fallback.as_path()),
        )
        .context("Failed to prepare downloads directory")?;
        prepare_directory(
            &mut self.backups_location,
            &defaults.backups_location,
            (!portable_mode).then_some(backups_fallback.as_path()),
        )
        .context("Failed to prepare backups directory")?;
        Ok(())
    }

    pub(crate) fn save_to_file(&self, settings_file: &Path) -> Result<()> {
        // TODO: Validate settings

        let settings_json =
            serde_json::to_string_pretty(self).context("Failed to serialize settings")?;
        fs::write(settings_file, settings_json).context("Failed to write settings file")?;
        Ok(())
    }

    pub(crate) fn downloads_location(&self) -> PathBuf {
        PathBuf::from(&self.downloads_location)
    }

    pub(crate) fn backups_location(&self) -> PathBuf {
        PathBuf::from(&self.backups_location)
    }
}

fn prepare_directory(location: &mut String, default: &str, fallback: Option<&Path>) -> Result<()> {
    let path = Path::new(location);
    if path != Path::new(default) && Some(path) != fallback {
        if path.is_dir() {
            return Ok(());
        }
        warn!(path = %path.display(), "Configured directory is unavailable, using default");
        *location = default.to_string();
    }

    let path = Path::new(location);
    if let Err(error) = ensure_writable_directory(path) {
        let Some(fallback) = fallback.filter(|fallback| *fallback != path) else {
            return Err(error);
        };
        warn!(
            path = %path.display(),
            fallback = %fallback.display(),
            error = %format!("{error:#}"),
            "Default directory is unavailable, using app data"
        );
        ensure_writable_directory(fallback)?;
        *location = fallback.to_string_lossy().into_owned();
    }
    Ok(())
}

fn ensure_writable_directory(path: &Path) -> Result<()> {
    if path.is_absolute() {
        let parent = path.parent().context("Failed to get directory parent")?;
        ensure!(parent.is_dir(), "Directory parent ({}) is unavailable", parent.display());
    }
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory {}", path.display()))?;
    // Existing directories can still deny writes, for example with controlled folder access.
    let mut probe = tempfile::Builder::new()
        .prefix(".yaas-write-test-")
        .tempfile_in(path)
        .with_context(|| format!("Failed to create a file in {}", path.display()))?;
    probe.write_all(b"YAAS").with_context(|| format!("Failed to write in {}", path.display()))?;
    probe.close().with_context(|| format!("Failed to remove test file in {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::{
        DownloadCleanupPolicy as Policy, DownloadCleanupTiming as Timing, Settings,
        prepare_directory,
    };

    #[test]
    fn writable_default_directory_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("preferred");
        let fallback = dir.path().join("fallback");
        let default = preferred.to_string_lossy().into_owned();
        let mut location = default.clone();

        prepare_directory(&mut location, &default, Some(&fallback)).unwrap();

        assert_eq!(Path::new(&location), preferred);
        assert_eq!(fs::read_dir(&preferred).unwrap().count(), 0);
        assert!(!fallback.exists());
    }

    #[test]
    fn unavailable_default_directory_falls_back_to_app_data() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, "existing file").unwrap();
        let fallback = dir.path().join("fallback");

        for preferred in [blocked.clone(), blocked.join("YAAS"), dir.path().join("missing/YAAS")] {
            let default = preferred.to_string_lossy().into_owned();
            let mut location = default.clone();

            prepare_directory(&mut location, &default, Some(&fallback)).unwrap();

            assert_eq!(Path::new(&location), fallback);
            assert_eq!(fs::read_dir(&fallback).unwrap().count(), 0);
        }
        assert_eq!(fs::read_to_string(&blocked).unwrap(), "existing file");
        assert!(!dir.path().join("missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn existing_unwritable_default_directory_falls_back_to_app_data() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("protected");
        fs::create_dir(&preferred).unwrap();
        let permissions = fs::metadata(&preferred).unwrap().permissions();
        fs::set_permissions(&preferred, fs::Permissions::from_mode(0o555)).unwrap();
        let fallback = dir.path().join("fallback");
        let default = preferred.to_string_lossy().into_owned();
        let mut location = default.clone();

        // Privileged users may still be able to write to read-only directories.
        if tempfile::tempfile_in(&preferred).is_ok() {
            fs::set_permissions(&preferred, permissions).unwrap();
            return;
        }
        let result = prepare_directory(&mut location, &default, Some(&fallback));
        fs::set_permissions(&preferred, permissions).unwrap();

        result.unwrap();
        assert_eq!(Path::new(&location), fallback);
        assert_eq!(fs::read_dir(&fallback).unwrap().count(), 0);
    }

    #[test]
    fn existing_custom_directory_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom");
        fs::create_dir(&custom).unwrap();
        let preferred = dir.path().join("preferred");
        let fallback = dir.path().join("fallback");
        let mut location = custom.to_string_lossy().into_owned();

        prepare_directory(&mut location, &preferred.to_string_lossy(), Some(&fallback)).unwrap();

        assert_eq!(Path::new(&location), custom);
        assert!(!preferred.exists());
        assert!(!fallback.exists());
    }

    #[test]
    fn directory_errors_are_returned_when_fallback_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("preferred");
        let fallback = dir.path().join("fallback");
        fs::write(&preferred, "blocked").unwrap();
        fs::write(&fallback, "blocked").unwrap();
        let default = preferred.to_string_lossy().into_owned();

        for fallback in [None, Some(fallback.as_path())] {
            let mut location = default.clone();
            assert!(prepare_directory(&mut location, &default, fallback).is_err());
            assert_eq!(location, default);
        }
    }

    #[test]
    fn missing_custom_directory_uses_a_writable_default() {
        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("preferred");
        let fallback = dir.path().join("fallback");
        let mut location = dir.path().join("missing").to_string_lossy().into_owned();

        prepare_directory(&mut location, &preferred.to_string_lossy(), Some(&fallback)).unwrap();

        assert_eq!(Path::new(&location), preferred);
        assert!(preferred.is_dir());
    }

    #[test]
    fn saved_fallback_directories_are_recreated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let settings_file = dir.path().join("settings.json");
        let settings = Settings {
            downloads_location: dir.path().join("downloads").to_string_lossy().into_owned(),
            backups_location: dir.path().join("backups").to_string_lossy().into_owned(),
            bandwidth_limit: "12M".into(),
            ..Settings::new(false)
        };
        settings.save_to_file(&settings_file).unwrap();

        let loaded = Settings::load_from_file(&settings_file, false).unwrap();

        assert_eq!(loaded, settings);
        assert!(loaded.downloads_location().is_dir());
        assert!(loaded.backups_location().is_dir());
        assert_eq!(Settings::load_from_file(&settings_file, false).unwrap(), settings);
    }

    #[test]
    fn directory_errors_do_not_discard_saved_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings_file = dir.path().join("settings.json");
        let downloads = dir.path().join("downloads");
        fs::write(&downloads, "blocked").unwrap();
        let settings = Settings {
            downloads_location: downloads.to_string_lossy().into_owned(),
            backups_location: dir.path().join("backups").to_string_lossy().into_owned(),
            bandwidth_limit: "12M".into(),
            ..Settings::new(false)
        };
        settings.save_to_file(&settings_file).unwrap();

        assert_eq!(Settings::load_from_file(&settings_file, false).unwrap(), settings);
        let saved: Settings =
            serde_json::from_str(&fs::read_to_string(&settings_file).unwrap()).unwrap();
        assert_eq!(saved, settings);
    }

    #[test]
    fn update_channel_defaults_and_round_trips() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.update_channel, super::UpdateChannel::default());
        assert!(settings.check_updates_on_startup);
        let nightly: Settings = serde_json::from_str(r#"{"update_channel":"nightly"}"#).unwrap();
        assert_eq!(nightly.update_channel, super::UpdateChannel::Nightly);
        let json = serde_json::to_string(&nightly).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), nightly);
    }

    #[test]
    fn startup_update_checks_can_be_disabled() {
        let settings: Settings =
            serde_json::from_str(r#"{"check_updates_on_startup":false}"#).unwrap();
        assert!(!settings.check_updates_on_startup);
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn uninstall_backup_defaults_for_existing_settings() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.auto_backup_on_uninstall);
    }

    #[test]
    fn uninstall_backup_can_be_disabled_and_round_trips() {
        let settings: Settings =
            serde_json::from_str(r#"{"auto_backup_on_uninstall":false}"#).unwrap();
        assert!(!settings.auto_backup_on_uninstall);
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn cleanup_timing_defaults_for_existing_settings() {
        let settings: Settings =
            serde_json::from_str(r#"{"cleanup_policy":"KeepTwoVersions"}"#).unwrap();
        assert_eq!(settings.cleanup_policy, Policy::KeepTwoVersions);
        assert_eq!(settings.cleanup_timing, Timing::AfterInstall);
    }

    #[test]
    fn cleanup_timing_round_trips() {
        let settings = Settings { cleanup_timing: Timing::AfterDownload, ..Settings::default() };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn cleanup_only_runs_at_the_selected_stage() {
        for timing in [Timing::AfterInstall, Timing::AfterDownload] {
            for completed in [Timing::AfterInstall, Timing::AfterDownload] {
                assert_eq!(
                    Policy::DeleteAfterInstall.applies_at(timing, completed),
                    completed == Timing::AfterInstall
                );
                assert!(!Policy::KeepAllVersions.applies_at(timing, completed));
                for policy in [Policy::KeepOneVersion, Policy::KeepTwoVersions] {
                    assert_eq!(policy.applies_at(timing, completed), timing == completed);
                }
            }
        }
    }
}
