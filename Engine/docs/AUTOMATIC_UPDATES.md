# Automatic desktop updates

Windows installers that include `tools/update-desktop.ps1` enable automatic updates
when Project Manager or the editor starts. Older installations need one manual
upgrade from [GitHub Releases](https://github.com/Rustic-Game-Engine/engine/releases)
to receive this feature. No Rust toolchain or GitHub login is needed.

The updater checks on launch and every hour while Rustic is open. It includes the
project's `vM.m-Pre-Alpha` releases, orders versions numerically, and also recognizes
`vM.m.p` stable releases. GitHub's latest-release shortcut is not used because it
excludes prereleases. Drafts and unrelated tag formats are ignored.

A newer Windows x64 installer downloads in the background. Its byte count and
SHA-256 must match the asset metadata returned by GitHub's HTTPS API before Setup
can run. Releases without a checksum or exactly one matching installer are skipped.
When an update is ready while Rustic is open, one window asks whether to install
it. **Not now** or closing the prompt dismisses it for that release, including
across restarts. A small **Update** button remains in the top-right title bar,
immediately left of the minimize/maximize/close controls, in both Project Manager
and the editor. Click it to install later. Newer releases can show a new prompt.

Choosing **Install update** or **Update** gracefully closes the desktop windows.
Editors ask to save unsaved scenes first; save failures leave the editor open.
Active Play sessions stop and their runtime changes can be reviewed before closing.
Canceling leaves the update available. Standalone games/workers must be closed
separately: Setup waits until all Rustic processes from that installation exit.

After successful installation, the updater reopens each window closed for the
update, restoring every editor's original project, including paths with spaces or
Unicode. It starts the newly installed executables as the original user. Failed
installations do not replay session records. An automatic installation after a
normal app exit does not reopen the app. The updater never force-closes an editor
or reboots Windows. Setup preserves its prior directory, installation scope,
language components and shortcut/PATH tasks. An all-users installation can require
Windows' elevation prompt; Windows security prompts cannot be suppressed.

The build script embeds the Git commit in Setup, which writes `build-commit.txt`
alongside the installed executables and records `install-scope.txt`. The release
publisher already targets the exact built commit. The updater finds that release
before comparing versions; the Cargo package version and timestamp in installer
filenames are not release versions. Source builds, unpublished workflow artifacts,
Linux and macOS do not automatically install releases. A build not yet associated
with a published release waits rather than replacing itself with an older build.

## Diagnose an update

Logs and temporary downloads are under
`%LOCALAPPDATA%\Rustic Game Engine\updates\<installation-key>\`. `update.log`
records download, verification and Setup failures; `update-setup.log` contains
Setup's own diagnostics. A file lock prevents duplicate workers from the same
user and installation. Network/API rate-limit and Setup failures leave the current
application usable and retry after an hour while it remains open or on a later
launch. If download completed but Setup has not started, close every Rustic process.
If a release is missing its installer/checksum, use its release page or wait for a
complete release. Do not edit `build-commit.txt` to force an update.

Run the network-free updater regression suite with Windows PowerShell 5.1:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test-desktop-updates.ps1
```

The Windows installer builder runs this suite before compiling the application.
PowerShell 7 can run the mocked suite on Linux, but that does not verify Windows
elevation, process visibility or the installed Setup executable.
