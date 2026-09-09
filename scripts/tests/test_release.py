import importlib.util
import json
import os
import struct
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "release", Path(__file__).parents[1] / "release.py"
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SHA = "a" * 40
ENV = {
    "YAAS_RELEASE_CHANNEL": "stable",
    "YAAS_RUN_NUMBER": "123",
    "YAAS_RUN_ATTEMPT": "2",
}


def pe(machine=0x8664):
    data = bytearray(128)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, machine)
    return bytes(data)


def elf(machine=62):
    data = bytearray(64)
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", data, 18, machine)
    return bytes(data)


def universal():
    data = bytearray(48)
    data[:4] = b"\xca\xfe\xba\xbe"
    struct.pack_into(">I", data, 4, 2)
    struct.pack_into(">I", data, 8, 0x01000007)
    struct.pack_into(">I", data, 28, 0x0100000C)
    return bytes(data)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.pubspec = self.root / "pubspec.yaml"
        self.pubspec.write_text("name: yaas\nversion: 1.0.0+1\n")
        self.changelog = self.root / "CHANGELOG.md"
        self.changelog.write_text(
            "# Changelog\n\n## Unreleased\n\n## 1.0.0\n\n- First release.\n\n## 0.9.0\n\n- Older.\n"
        )

    def test_version_validation(self):
        self.assertEqual(release.app_version(self.root), ("1.0.0", 1))
        for version in [
            "1.0",
            "v1.0.0+1",
            "1.0.0-beta.1+1",
            "01.0.0+1",
            "1.0.0+0",
            "1.0.0+65536",
        ]:
            with self.subTest(version=version):
                self.pubspec.write_text(f"version: {version}\n")
                with self.assertRaises(ValueError):
                    release.app_version(self.root)
        self.pubspec.write_text("version: 1.0.0+1\nversion: 2.0.0+2\n")
        with self.assertRaises(ValueError):
            release.app_version(self.root)

    def test_notes_are_specific_and_nonempty(self):
        self.assertEqual(release.release_notes("1.0.0", self.root), "- First release.")
        for notes in [
            "## Unreleased\n- Soon",
            "## 1.0.0\n\n## 0.9.0\n- Old",
            "## 1.0.0\n- One\n## 1.0.0\n- Two",
        ]:
            self.changelog.write_text(notes)
            with self.assertRaises(ValueError):
                release.release_notes("1.0.0", self.root)

    def test_build_identity(self):
        identity = release.build_identity("nightly", SHA, ENV)
        self.assertEqual(identity["run_attempt"], 2)
        self.assertEqual(identity["commit"], SHA)
        for channel, sha, env in [
            ("bad", SHA, ENV),
            ("stable", "short", ENV),
            ("nightly", SHA, {}),
            ("stable", SHA, {**ENV, "YAAS_RUN_NUMBER": "0"}),
        ]:
            with self.assertRaises(ValueError):
                release.build_identity(channel, sha, env)

    def test_publication_policy(self):
        for channel in ["stable", "nightly"]:
            release.publication_policy(
                channel,
                {
                    "GITHUB_REF": "refs/heads/master",
                    "GITHUB_EVENT_NAME": "workflow_dispatch",
                },
            )
            for ref, event in [
                ("refs/heads/feature", "workflow_dispatch"),
                ("refs/heads/master", "pull_request"),
                ("refs/tags/v1.0.0", "push"),
            ]:
                with self.assertRaises(ValueError):
                    release.publication_policy(
                        channel, {"GITHUB_REF": ref, "GITHUB_EVENT_NAME": event}
                    )
        with self.assertRaises(ValueError):
            release.publication_policy(
                "stable",
                {"GITHUB_REF": "refs/heads/master", "GITHUB_EVENT_NAME": "push"},
            )

    def test_architectures(self):
        self.assertEqual(release.architectures(pe()), {"x86_64"})
        self.assertEqual(release.architectures(elf()), {"x86_64"})
        self.assertEqual(release.architectures(universal()), {"x86_64", "aarch64"})
        self.assertEqual(release.architectures(pe(0xAA64)), {"aarch64"})
        self.assertIsNone(release.architectures(b"ordinary data"))

    def test_linux_appimage_layout_and_native_libraries(self):
        for name in [
            "yaas",
            "lib/libhub.so",
            "lib/libflutter_linux_gtk.so",
            "usr/bin/adb",
            "usr/bin/7zzs",
            "usr/bin/yaas-updater",
        ]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(elf())
        release.verify_bundle("linux", self.root)
        (self.root / "lib/libhub.so").write_bytes(elf(183))
        with self.assertRaisesRegex(ValueError, "architecture"):
            release.verify_bundle("linux", self.root)

    def make_artifacts(self):
        artifacts = self.root / "artifacts"
        windows = artifacts / "build-windows"
        windows.mkdir(parents=True)
        for name in ["yaas.exe", "hub.dll", "adb.exe", "7za.exe", "yaas-updater.exe"]:
            (windows / name).write_bytes(pe())
        (windows / "launch_portable.bat").write_text("yaas.exe --portable")
        (artifacts / "build-linux").mkdir()
        (artifacts / "build-linux/yaas.AppImage").write_bytes(elf())
        (artifacts / "build-macos").mkdir()
        with zipfile.ZipFile(artifacts / "build-macos/YAAS-macos.zip", "w") as archive:
            for name in ["YAAS", "adb", "7zz", "yaas-updater"]:
                archive.writestr(f"YAAS.app/Contents/MacOS/{name}", universal())
        identity = {
            **release.build_identity("stable", SHA, ENV),
            "version": "1.0.0",
            "build_number": 1,
        }
        with zipfile.ZipFile(artifacts / "build-macos/YAAS-macos.zip", "a") as archive:
            archive.writestr("YAAS.app/Contents/Resources/yaas-build.json", json.dumps(identity))
        for platform in ["windows", "linux", "macos"]:
            (artifacts / f"build-{platform}/build-identity.json").write_text(
                json.dumps(identity)
            )
        return artifacts

    def package(self, artifacts):
        with (
            patch.object(release, "app_version", return_value=("1.0.0", 1)),
            patch.dict(os.environ, ENV),
        ):
            return release.package(SHA, artifacts, self.root / "release")

    def test_complete_packages_and_checksums(self):
        manifest = self.package(self.make_artifacts())
        self.assertEqual(manifest["tag"], "v1.0.0")
        self.assertEqual(manifest["run_attempt"], 2)
        self.assertEqual(len(manifest["assets"]), 3)
        output = self.root / "release"
        with zipfile.ZipFile(output / "YAAS-windows-x64.zip") as archive:
            inventory = json.loads(archive.read("yaas-package.json"))
            self.assertEqual(set(inventory["files"]), set(archive.namelist()))
            self.assertNotIn("build-identity.json", inventory["files"])
            self.assertIn("yaas-updater.exe", inventory["files"])
            self.assertEqual(inventory["identity"]["commit"], SHA)
            self.assertEqual(inventory["identity"]["version"], manifest["version"])
        with (
            patch.object(release, "app_version", return_value=("1.0.0", 1)),
            patch.object(release, "run", return_value=SHA),
        ):
            self.assertEqual(release.read_manifest(output, "stable"), manifest)
            (output / "YAAS-windows-x64.zip").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "checksum"):
                release.read_manifest(output, "stable")

    def test_missing_platform_blocks_packaging(self):
        artifacts = self.make_artifacts()
        (artifacts / "build-macos/YAAS-macos.zip").unlink()
        with self.assertRaisesRegex(ValueError, "Missing macOS"):
            self.package(artifacts)
        self.assertFalse((self.root / "release").exists())

    def test_mixed_build_attempts_are_rejected(self):
        artifacts = self.make_artifacts()
        path = artifacts / "build-windows/build-identity.json"
        identity = json.loads(path.read_text())
        identity["run_attempt"] = 1
        path.write_text(json.dumps(identity))
        with self.assertRaisesRegex(ValueError, "Re-run all jobs"):
            self.package(artifacts)

    def test_assembly_retry_preserves_binary_identity(self):
        artifacts = self.make_artifacts()
        with (
            patch.object(release, "app_version", return_value=("1.0.0", 1)),
            patch.dict(os.environ, {**ENV, "YAAS_RUN_ATTEMPT": "3"}),
        ):
            manifest = release.package(SHA, artifacts, self.root / "release")
        self.assertEqual(manifest["run_attempt"], 2)

    def test_wrong_bundled_architecture_blocks_packaging(self):
        artifacts = self.make_artifacts()
        (artifacts / "build-windows/hub.dll").write_bytes(pe(0x14C))
        with self.assertRaisesRegex(ValueError, "architecture"):
            self.package(artifacts)

    def test_windows_allows_upstream_32_bit_helper_processes(self):
        artifacts = self.make_artifacts()
        for name in ["adb.exe", "7za.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"]:
            (artifacts / "build-windows" / name).write_bytes(pe(0x14C))
        self.package(artifacts)

    def test_missing_tool_blocks_packaging(self):
        artifacts = self.make_artifacts()
        (artifacts / "build-windows/adb.exe").unlink()
        with self.assertRaisesRegex(ValueError, "Missing bundled"):
            self.package(artifacts)

    @patch.object(release, "remote_tag", return_value=None)
    @patch.object(release, "find_release")
    def test_published_versions_and_conflicting_tags_are_rejected(self, find, tag):
        find.return_value = {"draft": False}
        with self.assertRaisesRegex(ValueError, "already published"):
            release.check_draft("owner/repo", "1.0.0", SHA)
        find.return_value = {"draft": True}
        self.assertEqual(
            release.check_draft("owner/repo", "1.0.0", SHA), {"draft": True}
        )
        tag.return_value = "b" * 40
        with self.assertRaisesRegex(ValueError, "never moved"):
            release.check_draft("owner/repo", "1.0.0", SHA)
        tag.return_value = SHA
        release.check_draft("owner/repo", "1.0.0", SHA)

    def test_old_nightly_and_duplicate_attempt_do_not_publish(self):
        existing = {"body": "<!-- yaas-build: 123.2 -->"}
        for number, attempt, allowed in [
            (122, 9, False),
            (123, 1, False),
            (123, 2, False),
            (123, 3, True),
            (124, 1, True),
        ]:
            self.assertEqual(
                release.newer_nightly(
                    existing, {"run_number": number, "run_attempt": attempt}
                ),
                allowed,
            )

    def test_incomplete_nightly_upload_can_resume_with_original_packages(self):
        existing = {
            "body": f"Preparation incomplete.\nNightly build from `{SHA}`.\n<!-- yaas-build: 123.2 -->",
            "target_commitish": "master",
        }
        manifest = {"run_number": 123, "run_attempt": 2, "commit": SHA}
        self.assertTrue(release.newer_nightly(existing, manifest))
        self.assertFalse(
            release.newer_nightly(existing, {**manifest, "commit": "b" * 40})
        )

    @patch.object(release, "remote_tag", return_value="b" * 40)
    @patch.object(
        release.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)
    )
    def test_legacy_nightly_rejects_older_commit(self, run, tag):
        self.assertFalse(release.newer_nightly({"body": "old format"}, {"commit": SHA}))

    def test_refresh_draft_stays_unpublished_and_uploads_metadata_last(self):
        manifest = self.package(self.make_artifacts())
        existing = {"id": 42, "draft": True}
        response = {
            "id": 42,
            "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
        }
        with (
            patch.object(release, "publication_policy"),
            patch.object(release, "repository", return_value="owner/repo"),
            patch.object(release, "read_manifest", return_value=manifest),
            patch.object(release, "release_notes", return_value="- Notes"),
            patch.object(release, "check_draft", return_value=existing),
            patch.object(release, "api", return_value=response) as api,
            patch.object(release, "run") as run,
            patch.object(release, "summary"),
        ):
            release.publish("stable", self.root / "release")
        initial_payload = api.call_args_list[0].args[1]
        self.assertTrue(initial_payload["draft"])
        self.assertEqual(initial_payload["target_commitish"], SHA)
        self.assertIn("incomplete", initial_payload["body"])
        self.assertEqual(
            api.call_args_list[-1].args[1], {"body": f"- Notes\n\nBuilt from `{SHA}`."}
        )
        self.assertTrue(run.call_args_list[-1].args[4].endswith("release.json"))
        self.assertFalse(any(call.args[0] == "git" for call in run.call_args_list))

    def test_upload_failure_leaves_draft_marked_incomplete(self):
        manifest = self.package(self.make_artifacts())
        with (
            patch.object(release, "publication_policy"),
            patch.object(release, "repository", return_value="owner/repo"),
            patch.object(release, "read_manifest", return_value=manifest),
            patch.object(release, "release_notes", return_value="- Notes"),
            patch.object(release, "check_draft", return_value=None),
            patch.object(release, "api", return_value={"id": 42}) as api,
            patch.object(
                release, "run", side_effect=subprocess.CalledProcessError(1, "gh")
            ),
            self.assertRaises(subprocess.CalledProcessError),
        ):
            release.publish("stable", self.root / "release")
        self.assertEqual(api.call_count, 1)
        self.assertTrue(api.call_args.args[1]["draft"])


if __name__ == "__main__":
    unittest.main()
