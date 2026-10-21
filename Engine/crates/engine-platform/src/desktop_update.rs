//! Start the packaged Windows updater independently of the desktop process.

/// Checks for releases in the background when running an installed Windows build.
/// Source builds and other platforms do not start an updater.
///
/// # Errors
/// Returns an error if the packaged updater cannot be launched.
pub fn start_desktop_updater() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use std::process::{Command, Stdio};

        let executable = std::env::current_exe()?;
        let Some(directory) = executable.parent() else {
            return Ok(());
        };
        let script = directory.join("tools/update-desktop.ps1");
        if !script.is_file() || !directory.join("build-commit.txt").is_file() {
            return Ok(());
        }
        let windows = std::env::var_os("SystemRoot").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "SystemRoot is unavailable")
        })?;
        Command::new(
            std::path::PathBuf::from(windows)
                .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        )
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-InstallDirectory")
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP: survive the desktop exit.
        .creation_flags(0x0800_0200)
        .spawn()?;
    }
    Ok(())
}
