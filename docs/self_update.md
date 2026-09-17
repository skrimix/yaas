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

The app verifies the package's size and SHA-256 against the release manifest. It prepares an update request, confirms exit, and gives casting and tasks up to ten seconds to stop. It then starts a separate updater and exits.

Only one updater runs per OS user, across normal and portable installations. The updater waits up to five seconds for YAAS processes from the target installation to exit, then terminates remaining YAAS and bundled helper processes. It identifies bundled ADB by its executable path; external ADB servers are left running. If matching processes remain after another five seconds, installation stops.

The updater extracts and checks the package before changing installed files, then installs it and starts YAAS with its previous arguments and working directory:

- Windows overwrites files from the package. It preserves `_portable_data` and files absent from the package, including obsolete application files.
- Linux replaces the original AppImage and preserves its executable permissions.
- macOS replaces the complete bundle, preserving framework links and checking its signature.

Read-only or translocated installations require moving or reinstalling the application first. No privileges are elevated.

There are no backups, rollback, or startup acknowledgements. A failed or interrupted replacement may require a manual reinstall. Launching the new process finishes the update; the updater does not wait for the new app to initialize.

Requests and `updater.log` files live in `updates/requests` under YAAS's normal user data directory, including in portable mode. Failed requests and downloads remain available for diagnosis or retry. Successful runs remove their request and downloaded package; a later app launch cleans leftover updater copies. Old transaction directories are left untouched.

## Validation

`cargo test -p app-update` checks extraction, file replacement, process matching, and the real updater using fixture application processes. Core tests cover release discovery, pinned downloads, checksums, cancellation, and channel changes. Flutter tests cover startup checks, preference saves, update controls, banners, release notes, and exit confirmation. Package CI runs updater tests on Linux, Windows, and macOS.

Before publishing installation support, test two packaged releases on each OS: download/install/relaunch, cancellation with active tasks, portable data, read-only destinations, and a failed launch. On Windows, include a running bundled ADB server. On macOS, include framework symlinks and both CPU architectures. On Linux, include multiple AppImage mounts and paths containing spaces. Fixture tests do not replace these packaged checks.
