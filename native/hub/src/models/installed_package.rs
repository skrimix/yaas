use anyhow::Result;
use lazy_regex::{Lazy, Regex, lazy_regex};
use rinf::SignalPiece;
use serde::{Deserialize, Serialize};

/// Regex pattern to detect known rename markers in package names.
pub(super) static RENAME_PATTERN: Lazy<Regex> = lazy_regex!(r"(^mr\.)|(^mrf\.)|(\.mrf\.)|(\.jjb)");

fn is_package_renamed(package_name: &str) -> bool {
    RENAME_PATTERN.is_match(package_name)
}

/// Represents the size information of an installed application
#[derive(Debug, Clone, Default, Deserialize, Serialize, SignalPiece)]
pub(crate) struct AppSize {
    app: u64,
    data: u64,
    cache: u64,
}

/// Represents an installed package on the device with its metadata
#[derive(Debug, Clone, Default, Deserialize, Serialize, SignalPiece)]
pub(crate) struct InstalledPackage {
    uid: u64,
    system: bool,
    package_name: String,
    version_code: u64,
    version_name: String,
    label: String,
    launchable: bool,
    vr: bool,
    size: AppSize,
    /// Whether the package name contains known rename markers
    #[serde(default)]
    is_package_renamed: bool,
}

/// Parses the output of list_apps.dex command
pub(crate) fn parse_list_apps_dex(
    dex_output: &str,
) -> Result<Vec<InstalledPackage>, serde_json::Error> {
    let mut packages: Vec<InstalledPackage> = serde_json::from_str(dex_output)?;
    for pkg in &mut packages {
        pkg.is_package_renamed = is_package_renamed(&pkg.package_name);
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_apps_dex() {
        let output = r#"[
{
  "uid": 10029,
  "system": true,
  "package_name": "com.oculus.shellenv",
  "version_code": 640490640,
  "version_name": "69.0.0.556.352",
  "label": "ShellEnv",
  "launchable": true,
  "vr": true,
  "size": {
    "app": 1,
    "data": 2,
    "cache": 3
  }
},
{
  "uid": 10148,
  "system": false,
  "package_name": "com.meta.pclinkservice.server",
  "version_code": 700100136,
  "version_name": "8.0.0.0.110",
  "label": "PCLinkService",
  "launchable": false,
  "vr": false,
  "size": {
    "app": 3,
    "data": 4,
    "cache": 5
  }
},
{
  "uid": 10038,
  "system": true,
  "package_name": "com.oculus.mrservice",
  "version_code": 640488307,
  "version_name": "69.0.0.562.335",
  "label": "com.oculus.mrservice",
  "launchable": false,
  "vr": false,
  "size": {
    "app": 6,
    "data": 7,
    "cache": 8
  }
},
{
  "uid": 10131,
  "system": true,
  "package_name": "com.oculus.configuration",
  "version_code": 639421834,
  "version_name": "69.0.0.628.468",
  "label": "com.oculus.configuration.OsConfigApplication",
  "launchable": false,
  "vr": false,
  "size": {
    "app": 9,
    "data": 10,
    "cache": 11
  }
}
]
"#;
        let packages = parse_list_apps_dex(output).unwrap();

        assert_eq!(packages.len(), 4);

        assert!(packages[0].system);
        assert!(!packages[1].system);
        assert!(packages[2].system);
        assert!(packages[3].system);

        assert_eq!(packages[0].package_name, "com.oculus.shellenv");
        assert_eq!(packages[1].package_name, "com.meta.pclinkservice.server");
        assert_eq!(packages[2].package_name, "com.oculus.mrservice");
        assert_eq!(packages[3].package_name, "com.oculus.configuration");

        assert_eq!(packages[0].version_code, 640490640);
        assert_eq!(packages[1].version_code, 700100136);
        assert_eq!(packages[2].version_code, 640488307);
        assert_eq!(packages[3].version_code, 639421834);

        assert_eq!(packages[0].version_name, "69.0.0.556.352");
        assert_eq!(packages[1].version_name, "8.0.0.0.110");
        assert_eq!(packages[2].version_name, "69.0.0.562.335");
        assert_eq!(packages[3].version_name, "69.0.0.628.468");

        assert_eq!(packages[0].label, "ShellEnv");
        assert_eq!(packages[1].label, "PCLinkService");
        assert_eq!(packages[2].label, "com.oculus.mrservice");
        assert_eq!(packages[3].label, "com.oculus.configuration.OsConfigApplication");

        assert!(packages[0].launchable);
        assert!(!packages[1].launchable);
        assert!(!packages[2].launchable);
        assert!(!packages[3].launchable);

        assert!(packages[0].vr);
        assert!(!packages[1].vr);
        assert!(!packages[2].vr);
        assert!(!packages[3].vr);

        assert_eq!(packages[0].size.app, 1);
        assert_eq!(packages[0].size.data, 2);
        assert_eq!(packages[0].size.cache, 3);

        assert_eq!(packages[1].size.app, 3);
        assert_eq!(packages[1].size.data, 4);
        assert_eq!(packages[1].size.cache, 5);

        assert_eq!(packages[2].size.app, 6);
        assert_eq!(packages[2].size.data, 7);
        assert_eq!(packages[2].size.cache, 8);

        assert_eq!(packages[3].size.app, 9);
        assert_eq!(packages[3].size.data, 10);
        assert_eq!(packages[3].size.cache, 11);
    }

    #[test]
    fn test_parse_list_apps_dex_empty() {
        let output = r#"[
        ]"#;
        let parsed = parse_list_apps_dex(output).unwrap();
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn test_parse_list_apps_dex_invalid() {
        let output = r#"[
        {
            "uid": 10029,
        }
        ]"#;
        assert!(parse_list_apps_dex(output).is_err());
    }

    #[test]
    fn test_is_package_renamed_mr_prefix() {
        assert!(is_package_renamed("mr.com.example.app"));
        assert!(is_package_renamed("mr.test.package"));
    }

    #[test]
    fn test_is_package_renamed_normal_packages() {
        // Normal packages should not be detected as renamed
        assert!(!is_package_renamed("com.oculus.shellenv"));
        assert!(!is_package_renamed("com.meta.pclinkservice.server"));
        assert!(!is_package_renamed("com.oculus.mrservice")); // mrservice != mr. prefix
        assert!(!is_package_renamed("com.example.app"));
        assert!(!is_package_renamed("org.mozilla.firefox"));
    }

    #[test]
    fn test_parse_list_apps_dex_with_renamed_packages() {
        let output = r#"[
{
  "uid": 10001,
  "system": false,
  "package_name": "mr.com.example.game",
  "version_code": 100,
  "version_name": "1.0.0",
  "label": "Renamed Game",
  "launchable": true,
  "vr": true,
  "size": {"app": 100, "data": 50, "cache": 10}
},
{
  "uid": 10002,
  "system": false,
  "package_name": "com.example.normal",
  "version_code": 200,
  "version_name": "2.0.0",
  "label": "Normal App",
  "launchable": true,
  "vr": false,
  "size": {"app": 200, "data": 100, "cache": 20}
},
{
  "uid": 10003,
  "system": false,
  "package_name": "mrf.com.another.game",
  "version_code": 300,
  "version_name": "3.0.0",
  "label": "Another Renamed",
  "launchable": true,
  "vr": true,
  "size": {"app": 300, "data": 150, "cache": 30}
}
]"#;
        let packages = parse_list_apps_dex(output).unwrap();

        assert_eq!(packages.len(), 3);

        // First package: mr. prefix - should be renamed
        assert!(packages[0].is_package_renamed);
        assert_eq!(packages[0].package_name, "mr.com.example.game");

        // Second package: normal - should NOT be renamed
        assert!(!packages[1].is_package_renamed);
        assert_eq!(packages[1].package_name, "com.example.normal");

        // Third package: mrf. prefix - should be renamed
        assert!(packages[2].is_package_renamed);
        assert_eq!(packages[2].package_name, "mrf.com.another.game");
    }
}
