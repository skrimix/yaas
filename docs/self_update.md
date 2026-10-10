# Application updates

About contains update controls for packaged Windows, Linux AppImage, and macOS builds. Development builds can check and download releases, but cannot replace their installation.

## Using the UI

Choose stable or nightly on About, then select **Check for updates**. Channel changes save immediately and clear the previous candidate. Nightly builds may be less stable, and switching channels can offer an older version. Release notes are rendered as Markdown; links open in your browser.

YAAS checks once per launch after loading settings. Turn off **Check for updates on startup** on About to disable this. Startup failures remain on About. An available update shows a dismissible banner with a **View update** action; dismissing it hides the banner for the current session.

Select **Download update**, then **Install and restart** when the download is ready. Neither download nor installation starts automatically. Checks and downloads can be cancelled. Install uses the normal exit flow, including the active-task confirmation; cancelling it leaves the downloaded update ready. About shows installation restrictions and expandable error details when an action fails.

## Calling the backend

Subscribe to `AppUpdateStateChanged` and request a snapshot with `GetAppUpdateStateRequest`. Save `Settings.update_channel` to select stable or nightly. The default follows the installed channel; development builds default to stable.

1. Send `CheckAppUpdateRequest`. The phase becomes `Available` (or `UpToDate`).
2. Send `DownloadAppUpdateRequest`. `Downloading` covers the download, the checksum check, and staging; the phase then becomes `Ready`.
3. Exit the app with `AppShutdownRequest { install_update: true }`. After tasks and casting stop, the backend starts the install script; the app then exits.

`CancelAppUpdateRequest` cancels a check or download. Changing channels cancels outstanding work and drops the candidate and its download. Flutter initiates the optional startup check using `Settings.check_updates_on_startup`; the backend does not schedule checks or downloads.

The snapshot includes download progress, release notes/link, installation availability, and an error message. A failed check keeps a downloaded update ready; checking again for the same package keeps the download. Stable updates compare numeric version/build; nightly updates compare workflow run/attempt.

## Releases

Each GitHub release carries a `release.json` (written by `scripts/release.py`) with the build identity and each package's name, size, and SHA-256. The stable channel reads `releases/latest`; nightly reads the `nightly` tag. The package is downloaded by its GitHub asset ID, so a replaced nightly asset fails instead of mixing builds, and its size and SHA-256 must match `release.json`.

## Installation

Files live in `updates/` under the app data directory (the portable data directory in portable mode):

- `updates/<checksum prefix>/package`: the verified download.
- `updates/<checksum prefix>/new/`: the staged payload on Windows (the extracted ZIP) and macOS (`YAAS.app`, extracted with `ditto` and verified with `codesign --verify --deep --strict`). On Linux, the package itself is the AppImage and is only checked for the AppImage header.
- `updates/update.bat` or `update.sh`: the install script.
- `updates/update.log`: output of the last install script.

Staging happens right after the download, while the app is still running, so extraction or signature problems surface as ordinary errors. At startup everything in `updates/` except `update.log` is removed; downloads do not survive a restart.

On exit with an install request, Windows builds also stop the ADB server if its binary is bundled next to the app, so it doesn't lock installation files. The script then waits up to about 30 seconds for the app to exit (then kills it), replaces the installation, removes the update workspace, relaunches YAAS, and deletes itself:

- Windows copies the staged files over the install directory with `robocopy` (no purge: `_portable_data` and files absent from the package are preserved, including obsolete application files). On failure the script opens `update.log` in Notepad.
- Linux copies the AppImage next to the installed one with the installed permission bits, moves it into place atomically, and restarts it.
- macOS copies the staged bundle next to the installed one, then replaces the installed bundle and opens it.

Read-only or translocated installations require moving or reinstalling the application first. No privileges are elevated. There are no backups or rollback; a failed replacement may require a manual reinstall, and `update.log` remains for diagnosis. The relaunched app keeps only the `--portable` flag; other arguments and the working directory are not preserved.

## Validation

`cargo test -p hub update` covers release discovery, pinned downloads, checksums, cancellation, channel changes, workspace cleanup, package extraction, and script rendering for all platforms; on Linux, Windows, and macOS it also runs a real generated script against a fixture installation (package CI runs this on all three). Flutter tests cover startup checks, preference saves, update controls, banners, release notes, and the install exit request.

Before publishing installation support, test two packaged releases on each OS: download/install/relaunch (including portable mode), cancelling the exit with active tasks, read-only destinations, and a failed launch. On Windows, include a running bundled ADB server. On macOS, include framework symlinks and both CPU architectures. On Linux, include paths containing spaces. Fixture tests do not replace these packaged checks.
