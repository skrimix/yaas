/// Reads the numeric application version used by desktop packages.
pub fn app_version(pubspec: &str) -> Result<(&str, &str), &'static str> {
    let mut versions = pubspec.lines().filter_map(|line| line.strip_prefix("version:"));
    let value = versions.next().ok_or("Missing pubspec version")?.trim();
    if versions.next().is_some() {
        return Err("Duplicate pubspec version");
    }
    let (version, build) = value.split_once('+').ok_or("Expected version+build")?;
    let components: Vec<_> = version.split('.').collect();
    if components.len() != 3
        || components.iter().chain(std::iter::once(&build)).any(|part| {
            part.is_empty()
                || !part.bytes().all(|c| c.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
                || part.parse::<u16>().is_err()
        })
        || build == "0"
    {
        return Err("Expected numeric MAJOR.MINOR.PATCH+BUILD, each at most 65535, BUILD positive");
    }
    Ok((version, build))
}

pub fn validate_channel(channel: &str, run: &str, attempt: &str) -> Result<(), &'static str> {
    if !matches!(channel, "development" | "nightly" | "stable") {
        return Err("Unknown YAAS_RELEASE_CHANNEL");
    }
    if channel == "development" && run.is_empty() && attempt.is_empty() {
        return Ok(());
    }
    if [run, attempt].iter().any(|value| {
        value.is_empty()
            || value.starts_with('0')
            || !value.bytes().all(|c| c.is_ascii_digit())
            || value.parse::<u64>().is_err()
    }) {
        return Err("CI builds require positive YAAS_RUN_NUMBER and YAAS_RUN_ATTEMPT");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_native_package_version() {
        assert_eq!(app_version("name: yaas\nversion: 1.2.3+4\n"), Ok(("1.2.3", "4")));
        for input in ["1.2+4", "1.2.3-beta.1+4", "1.2.3", "01.2.3+4", "1.2.3+0", "1.2.3+65536"] {
            assert!(app_version(&format!("version: {input}")).is_err(), "{input}");
        }
        assert!(app_version("version: 1.2.3+4\nversion: 2.0.0+5").is_err());
    }

    #[test]
    fn distinguishes_local_and_ci_builds() {
        assert!(validate_channel("development", "", "").is_ok());
        assert!(validate_channel("nightly", "123", "2").is_ok());
        assert!(validate_channel("stable", "124", "1").is_ok());
        assert!(validate_channel("unknown", "1", "1").is_err());
        assert!(validate_channel("stable", "", "").is_err());
        assert!(validate_channel("development", "123", "").is_err());
        assert!(validate_channel("nightly", "0", "1").is_err());
    }
}
