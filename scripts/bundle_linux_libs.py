#!/usr/bin/env python3
"""Normalize bundled shared libraries, RPATHs, and AppRun inside a Linux AppDir."""

import argparse
import os
import re
import shutil
import struct
import subprocess
from collections import deque
from pathlib import Path

# Libraries that belong to the host OS, C/C++ runtime, GPU driver, display server,
# or GTK3 desktop stack. Bundling older build-host copies of these breaks newer
# Mesa/LLVM GPU drivers, ALSA PipeWire plugins, or glibc private symbol ABI.
HOST_ABI_PREFIXES = (
    # Dynamic linker & glibc
    "ld-linux.so.",
    "ld-linux-x86-64.so.",
    "ld-linux-aarch64.so.",
    "libc.so.",
    "libmvec.so.",
    "libm.so.",
    "libdl.so.",
    "libpthread.so.",
    "librt.so.",
    "libresolv.so.",
    "libutil.so.",
    "libanl.so.",
    "libBrokenLocale.so.",
    "libnsl.so.",
    "libthread_db.so.",
    # GCC C++ & unwind runtime (must match host Mesa/LLVM DRI drivers)
    "libstdc++.so.",
    "libgcc_s.so.",
    # Host OpenGL / EGL / GLES / GBM / DRM stack (must match host GPU driver)
    "libEGL.so.",
    "libGL.so.",
    "libGLX.so.",
    "libOpenGL.so.",
    "libGLdispatch.so.",
    "libGLESv1_CM.so.",
    "libGLESv2.so.",
    "libgbm.so.",
    "libdrm.so.",
    "libdrm_",
    # Core host audio / IPC / device / compression / XML-parser primitives
    "libasound.so.",
    "libdbus-1.so.",
    "libsystemd.so.",
    "libudev.so.",
    "libz.so.",
    "libexpat.so.",
    # Wayland & core X11 client libraries (shared with host GTK3 & compositor)
    "libwayland-client.so.",
    "libwayland-cursor.so.",
    "libwayland-egl.so.",
    "libwayland-server.so.",
    "libX11.so.",
    "libX11-xcb.so.",
    "libxcb.so.",
    "libxcb-dri2.so.",
    "libxcb-dri3.so.",
    "libxcb-glx.so.",
    "libxcb-present.so.",
    "libxcb-randr.so.",
    "libxcb-render.so.",
    "libxcb-shm.so.",
    "libxcb-sync.so.",
    "libxcb-xfixes.so.",
    "libXext.so.",
    "libXfixes.so.",
    "libXi.so.",
    "libXrender.so.",
    "libXcursor.so.",
    "libXdamage.so.",
    "libXcomposite.so.",
    "libXrandr.so.",
    "libXinerama.so.",
    "libXau.so.",
    "libXdmcp.so.",
    "libxkbcommon.so.",
    "libxshmfence.so.",
    # Host GTK3 / GLib / Cairo / Pango / Fontconfig / Freetype / Harfbuzz / ATK stack
    "libgtk-3.so.",
    "libgdk-3.so.",
    "libgdk_pixbuf-2.0.so.",
    "libgio-2.0.so.",
    "libgobject-2.0.so.",
    "libglib-2.0.so.",
    "libgmodule-2.0.so.",
    "libatk-1.0.so.",
    "libatk-bridge-2.0.so.",
    "libatspi.so.",
    "libcairo.so.",
    "libcairo-gobject.so.",
    "libpango-1.0.so.",
    "libpangocairo-1.0.so.",
    "libpangoft2-1.0.so.",
    "libharfbuzz.so.",
    "libfontconfig.so.",
    "libfreetype.so.",
    "libfribidi.so.",
    "libpixman-1.so.",
)

NEEDED_RE = re.compile(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]")
RPATH_RE = re.compile(r"\((?:RUNPATH|RPATH)\)\s+Library r(?:un)?path: \[([^\]]+)\]")
LDD_RESOLVED_RE = re.compile(r"^\s*(\S+)\s+=>\s+(\S+)\s+\(0x[0-9a-fA-F]+\)\s*$")
LDCONFIG_RE = re.compile(r"^\s*(\S+)\s+\([^)]*\)\s+=>\s+(\S+)\s*$")

APPRUN_CONTENT = """#!/usr/bin/env bash
set -e

APPDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$APPDIR"
export PATH="$APPDIR/usr/bin${PATH:+:$PATH}"
export LIBMPV_LIBRARY_PATH="${LIBMPV_LIBRARY_PATH:-$APPDIR/usr/lib/libmpv.so}"
exec ./yaas "$@"
"""


def require(condition, message):
    if not condition:
        raise ValueError(message)


def is_host_abi(soname):
    return soname.startswith(HOST_ABI_PREFIXES)


def is_x86_64_elf(path):
    try:
        with path.open("rb") as stream:
            header = stream.read(20)
    except OSError:
        return False
    if len(header) < 20 or header[:4] != b"\x7fELF":
        return False
    endian = "<" if header[5] == 1 else ">"
    machine = struct.unpack_from(endian + "H", header, 18)[0]
    return machine == 62


def regular_elf_files(directory):
    if not directory.is_dir():
        return []
    return sorted(
        path
        for path in directory.iterdir()
        if path.is_file() and not path.is_symlink() and is_x86_64_elf(path)
    )


def read_dynamic(path):
    output = subprocess.check_output(["readelf", "-d", str(path)], text=True)
    needed = NEEDED_RE.findall(output)
    rpaths = RPATH_RE.findall(output)
    return needed, rpaths


def ldconfig_library_map():
    resolved = {}
    try:
        output = subprocess.check_output(["ldconfig", "-p"], text=True)
    except (OSError, subprocess.CalledProcessError):
        return resolved

    for line in output.splitlines():
        match = LDCONFIG_RE.match(line)
        if not match:
            continue
        soname, candidate_str = match.groups()
        if soname in resolved:
            continue
        candidate = Path(candidate_str)
        if candidate.is_file() and is_x86_64_elf(candidate):
            resolved[soname] = candidate
    return resolved


def update_from_ldd(path, lib_dirs, resolved):
    env = os.environ.copy()
    existing = env.get("LD_LIBRARY_PATH", "")
    search = ":".join(str(d) for d in lib_dirs if d.is_dir())
    env["LD_LIBRARY_PATH"] = f"{search}:{existing}" if existing else search

    result = subprocess.run(
        ["ldd", str(path)], env=env, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        return
    for line in result.stdout.splitlines():
        match = LDD_RESOLVED_RE.match(line)
        if not match:
            continue
        soname, candidate_str = match.groups()
        candidate = Path(candidate_str)
        if candidate.parent in lib_dirs:
            continue
        if candidate.is_file() and is_x86_64_elf(candidate):
            resolved.setdefault(Path(soname).name, candidate)


def copy_shared_object(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(0o755)


def patchelf(args, target):
    target.chmod(0o755)
    subprocess.run(["patchelf", *args, str(target)], check=True)


def write_launcher_files(appdir):
    apprun = appdir / "AppRun"
    apprun.write_text(APPRUN_CONTENT)
    apprun.chmod(0o755)

    desktop = appdir / "yaas.desktop"
    if desktop.is_file():
        cleaned = re.sub(
            r"^Exec=LD_LIBRARY_PATH=\S+\s+",
            "Exec=",
            desktop.read_text(),
            flags=re.MULTILINE,
        )
        desktop.write_text(cleaned)


def ensure_libmpv_symlink(usr_lib):
    mpv_libs = sorted(
        path.name
        for path in usr_lib.glob("libmpv.so.*")
        if path.is_file() and not path.is_symlink()
    )
    require(
        len(mpv_libs) == 1,
        f"Expected exactly one bundled libmpv.so.*, found: {mpv_libs}",
    )
    link = usr_lib / "libmpv.so"
    if link.exists() or link.is_symlink():
        link.unlink()
    link.symlink_to(mpv_libs[0])


def verify_appdir(appdir):
    main_bin = appdir / "yaas"
    lib_dir = appdir / "lib"
    usr_lib = appdir / "usr/lib"
    apprun = appdir / "AppRun"
    desktop = appdir / "yaas.desktop"

    require(main_bin.is_file() and is_x86_64_elf(main_bin), f"Missing binary: {main_bin}")
    require(apprun.is_file() and os.access(apprun, os.X_OK), "AppRun must be executable")
    require(
        "export LD_LIBRARY_PATH=" not in apprun.read_text(),
        "AppRun must not overwrite LD_LIBRARY_PATH",
    )
    if desktop.is_file():
        require(
            "LD_LIBRARY_PATH=" not in desktop.read_text(),
            "yaas.desktop must not hardcode LD_LIBRARY_PATH",
        )

    mpv_link = usr_lib / "libmpv.so"
    require(
        mpv_link.is_symlink() and mpv_link.resolve().is_file(),
        "usr/lib/libmpv.so must symlink to the bundled versioned libmpv",
    )

    bundled_names = {
        path.name
        for directory in (lib_dir, usr_lib)
        if directory.is_dir()
        for path in directory.iterdir()
    }

    forbidden = sorted(
        path.name for path in usr_lib.iterdir() if is_host_abi(path.name)
    )
    require(
        not forbidden,
        f"Host ABI libraries must not be bundled in usr/lib: {', '.join(forbidden)}",
    )

    targets = [main_bin, *regular_elf_files(lib_dir), *regular_elf_files(usr_lib)]
    for elf in targets:
        needed, rpaths = read_dynamic(elf)
        rel = elf.relative_to(appdir)

        for entry in needed:
            require(
                "/" not in entry,
                f"{rel} has slash-prefixed DT_NEEDED '{entry}'",
            )
            require(
                entry in bundled_names or is_host_abi(entry),
                f"{rel} requires unbundled non-host library '{entry}'",
            )

        # Dart's gen_snapshot AOT image (lib/libapp.so) has no DT_NEEDED entries.
        if not needed:
            continue

        require(rpaths, f"{rel} is missing DT_RUNPATH")
        for rpath in rpaths:
            for part in rpath.split(":"):
                require(
                    part == "$ORIGIN" or part.startswith("$ORIGIN/"),
                    f"{rel} has non-relocatable RUNPATH entry '{part}'",
                )


def bundle_linux_libs(appdir):
    appdir = appdir.resolve()
    main_bin = appdir / "yaas"
    lib_dir = appdir / "lib"
    usr_lib = appdir / "usr/lib"
    usr_lib.mkdir(parents=True, exist_ok=True)
    lib_dirs = (lib_dir, usr_lib)

    resolved = ldconfig_library_map()
    for seed in [main_bin, *regular_elf_files(lib_dir), *regular_elf_files(usr_lib)]:
        update_from_ldd(seed, lib_dirs, resolved)

    # Remove host-ABI libraries that fastforge copied into usr/lib.
    for path in list(usr_lib.iterdir()):
        if is_host_abi(path.name):
            path.unlink()

    queue = deque([main_bin, *regular_elf_files(lib_dir), *regular_elf_files(usr_lib)])
    visited = set()

    while queue:
        elf = queue.popleft()
        if elf in visited or not elf.is_file():
            continue
        visited.add(elf)

        needed_list, _ = read_dynamic(elf)
        if not needed_list:
            continue

        for needed in needed_list:
            soname = Path(needed).name
            if "/" in needed:
                if (
                    not is_host_abi(soname)
                    and not (lib_dir / soname).exists()
                    and not (usr_lib / soname).exists()
                ):
                    source = Path(needed) if Path(needed).is_file() else resolved.get(soname)
                    require(
                        source is not None and is_x86_64_elf(source),
                        f"Cannot resolve slash-prefixed DT_NEEDED '{needed}' for {elf}",
                    )
                    dest = usr_lib / soname
                    copy_shared_object(source, dest)
                    patchelf(["--set-soname", soname], dest)
                    update_from_ldd(dest, lib_dirs, resolved)
                    queue.append(dest)
                patchelf(["--replace-needed", needed, soname], elf)

            if is_host_abi(soname):
                continue
            if (lib_dir / soname).exists() or (usr_lib / soname).exists():
                continue

            if soname not in resolved:
                update_from_ldd(elf, lib_dirs, resolved)
            source = resolved.get(soname)
            require(
                source is not None and is_x86_64_elf(source),
                f"Cannot find shared library '{soname}' required by {elf.relative_to(appdir)}",
            )
            dest = usr_lib / soname
            copy_shared_object(source, dest)
            update_from_ldd(dest, lib_dirs, resolved)
            queue.append(dest)

    ensure_libmpv_symlink(usr_lib)

    patchelf(["--set-rpath", "$ORIGIN/lib:$ORIGIN/usr/lib"], main_bin)
    for elf in regular_elf_files(lib_dir):
        needed, _ = read_dynamic(elf)
        if needed:
            patchelf(["--set-rpath", "$ORIGIN:$ORIGIN/../usr/lib"], elf)
    for elf in regular_elf_files(usr_lib):
        needed, _ = read_dynamic(elf)
        if needed:
            patchelf(["--set-rpath", "$ORIGIN"], elf)

    write_launcher_files(appdir)
    verify_appdir(appdir)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("appdir", type=Path, help="Extracted squashfs-root directory")
    args = parser.parse_args()
    bundle_linux_libs(args.appdir)


if __name__ == "__main__":
    main()
