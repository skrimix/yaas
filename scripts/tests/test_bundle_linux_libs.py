import importlib.util
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "bundle_linux_libs", Path(__file__).parents[1] / "bundle_linux_libs.py"
)
bundler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundler)


def have_elf_toolchain():
    return all(shutil.which(cmd) for cmd in ("cc", "patchelf", "readelf", "ldd"))


@unittest.skipUnless(have_elf_toolchain(), "Requires cc, patchelf, readelf, and ldd")
class BundleLinuxLibsTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.sys_libs = self.root / "syslibs"
        self.sys_libs.mkdir()
        self.appdir = self.root / "AppDir"
        (self.appdir / "lib").mkdir(parents=True)
        (self.appdir / "usr/lib").mkdir(parents=True)

    def build_c(self, output, code, extra_args=()):
        src = self.root / f"{output.name}.c"
        src.write_text(code)
        subprocess.run(
            ["cc", "-O1", str(src), *extra_args, "-o", str(output)],
            check=True,
        )
        return output

    def make_shared(self, path, soname, code="int sym(void) { return 42; }\n", link=()):
        return self.build_c(
            path,
            code,
            ["-shared", "-fPIC", f"-Wl,-soname,{soname}", *link],
        )

    def populate_minimal_appdir(self):
        # Host-ABI stub that fastforge might erroneously copy into usr/lib
        self.make_shared(self.appdir / "usr/lib/libEGL.so.1", "libEGL.so.1")

        # Transitive non-host dependency (simulates libbz2.so.1.0 / libjpeg.so.8)
        leaf = self.make_shared(
            self.sys_libs / "libcustomleaf.so.1",
            "libcustomleaf.so.1",
            "int leaf_fn(void) { return 7; }\n",
        )

        # Bundled libmpv linking leaf via build-host rpath (simulates libmpv -> libarchive -> libbz2)
        mpv = self.make_shared(
            self.appdir / "usr/lib/libmpv.so.2",
            "libmpv.so.2",
            "int leaf_fn(void); int mpv_fn(void) { return leaf_fn(); }\n",
            [str(leaf), f"-Wl,-rpath,{self.sys_libs}"],
        )

        # Dart AOT snapshot stub with zero DT_NEEDED (must not be touched by patchelf)
        self.build_c(
            self.appdir / "lib/libapp.so",
            "void _kDartVmSnapshotData(void) {}\n",
            ["-shared", "-fPIC", "-nostdlib"],
        )

        # Plugin in lib/ with a leaked build-machine RPATH
        plugin = self.make_shared(
            self.appdir / "lib/libvideo_plugin.so",
            "libvideo_plugin.so",
            "int mpv_fn(void); int plug_fn(void) { return mpv_fn(); }\n",
            [str(mpv), "-Wl,-rpath,/workspace/build/ephemeral"],
        )

        # Main executable
        self.build_c(
            self.appdir / "yaas",
            "int plug_fn(void); int main(void) { return plug_fn() == 7 ? 0 : 1; }\n",
            [
                str(plugin),
                f"-Wl,-rpath-link,{self.appdir / 'usr/lib'}:{self.sys_libs}",
                "-Wl,-rpath,$ORIGIN/lib",
            ],
        )

        (self.appdir / "AppRun").write_text(
            "#!/bin/bash\nexport LD_LIBRARY_PATH=usr/lib\nexec ./yaas\n"
        )
        (self.appdir / "yaas.desktop").write_text(
            "[Desktop Entry]\nName=YAAS\nExec=LD_LIBRARY_PATH=usr/lib yaas %u\n"
        )

    def test_bundles_transitive_libs_prunes_host_abi_and_sets_relocatable_rpaths(self):
        self.populate_minimal_appdir()
        app_snapshot_before = (self.appdir / "lib/libapp.so").read_bytes()

        bundler.bundle_linux_libs(self.appdir)

        # Host ABI library removed; transitive non-host library bundled; libmpv.so symlinked
        self.assertFalse((self.appdir / "usr/lib/libEGL.so.1").exists())
        self.assertTrue((self.appdir / "usr/lib/libcustomleaf.so.1").is_file())
        self.assertEqual(
            (self.appdir / "usr/lib/libmpv.so").readlink(), Path("libmpv.so.2")
        )

        # Dart snapshot ELF was not modified
        self.assertEqual(
            (self.appdir / "lib/libapp.so").read_bytes(), app_snapshot_before
        )

        # Remove build-time syslibs directory and verify the relocated AppDir still executes!
        shutil.rmtree(self.sys_libs)
        env = {"PATH": "/usr/bin:/bin", "LD_LIBRARY_PATH": "/custom/preserved/path"}
        probe = self.appdir / "usr/bin/env_probe"
        probe.parent.mkdir(parents=True, exist_ok=True)
        result = subprocess.run([str(self.appdir / "AppRun")], env=env, check=False)
        self.assertEqual(result.returncode, 0)

        # AppRun and desktop entry do not clobber LD_LIBRARY_PATH
        self.assertNotIn("export LD_LIBRARY_PATH=", (self.appdir / "AppRun").read_text())
        self.assertEqual(
            (self.appdir / "yaas.desktop").read_text(),
            "[Desktop Entry]\nName=YAAS\nExec=yaas %u\n",
        )

    def test_normalizes_slash_prefixed_dt_needed(self):
        self.populate_minimal_appdir()
        slash_lib = self.sys_libs / "libslash.so"
        self.make_shared(
            slash_lib,
            str(slash_lib),
            "int slash_fn(void) { return 3; }\n",
        )
        # Inject slash-prefixed DT_NEEDED like Arch's /usr/lib/libmujs.so
        bundler.patchelf(
            ["--add-needed", str(slash_lib)], self.appdir / "usr/lib/libmpv.so.2"
        )

        bundler.bundle_linux_libs(self.appdir)

        self.assertTrue((self.appdir / "usr/lib/libslash.so").is_file())
        needed, _ = bundler.read_dynamic(self.appdir / "usr/lib/libmpv.so.2")
        self.assertIn("libslash.so", needed)
        self.assertTrue(all("/" not in entry for entry in needed))

    def test_verify_rejects_leaked_rpath_or_missing_dep(self):
        self.populate_minimal_appdir()
        bundler.bundle_linux_libs(self.appdir)

        bundler.patchelf(
            ["--set-rpath", "$ORIGIN:/tmp/leaked"], self.appdir / "lib/libvideo_plugin.so"
        )
        with self.assertRaisesRegex(ValueError, "non-relocatable RUNPATH"):
            bundler.verify_appdir(self.appdir)

        bundler.patchelf(
            ["--set-rpath", "$ORIGIN:$ORIGIN/../usr/lib"],
            self.appdir / "lib/libvideo_plugin.so",
        )
        (self.appdir / "usr/lib/libcustomleaf.so.1").unlink()
        with self.assertRaisesRegex(ValueError, "unbundled non-host library"):
            bundler.verify_appdir(self.appdir)


if __name__ == "__main__":
    unittest.main()
