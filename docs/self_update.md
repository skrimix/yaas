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

`CancelAppUpdateRequest` cancels checks, downloads, and pending installation preparation. Once shutdown has committed the helper, cancellation is unavailable. Changing channels cancels outstanding work and invalidates the candidate. Flutter initiates the optional startup check using `Settings.check_updates_on_startup`; the backend does not schedule checks or downloads.

The snapshot includes download progress, release notes/link, installation availability, and an error category/message. Errors do not erase a previously verified download. A replaced nightly asset requires a fresh check. Stable updates compare numeric version/build; nightly updates compare workflow run/attempt. Switching channels can offer an older version.

## Installation and recovery

Packages are verified against the release manifest's size and SHA-256 before installation. The helper waits for process exit, journals replacement steps, and retains backups until the new core confirms its build identity. A replacement or process-launch failure restores the previous files and attempts to relaunch the previous app. Failure to confirm startup keeps the backup; it does not trigger a timed rollback.

Windows packages include `yaas-package.json`, which lists owned files. Updates preserve `_portable_data` and unrelated files. Existing installations without this inventory must first be replaced manually. Linux replaces the original AppImage. macOS replaces the complete bundle, preserving framework links and signatures. Read-only or translocated installations require moving/reinstalling the application first.

Transaction journals and helper logs are under `updates/transactions` in the app data directory. Backups are stored beside the installation, in the workspace recorded in `transaction.json`. An interrupted replacement or failed rollback blocks further installation. Close YAAS before restoring files from that journal; keep the journal and backup until recovery is complete. No privileges are elevated.

## Validation

`cargo test -p app-update` exercises file transactions and launches the real helper against fixture application processes. Core tests cover release discovery, pinned downloads, checksums, cancellation, and channel changes. Flutter tests cover startup checks, preference saves, update controls, banners, release notes, and the existing exit confirmation. Package CI runs helper tests on Linux, Windows, and macOS.

Before publishing installation support, test two packaged releases on each OS: download/install/relaunch, cancellation with active tasks, portable data, read-only destinations, and a failed launch. On Windows, include a running bundled ADB server. On macOS, include framework symlinks and both CPU architectures. On Linux, verify an AppImage launched from a directory containing spaces. Tests against fixture processes do not replace these packaged checks.
