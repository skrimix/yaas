import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

CI_SCRIPT = Path(__file__).parents[1] / "ci.sh"


class CiScriptTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)

    def test_github_env_and_shell_env_produce_matching_values(self):
        gh_env_file = self.root / "github.env"
        env = {
            **os.environ,
            "GITHUB_ENV": str(gh_env_file),
            "FLUTTER_VERSION": "3.99.1",
            "RINF_CLI_VERSION": "9.1.0",
        }
        subprocess.run([str(CI_SCRIPT), "github-env"], env=env, check=True)

        github_parsed = dict(
            line.split("=", 1)
            for line in gh_env_file.read_text().splitlines()
            if line
        )
        # GITHUB_ENV treats backslashes literally; space-separated package lists must stay unescaped.
        self.assertNotIn("\\", github_parsed["LINUX_APT_PACKAGES"])
        self.assertIn("patchelf", github_parsed["LINUX_APT_PACKAGES"].split())

        shell_eval_out = subprocess.check_output(
            [
                "bash",
                "-euo",
                "pipefail",
                "-c",
                f'eval "$("{CI_SCRIPT}" shell-env)"; '
                'printf "%s\\n%s\\n%s\\n%s\\n" '
                '"$FLUTTER_VERSION" "$RINF_CLI_VERSION" "$LINUX_APT_CACHE_VERSION" "$LINUX_APT_PACKAGES"',
            ],
            env={
                **os.environ,
                "FLUTTER_VERSION": "3.99.1",
                "RINF_CLI_VERSION": "9.1.0",
            },
            text=True,
        ).splitlines()

        self.assertEqual(
            shell_eval_out,
            [
                github_parsed["FLUTTER_VERSION"],
                github_parsed["RINF_CLI_VERSION"],
                github_parsed["LINUX_APT_CACHE_VERSION"],
                github_parsed["LINUX_APT_PACKAGES"],
            ],
        )

    def test_install_linux_tools_isolates_fastforge_pub_cache_without_leaking_to_children(self):
        fake_tools = self.root / "fake-tools"
        fake_tools.mkdir()
        bin_dir = self.root / "bin"
        ff_cache = self.root / "fastforge-cache"
        caller_cache = self.root / "caller-pub-cache"
        github_path = self.root / "github_path"

        fake_curl = fake_tools / "curl"
        fake_curl.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            'out=""\n'
            'while (($# > 0)); do\n'
            '  if [[ "$1" == "-o" ]]; then out="$2"; shift 2; else shift; fi\n'
            "done\n"
            'printf "#!/usr/bin/env bash\\necho fake-appimagetool\\n" > "$out"\n'
        )
        fake_curl.chmod(fake_curl.stat().st_mode | stat.S_IXUSR)

        fake_dart = fake_tools / "dart"
        fake_dart.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            'if [[ "${1:-}" == "pub" && "${2:-}" == "global" && "${3:-}" == "activate" && "${4:-}" == "fastforge" ]]; then\n'
            '  snap_dir="${PUB_CACHE:?}/global_packages/fastforge/bin"\n'
            '  mkdir -p "$snap_dir"\n'
            '  printf "snapshot-v1" > "$snap_dir/main.dart-3.99.0.snapshot"\n'
            "  exit 0\n"
            "fi\n"
            'printf "SNAP=%s\\nPUB_CACHE=%s\\nARGS=%s\\n" "$1" "${PUB_CACHE:-unset}" "${*:2}"\n'
        )
        fake_dart.chmod(fake_dart.stat().st_mode | stat.S_IXUSR)

        env = {
            **os.environ,
            "PATH": f"{fake_tools}:{os.environ['PATH']}",
            "PUB_CACHE": str(caller_cache),
            "GITHUB_PATH": str(github_path),
        }
        subprocess.run(
            [str(CI_SCRIPT), "install-linux-tools", str(bin_dir), str(ff_cache)],
            env=env,
            check=True,
        )

        self.assertEqual(github_path.read_text().strip(), str(bin_dir))
        self.assertEqual(
            subprocess.check_output([str(bin_dir / "appimagetool")], text=True).strip(),
            "fake-appimagetool",
        )

        ff_out = subprocess.check_output(
            [str(bin_dir / "fastforge"), "package", "--platform", "linux"],
            env=env,
            text=True,
        ).splitlines()
        self.assertEqual(
            ff_out,
            [
                f"SNAP={ff_cache}/global_packages/fastforge/bin/main.dart-3.99.0.snapshot",
                f"PUB_CACHE={caller_cache}",
                "ARGS=package --platform linux",
            ],
        )


if __name__ == "__main__":
    unittest.main()
