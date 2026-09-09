#!/usr/bin/env python3
"""Build release metadata and prepare GitHub downloads using the gh CLI."""

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zipfile
from pathlib import Path

PACKAGES = (
    ("YAAS-windows-x64.zip", "windows", ["x86_64"]),
    ("YAAS-linux-x86_64.AppImage", "linux", ["x86_64"]),
    ("YAAS-macos.zip", "macos", ["aarch64", "x86_64"]),
)
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)\+([1-9]\d*)")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def app_version(root=Path(".")):
    lines = re.findall(
        r"^version:\s*(\S+)\s*$", (root / "pubspec.yaml").read_text(), re.MULTILINE
    )
    require(
        len(lines) == 1 and VERSION.fullmatch(lines[0]),
        "pubspec.yaml must contain one version: MAJOR.MINOR.PATCH+BUILD line",
    )
    version, build = lines[0].split("+")
    # Windows stores each component in a 16-bit version field.
    require(
        all(int(part) <= 65535 for part in [*version.split("."), build]),
        "Version components and build number must be at most 65535",
    )
    return version, int(build)


def release_notes(version, root=Path(".")):
    text = (root / "CHANGELOG.md").read_text()
    headings = list(re.finditer(r"^## (.+)$", text, re.MULTILINE))
    matches = [(i, h) for i, h in enumerate(headings) if h[1] == version]
    require(
        len(matches) == 1, f"Add exactly one '## {version}' section to CHANGELOG.md"
    )
    index, heading = matches[0]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
    notes = text[heading.end() : end].strip()
    require(
        notes and not notes.startswith("<!--"), f"Write release notes for {version}"
    )
    return notes


def build_identity(channel, sha, env=os.environ):
    require(channel in ("development", "nightly", "stable"), "Invalid release channel")
    require(re.fullmatch(r"[0-9a-f]{40}", sha), "Expected a full commit SHA")
    numbers = [env.get("YAAS_RUN_NUMBER", ""), env.get("YAAS_RUN_ATTEMPT", "")]
    require(
        all(re.fullmatch(r"[1-9]\d*", n) for n in numbers),
        "CI run number and attempt must be positive integers",
    )
    return {
        "channel": channel,
        "commit": sha,
        "run_number": int(numbers[0]),
        "run_attempt": int(numbers[1]),
    }


def write_identity(sha, path):
    version, build = app_version()
    identity = build_identity(os.environ["YAAS_RELEASE_CHANNEL"], sha)
    identity.update(version=version, build_number=build)
    path.write_text(json.dumps(identity, indent=2) + "\n")


def architectures(data):
    """Read ELF, PE, or Mach-O headers; return None for other files."""
    if data[:4] == b"\x7fELF":
        endian = "<" if data[5] == 1 else ">"
        machine = struct.unpack_from(endian + "H", data, 18)[0]
        return {62: {"x86_64"}, 183: {"aarch64"}}.get(machine, {"unsupported"})
    if data[:2] == b"MZ":
        offset = struct.unpack_from("<I", data, 60)[0]
        require(data[offset : offset + 4] == b"PE\0\0", "Invalid PE executable")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        return {0x8664: {"x86_64"}, 0xAA64: {"aarch64"}, 0x14C: {"x86"}}.get(
            machine, {"unsupported"}
        )
    magic = data[:4]
    thin = {b"\xcf\xfa\xed\xfe": "<", b"\xfe\xed\xfa\xcf": ">"}
    fat = {
        b"\xca\xfe\xba\xbe": (">", 20),
        b"\xbe\xba\xfe\xca": ("<", 20),
        b"\xca\xfe\xba\xbf": (">", 32),
        b"\xbf\xba\xfe\xca": ("<", 32),
    }
    cpu = {0x01000007: "x86_64", 0x0100000C: "aarch64"}
    if magic in thin:
        return {
            cpu.get(struct.unpack_from(thin[magic] + "I", data, 4)[0], "unsupported")
        }
    if magic in fat:
        endian, stride = fat[magic]
        count = struct.unpack_from(endian + "I", data, 4)[0]
        require(0 < count <= 16, "Invalid universal executable header")
        return {
            cpu.get(
                struct.unpack_from(endian + "I", data, 8 + i * stride)[0], "unsupported"
            )
            for i in range(count)
        }
    return None


def verify_bundle(platform, root):
    expected = {"aarch64", "x86_64"} if platform == "macos" else {"x86_64"}
    windows_helpers = {"adb.exe", "adbwinapi.dll", "adbwinusbapi.dll", "7za.exe"}

    def compatible(path, found):
        # Windows can run the upstream 32-bit tools as separate processes.
        # The app and libraries it loads still have to be 64-bit.
        if (
            platform == "windows"
            and path.relative_to(root).as_posix().lower() in windows_helpers
        ):
            return found in ({"x86"}, {"x86_64"})
        return found == expected

    required = {
        "windows": ["yaas.exe", "hub.dll", "adb.exe", "7za.exe"],
        "macos": ["Contents/MacOS/YAAS", "Contents/MacOS/adb", "Contents/MacOS/7zz"],
        "linux": [
            "yaas",
            "lib/libhub.so",
            "lib/libflutter_linux_gtk.so",
            "usr/bin/adb",
            "usr/bin/7zzs",
        ],
    }[platform]
    for name in required:
        path = root / name
        require(path.is_file(), f"Missing bundled file: {path}")
        with path.open("rb") as stream:
            require(
                compatible(path, architectures(stream.read(65536))),
                f"Wrong architecture: {path}",
            )
    # Include native libraries, frameworks, and bundled tools, not only the launcher.
    for path in root.rglob("*"):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as stream:
                found = architectures(stream.read(65536))
            require(
                found is None or compatible(path, found),
                f"Wrong architecture: {path}: {found}",
            )


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_metadata(directory, version, build, identity):
    expected = {name for name, _, _ in PACKAGES}
    require(
        {p.name for p in directory.iterdir()} == expected,
        "Expected exactly three release packages",
    )
    assets = []
    for name, platform, archs in PACKAGES:
        path = directory / name
        require(
            path.is_file() and path.stat().st_size > 0,
            f"Empty or missing package: {name}",
        )
        assets.append(
            {
                "name": name,
                "os": platform,
                "architectures": archs,
                "size": path.stat().st_size,
                "sha256": digest(path),
            }
        )
    tag = f"v{version}" if identity["channel"] == "stable" else identity["channel"]
    manifest = dict(
        schema_version=1,
        version=version,
        build_number=build,
        tag=tag,
        **identity,
        assets=assets,
    )
    (directory / "release.json").write_text(json.dumps(manifest, indent=2) + "\n")
    checksums = [
        f"{digest(directory / name)}  {name}\n"
        for name in sorted(expected | {"release.json"})
    ]
    (directory / "SHA256SUMS").write_text("".join(checksums))
    return manifest


def package(sha, artifacts=Path("artifacts"), output=Path("release")):
    version, build = app_version()
    identity = build_identity(os.environ["YAAS_RELEASE_CHANNEL"], sha)
    builds = [
        json.loads((artifacts / f"build-{platform}/build-identity.json").read_text())
        for _, platform, _ in PACKAGES
    ]
    require(
        all(build == builds[0] for build in builds),
        "Platform builds have different identities. Re-run all jobs to build a matching set.",
    )
    expected = {**identity, "version": version, "build_number": build}
    # Retrying assembly can reuse a complete set from an earlier attempt.
    expected["run_attempt"] = builds[0]["run_attempt"]
    require(
        builds[0] == expected,
        "Platform builds differ from the requested source/version/channel",
    )
    identity["run_attempt"] = builds[0]["run_attempt"]
    require(
        not output.exists() or not any(output.iterdir()),
        f"Output directory must be empty: {output}",
    )
    windows = artifacts / "build-windows"
    verify_bundle("windows", windows)
    require((windows / "launch_portable.bat").is_file(), "Missing portable launcher")
    linux = artifacts / "build-linux/yaas.AppImage"
    require(linux.is_file(), "Missing Linux AppImage")
    with linux.open("rb") as stream:
        require(
            architectures(stream.read(64)) == {"x86_64"}, "Wrong AppImage architecture"
        )
    macos = artifacts / "build-macos/YAAS-macos.zip"
    require(macos.is_file(), "Missing macOS ZIP")
    with zipfile.ZipFile(macos) as archive:
        require(archive.testzip() is None, "Corrupt macOS ZIP")
        for binary in ("YAAS", "adb", "7zz"):
            name = f"YAAS.app/Contents/MacOS/{binary}"
            require(name in archive.namelist(), f"Missing macOS binary: {binary}")
            with archive.open(name) as stream:
                require(
                    architectures(stream.read(65536)) == {"aarch64", "x86_64"},
                    f"Non-universal macOS binary: {binary}",
                )
        for name in archive.namelist():
            if name.endswith("/"):
                continue
            with archive.open(name) as stream:
                archs = architectures(stream.read(65536))
            require(
                archs is None or archs == {"aarch64", "x86_64"},
                f"Non-universal macOS file: {name}",
            )
    output.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output / PACKAGES[0][0], "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(windows.rglob("*")):
            if path.is_file() and path != windows / "build-identity.json":
                archive.write(path, path.relative_to(windows))
    shutil.copyfile(linux, output / PACKAGES[1][0])
    (output / PACKAGES[1][0]).chmod(0o755)
    shutil.copyfile(macos, output / PACKAGES[2][0])
    return write_metadata(output, version, build, identity)


def run(*args, **kwargs):
    return subprocess.run(
        args, check=True, capture_output=True, text=True, **kwargs
    ).stdout.strip()


def api(path, payload=None):
    args = ["gh", "api", path]
    if payload is None:
        return json.loads(run(*args))
    return json.loads(
        run(
            *args,
            "--method",
            "PATCH" if "/releases/" in path else "POST",
            "--input",
            "-",
            input=json.dumps(payload),
        )
    )


def repository():
    repo = os.environ["GITHUB_REPOSITORY"]
    require(re.fullmatch(r"[\w.-]+/[\w.-]+", repo), "Invalid repository")
    return repo


def publication_policy(channel, env=os.environ):
    require(
        env.get("GITHUB_REF") == "refs/heads/master",
        "Release preparation and publication require master",
    )
    event = env.get("GITHUB_EVENT_NAME")
    require(
        (channel == "stable" and event == "workflow_dispatch")
        or (channel == "nightly" and event in ("push", "workflow_dispatch")),
        "This event cannot publish releases",
    )


def find_release(repo, tag):
    pages = json.loads(
        run("gh", "api", "--paginate", "--slurp", f"repos/{repo}/releases?per_page=100")
    )
    matches = [
        release for page in pages for release in page if release["tag_name"] == tag
    ]
    require(
        len(matches) <= 1,
        f"More than one release uses {tag}; resolve the duplicate drafts first",
    )
    return matches[0] if matches else None


def remote_tag(tag):
    refs = run(
        "git", "ls-remote", "origin", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"
    )
    if not refs:
        return None
    # The peeled ref follows the tag ref for annotated tags.
    return refs.splitlines()[-1].split()[0]


def check_draft(repo, version, sha):
    existing = find_release(repo, f"v{version}")
    require(
        existing is None or existing["draft"],
        f"v{version} is already published; choose a new version",
    )
    tag_sha = remote_tag(f"v{version}")
    require(
        tag_sha is None or tag_sha == sha,
        f"v{version} already points to another commit; numbered tags are never moved",
    )
    return existing


def newer_nightly(existing, manifest):
    if existing is None:
        return True
    marker = re.search(r"<!-- yaas-build: (\d+)\.(\d+) -->", existing.get("body") or "")
    if marker:
        incoming = (manifest["run_number"], manifest["run_attempt"])
        published = tuple(map(int, marker.groups()))
        if incoming == published:
            return (
                existing["body"].startswith("Preparation incomplete.")
                and f"Nightly build from `{manifest['commit']}`." in existing["body"]
            )
        return incoming > published
    # When adopting the new format, don't replace a newer legacy nightly with older code.
    old_sha = remote_tag("nightly")
    if old_sha and old_sha != manifest["commit"]:
        result = subprocess.run(
            ["git", "merge-base", "--is-ancestor", old_sha, manifest["commit"]],
            check=False,
        )
        require(result.returncode in (0, 1), "Cannot compare nightly commits")
        return result.returncode == 0
    return True


def read_manifest(directory, channel):
    manifest = json.loads((directory / "release.json").read_text())
    version, build = app_version()
    require(
        manifest["schema_version"] == 1 and manifest["channel"] == channel,
        "Unexpected release metadata",
    )
    require(
        (manifest["version"], manifest["build_number"]) == (version, build),
        "Package version differs from source",
    )
    require(
        manifest["commit"] == run("git", "rev-parse", "HEAD"),
        "Packages were built from another commit",
    )
    expected_tag = f"v{version}" if channel == "stable" else "nightly"
    require(manifest["tag"] == expected_tag, "Unexpected release tag")
    require(
        [(a["name"], a["os"], a["architectures"]) for a in manifest["assets"]]
        == list(PACKAGES),
        "Unexpected package list",
    )
    expected_files = {name for name, _, _ in PACKAGES} | {"release.json", "SHA256SUMS"}
    require(
        {p.name for p in directory.iterdir()} == expected_files,
        "Unexpected release files",
    )
    for asset in manifest["assets"]:
        path = directory / asset["name"]
        require(
            path.stat().st_size == asset["size"] and digest(path) == asset["sha256"],
            f"Package checksum mismatch: {path.name}",
        )
    sums = "".join(
        f"{digest(directory / name)}  {name}\n"
        for name in sorted(expected_files - {"SHA256SUMS"})
    )
    require(
        (directory / "SHA256SUMS").read_text() == sums,
        "SHA256SUMS differs from the release files",
    )
    return manifest


def summary(message):
    print(message)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(path, "a") as stream:
            stream.write(message + "\n")


def publish(channel, directory=Path("release")):
    publication_policy(channel)
    repo = repository()
    manifest = read_manifest(directory, channel)
    tag, sha = manifest["tag"], manifest["commit"]
    if channel == "stable":
        notes = release_notes(manifest["version"])
        existing = check_draft(repo, manifest["version"], sha)
        body = notes + f"\n\nBuilt from `{sha}`."
    else:
        existing = find_release(repo, "nightly")
        if not newer_nightly(existing, manifest):
            summary("Skipped: this nightly is not newer than the published build.")
            return
        marker = (
            f"<!-- yaas-build: {manifest['run_number']}.{manifest['run_attempt']} -->"
        )
        body = f"Nightly build from `{sha}`.\n\nBuild {manifest['run_number']}.{manifest['run_attempt']}\n\n{marker}"
    payload = {
        "tag_name": tag,
        "target_commitish": sha,
        "name": f"YAAS {manifest['version']}" if channel == "stable" else "CI Nightly",
        "draft": channel == "stable",
        "prerelease": channel == "nightly",
        "make_latest": "false",
        "body": f"Preparation incomplete. Wait for the workflow to finish before using this release.\n\n{body}",
    }
    endpoint = f"repos/{repo}/releases"
    if existing:
        endpoint += f"/{existing['id']}"
    result = api(endpoint, payload)
    if channel == "nightly":
        # Only the nightly tag may move. Numbered tags are left to GitHub on publication.
        run("git", "push", "--force", "origin", f"{sha}:refs/tags/nightly")
    # Upload metadata last. An interrupted nightly update is detected by its checksums.
    names = [name for name, _, _ in PACKAGES] + ["SHA256SUMS", "release.json"]
    for name in names:
        run(
            "gh",
            "release",
            "upload",
            tag,
            str(directory / name),
            "--clobber",
            "--repo",
            repo,
        )
    result = api(f"repos/{repo}/releases/{result['id']}", {"body": body})
    summary(
        f"{'Draft ready' if channel == 'stable' else 'Nightly updated'}: {result['html_url']}"
    )
    if channel == "stable":
        summary(
            "Test all three downloads: startup, About version, device connection, and a basic operation. "
            "Then publish the draft on GitHub and mark it as the latest release. See releases.md."
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("--require-notes", action="store_true")
    check = commands.add_parser("check-draft")
    check.add_argument("--sha", required=True)
    pack = commands.add_parser("package")
    pack.add_argument("--sha", required=True)
    identity = commands.add_parser("identity")
    identity.add_argument("--sha", required=True)
    identity.add_argument("--output", required=True, type=Path)
    verify = commands.add_parser("verify-bundle")
    verify.add_argument("platform", choices=["windows", "linux", "macos"])
    verify.add_argument("directory", type=Path)
    pub = commands.add_parser("publish")
    pub.add_argument("channel", choices=["stable", "nightly"])
    args = parser.parse_args()
    if args.command == "validate":
        publication_policy("stable")
        version, build = app_version()
        if args.require_notes:
            release_notes(version)
        print(f"Preparing {version}+{build} from {os.environ['GITHUB_SHA']}")
    elif args.command == "check-draft":
        publication_policy("stable")
        version, _ = app_version()
        check_draft(repository(), version, args.sha)
    elif args.command == "package":
        package(args.sha)
    elif args.command == "identity":
        write_identity(args.sha, args.output)
    elif args.command == "verify-bundle":
        verify_bundle(args.platform, args.directory)
    elif args.command == "publish":
        publish(args.channel)


if __name__ == "__main__":
    try:
        main()
    except (
        ValueError,
        KeyError,
        OSError,
        struct.error,
        zipfile.BadZipFile,
        subprocess.CalledProcessError,
    ) as error:
        print(f"Release preparation failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        sys.exit(1)
