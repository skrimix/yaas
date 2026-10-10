# Application updates

About contains update controls for packaged Windows, Linux AppImage, and macOS builds. Development builds can check and download releases, but cannot replace their installation.

## Using the UI

Choose stable or nightly on About, then select **Check for updates**. Channel changes save immediately and clear the previous candidate. Nightly builds may be less stable, and switching channels can offer an older version. Release notes are rendered as Markdown; links open in your browser.

YAAS checks once per launch after loading settings. Turn off **Check for updates on startup** on About to disable this. Existing settings default to enabled. Startup failures remain on About. An available update shows a dismissible banner with a **View update** action; dismissing it hides that candidate for the current session.

Select **Download update**, then **Install and restart** when the download is ready. Neither download nor installation starts automatically. Checks, downloads, and installation preparation can be cancelled. Restart uses the existing active-task confirmation; cancelling leaves the downloaded update ready. About shows installation restrictions and expandable error details when an action fails.

## Calling the backend

Subscribe to `AppUpdateStateChanged` and request a snapshot with `GetAppUpdateStateRequest`. Save `Settings.update_channel` to select stable or nightly. The default follows the installed channel; development builds default to stable.

1. Send `CheckAppUpdateRequest`.
2. If the phase is `Available`, pass the release's `candidate_id` to `DownloadAppUpdateRequest`.
3. Wait for `Ready`, then pass the same ID to `InstallAppUpdateRequest`.
4. Flutter handles `AppUpdateExitRequested` through its existing exit confirmation. Cancelling that confirmation leaves the downloaded update ready.

`CancelAppUpdateRequest` cancels checks, downloads, and pending installation preparation. Once shutdown begins, cancellation is unavailable. Changing channels cancels outstanding work and invalidates the candidate. Flutter initiates the optional startup check using `Settings.check_updates_on_startup`; the backend does not schedule checks or downloads.

The snapshot includes download progress, release notes/link, installation availability, and an error category/message. Errors do not erase a previously verified download. A replaced nightly asset requires a fresh check. Stable updates compare numeric version/build; nightly updates compare workflow run/attempt. Switching channels can offer an older version.

## Installation

The app verifies the package's size and SHA-256 against the release manifest, then stages the update while still running, so extraction or signature problems surface as ordinary errors. It confirms exit and gives casting and tasks up to ten seconds to stop. The app stops its ADB tasks, then kills every other process whose executable lives inside the installation (the install directory on Windows, the AppImage mount on Linux, the bundle on macOS): a bundled ADB server, leftover archive tools or rclone, and other YAAS instances from the same installation. This keeps them from locking files or keeping an old AppImage mount alive; ADB servers started from other adb binaries are left running. The app then starts a small generated script, detached, and exits.

Staging lives in `updates/staging-<candidate-id>` under the app data directory (the portable data directory in portable mode), and the script is `updates/update-<candidate-id>.bat` or `.sh`. The script waits up to about 30 seconds for the app process to exit (then kills it), creates an install lock directory with `mkdir`, replaces the installation, removes the lock, restarts YAAS, and removes the staging directory, the downloaded package, and itself:

The lock is `.yaas-update.lock` inside the install directory on Windows and `<AppImage or bundle>.yaas-update.lock` next to the target elsewhere, so only one script replaces a given installation at a time. If the lock already exists, the script exits without installing. Staging also fails while a lock younger than 30 minutes exists; older locks are left by interrupted scripts and are removed.

- Windows extracts the ZIP during staging, then the script copies the staged files over the install directory with `robocopy` (no purge: `_portable_data` and files absent from the package are preserved, including obsolete application files). On failure the script opens its `update.log` in Notepad and leaves the staging directory in place.
- Linux needs no extraction: the package is the AppImage. The script copies it over the installed AppImage atomically (sibling file with the installed AppImage's permission bits, `mv`) and restarts it.
- macOS extracts the ZIP with `ditto` during staging (preserving framework symlinks) and verifies the staged bundle with `codesign --verify --deep --strict`. The script replaces the whole bundle (`rm -rf` + `ditto`) and opens it.

Read-only or translocated installations require moving or reinstalling the application first. No privileges are elevated.

There are no backups or rollback. A failed or interrupted replacement may require a manual reinstall; the script's `update.log` and staging directory remain for diagnosis. Launching the new process finishes the update; the script does not wait for the new app to initialize. The relaunched app keeps only the `--portable` flag; other arguments and the working directory are not preserved.

If staging itself fails, the staging directory and script are removed right away. Staging directories and scripts left behind by failed or interrupted installs are removed at startup once they are a day old.

## Validation

`cargo test -p hub update` covers release discovery, pinned downloads, checksums, cancellation, channel changes, package extraction checks, and script rendering for all platforms; on Linux, Windows, and macOS it also runs a real generated script against a fixture installation (package CI runs this on all three). Flutter tests cover startup checks, preference saves, update controls, banners, release notes, and exit confirmation.

Before publishing installation support, test two packaged releases on each OS: download/install/relaunch (including portable mode), cancellation with active tasks, read-only destinations, and a failed launch. On Windows, include a running bundled ADB server. On macOS, include framework symlinks and both CPU architectures. On Linux, include multiple AppImage mounts and paths containing spaces. Fixture tests do not replace these packaged checks.
