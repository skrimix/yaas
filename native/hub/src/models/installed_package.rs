use anyhow::{Context, Result};
use serde::Deserialize;

use crate::messages as proto;
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppSize {
    app: u64,
    data: u64,
    cache: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InstalledPackage {
    uid: u64,
    system: bool,
    package_name: String,
    version_code: u64,
    version_name: String,
    label: String,
    launchable: bool,
    vr: bool,
    #[serde(default)]
    size: AppSize,
}

impl InstalledPackage {
    pub fn update_sizes(&mut self, app_size: u64, data_size: u64, cache_size: u64) {
        self.size = AppSize { app: app_size, data: data_size, cache: cache_size };
    }

    // TODO: use the proto struct directly
    pub fn into_proto(self) -> proto::InstalledPackage {
        proto::InstalledPackage {
            uid: self.uid,
            system: self.system,
            package_name: self.package_name,
            version_code: self.version_code,
            version_name: self.version_name,
            label: self.label,
            launchable: self.launchable,
            vr: self.vr,
            size: Some(proto::AppSize {
                app: self.size.app,
                data: self.size.data,
                cache: self.size.cache,
            }),
        }
    }
}

fn parse_list_apps_dex(dex_output: &str) -> Result<Vec<InstalledPackage>, serde_json::Error> {
    serde_json::from_str(dex_output)
}

fn populate_sizes(packages: &mut [InstalledPackage], dumpsys_output: &str) -> Result<()> {
    let mut package_names: Vec<String> = Vec::new();
    let mut app_sizes: Vec<u64> = Vec::new();
    let mut data_sizes: Vec<u64> = Vec::new();
    let mut cache_sizes: Vec<u64> = Vec::new();

    for line in dumpsys_output.lines() {
        if line.starts_with("Package Names:") {
            package_names = serde_json::from_str(
                line.split_once(':').context("unexpected package names format")?.1.trim(),
            )
            .context("unexpected package names format")?;
        } else if line.starts_with("App Sizes:") {
            app_sizes = serde_json::from_str(
                line.split_once(':').context("unexpected app sizes format")?.1.trim(),
            )
            .context("unexpected app sizes format")?;
        } else if line.starts_with("App Data Sizes:") {
            data_sizes = serde_json::from_str(
                line.split_once(':').context("unexpected data sizes format")?.1.trim(),
            )
            .context("unexpected data sizes format")?;
        } else if line.starts_with("Cache Sizes:") {
            cache_sizes = serde_json::from_str(
                line.split_once(':').context("unexpected cache sizes format")?.1.trim(),
            )
            .context("unexpected cache sizes format")?;
        }
    }

    if package_names.is_empty()
        || app_sizes.is_empty()
        || data_sizes.is_empty()
        || cache_sizes.is_empty()
    {
        return Err(anyhow::anyhow!("missing required size information in dumpsys output"));
    }

    for (i, package_name) in package_names.iter().enumerate() {
        if let Some(package) = packages.iter_mut().find(|p| &p.package_name == package_name) {
            package.update_sizes(app_sizes[i], data_sizes[i], cache_sizes[i]);
        }
    }

    Ok(())
}

pub fn packages_from_device_output(
    dex_output: &str,
    dumpsys_output: &str,
) -> Result<Vec<InstalledPackage>> {
    let mut packages =
        parse_list_apps_dex(dex_output).context("failed to parse list_apps.dex output")?;
    populate_sizes(&mut packages, dumpsys_output).context("failed to populate package sizes")?;
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
  "vr": true
},
{
  "uid": 10148,
  "system": false,
  "package_name": "com.meta.pclinkservice.server",
  "version_code": 700100136,
  "version_name": "8.0.0.0.110",
  "label": "PCLinkService",
  "launchable": false,
  "vr": false
},
{
  "uid": 10038,
  "system": true,
  "package_name": "com.oculus.mrservice",
  "version_code": 640488307,
  "version_name": "69.0.0.562.335",
  "label": "com.oculus.mrservice",
  "launchable": false,
  "vr": false
},
{
  "uid": 10131,
  "system": true,
  "package_name": "com.oculus.configuration",
  "version_code": 639421834,
  "version_name": "69.0.0.628.468",
  "label": "com.oculus.configuration.OsConfigApplication",
  "launchable": false,
  "vr": false
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

        for package in packages {
            assert!(package.size.app == 0);
            assert!(package.size.data == 0);
            assert!(package.size.cache == 0);
        }
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
    fn test_update_packages_with_sizes() {
        let mut packages = vec![
            InstalledPackage {
                uid: 10029,
                system: true,
                package_name: "com.oculus.avatareditor".to_string(),
                version_code: 640490640,
                version_name: "69.0.0.556.352".to_string(),
                label: "AvatarEditor".to_string(),
                launchable: true,
                vr: true,
                size: AppSize::default(),
            },
            InstalledPackage {
                uid: 10148,
                system: false,
                package_name: "de.blinkt.openvpn".to_string(),
                version_code: 700100136,
                version_name: "8.0.0.0.110".to_string(),
                label: "OpenVPN".to_string(),
                launchable: false,
                vr: false,
                size: AppSize::default(),
            },
        ];

        let dumpsys_output = r#"Latency: 1ms [512B Data Write]
Recent Disk Write Speed (kB/s) = 33635
Data-Free: 6264024K / 55247244K total = 11% free
Cache-Free: 6264024K / 55247244K total = 11% free
System-Free: 0K / 1801992K total = 0% free
File-based Encryption: true
App Size: 34883465664
App Data Size: 365203760
App Cache Size: 73829424
Photos Size: 130072
Videos Size: 60125992
Audio Size: 0
Downloads Size: 0
System Size: 64000000000
Other Size: 126559304
Package Names: ["com.android.cts.priv.ctsshim","com.oculus.assetdelivery","com.oculus.avatareditor","de.blinkt.openvpn"]
App Sizes: [0,51712,23151616,66741760]
App Data Sizes: [0,25088,475136,16384]
Cache Sizes: [0,14336,16384,4096]"#;

        populate_sizes(&mut packages, dumpsys_output).unwrap();

        assert_eq!(packages[0].size.app, 23151616);
        assert_eq!(packages[0].size.data, 475136);
        assert_eq!(packages[0].size.cache, 16384);

        assert_eq!(packages[1].size.app, 66741760);
        assert_eq!(packages[1].size.data, 16384);
        assert_eq!(packages[1].size.cache, 4096);
    }

    #[test]
    fn test_update_packages_with_sizes_invalid() {
        let mut packages = vec![InstalledPackage {
            uid: 10029,
            system: true,
            package_name: "com.oculus.avatareditor".to_string(),
            version_code: 640490640,
            version_name: "69.0.0.556.352".to_string(),
            label: "AvatarEditor".to_string(),
            launchable: true,
            vr: true,
            size: AppSize::default(),
        }];

        let dumpsys_output = "Invalid output";
        assert!(populate_sizes(&mut packages, dumpsys_output).is_err());
    }
}
