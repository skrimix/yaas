use std::path::Path;

mod build_support;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=assets");
    write_app_identity();
    // TODO: Is there a better way to do this? Feels hacky.
    // Workaround: trigger a rebuild when the git repo changes to update
    // our built file.
    track_git_changes().expect("Failed to track Git build inputs");
    built::write_built_file().expect("Failed to acquire build-time information")
}

fn write_app_identity() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let pubspec_path = Path::new(&manifest_dir).join("../../pubspec.yaml");
    track_path(&pubspec_path);
    println!("cargo:rerun-if-changed=build_support.rs");
    let pubspec = std::fs::read_to_string(pubspec_path).expect("Failed to read pubspec.yaml");
    let (version, build) = build_support::app_version(&pubspec).expect("Invalid app version");
    let values = ["YAAS_RELEASE_CHANNEL", "YAAS_RUN_NUMBER", "YAAS_RUN_ATTEMPT"].map(|key| {
        println!("cargo:rerun-if-env-changed={key}");
        std::env::var(key).unwrap_or_default()
    });
    let [channel, run, attempt] = values;
    let channel = if channel.is_empty() { "development" } else { &channel };
    build_support::validate_channel(channel, &run, &attempt).expect("Invalid build identity");
    for (key, value) in [
        ("YAAS_APP_VERSION", version),
        ("YAAS_BUILD_NUMBER", build),
        ("YAAS_RELEASE_CHANNEL", channel),
        ("YAAS_RUN_NUMBER", &run),
        ("YAAS_RUN_ATTEMPT", &attempt),
    ] {
        println!("cargo:rustc-env={key}={value}");
    }
}

fn track_git_changes() -> Result<(), git2::Error> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let repo = match git2::Repository::discover(manifest_dir) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    // Worktrees have their own HEAD and index but share branch refs.
    for path in [
        repo.path().join("HEAD"),
        repo.path().join("index"),
        repo.path().join("commondir"),
        repo.commondir().join("packed-refs"),
        repo.commondir().join("config"),
    ] {
        // Missing optional Git files would make Cargo rebuild on every invocation.
        if path.exists() {
            track_path(&path);
        }
    }

    // Watching all refs would also rebuild after background fetches or stash updates.
    if let Some(branch) = repo.find_reference("HEAD")?.symbolic_target()? {
        track_existing_path(&repo.commondir().join(branch));
    }

    if let Some(workdir) = repo.workdir() {
        let git_file = workdir.join(".git");
        if git_file.is_file() {
            track_path(&git_file);
        }

        // The dirty flag includes tracked files outside the Rust crate, including Dart sources.
        for entry in repo.index()?.iter() {
            let name = std::str::from_utf8(&entry.path)
                .map_err(|_| git2::Error::from_str("Tracked file path is not UTF-8"))?;
            let path = workdir.join(name);
            track_existing_path(&path);
        }
    }
    Ok(())
}

fn track_existing_path(mut path: &Path) {
    // Watch the parent when a deleted file or packed branch ref is recreated.
    while !path.exists() {
        path = path.parent().expect("Build input must have an existing ancestor");
    }
    track_path(path);
}

fn track_path(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}
