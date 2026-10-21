//! Start the packaged Windows updater independently of the desktop process.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// A verified release waiting for the desktop applications to close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopUpdate {
    pub tag: String,
}

/// File-based coordination with the independently running Windows updater.
pub struct DesktopUpdateClient {
    directory: PathBuf,
    build_commit: String,
}

impl DesktopUpdateClient {
    /// Returns a client for a packaged Windows installation, if present.
    ///
    /// # Errors
    /// Returns errors discovering the executable or reading its build identity.
    pub fn discover() -> io::Result<Option<Self>> {
        #[cfg(windows)]
        {
            use sha2::{Digest as _, Sha256};
            let executable = std::env::current_exe()?;
            let Some(root) = executable.parent() else {
                return Ok(None);
            };
            if !root.join("tools/update-desktop.ps1").is_file()
                || !root.join("build-commit.txt").is_file()
            {
                return Ok(None);
            }
            let build_commit = fs::read_to_string(root.join("build-commit.txt"))?
                .trim()
                .to_owned();
            let Some(local) = std::env::var_os("LOCALAPPDATA") else {
                return Ok(None);
            };
            let key = format!(
                "{:X}",
                Sha256::digest(root.to_string_lossy().to_lowercase().as_bytes())
            );
            Ok(Some(Self {
                directory: PathBuf::from(local)
                    .join("Rustic Game Engine/updates")
                    .join(key),
                build_commit,
            }))
        }
        #[cfg(not(windows))]
        Ok(None)
    }

    /// Reads a verified pending update belonging to this installed build.
    ///
    /// # Errors
    /// Returns filesystem errors other than a missing status file.
    pub fn pending(&self) -> io::Result<Option<DesktopUpdate>> {
        let contents = match fs::read_to_string(self.directory.join("pending-update.txt")) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut lines = contents.lines();
        let commit = lines.next().unwrap_or_default();
        let tag = lines.next().unwrap_or_default();
        if commit != self.build_commit || !valid_tag(tag) {
            return Ok(None);
        }
        Ok(Some(DesktopUpdate {
            tag: tag.to_owned(),
        }))
    }

    /// Atomically claims the one-time prompt across windows and app restarts.
    ///
    /// # Errors
    /// Returns filesystem errors saving the prompt decision.
    pub fn claim_prompt(&self, update: &DesktopUpdate) -> io::Result<bool> {
        if !valid_tag(&update.tag) {
            return Err(io::Error::other("Invalid update tag"));
        }
        fs::create_dir_all(&self.directory)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join(format!("prompted-{}.txt", update.tag)))
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Requests a graceful close in all desktop windows for this installation.
    ///
    /// # Errors
    /// Returns filesystem errors publishing the installation request.
    pub fn request_install(&self, update: &DesktopUpdate) -> io::Result<()> {
        if !valid_tag(&update.tag) {
            return Err(io::Error::other("Invalid update tag"));
        }
        fs::write(self.directory.join("install-request.txt"), &update.tag)
    }

    /// Records this window's project for restoration after a successful update.
    ///
    /// # Errors
    /// Returns filesystem errors or an invalid project path.
    pub fn prepare_restart(
        &self,
        update: &DesktopUpdate,
        project: Option<&Path>,
    ) -> io::Result<()> {
        if !valid_tag(&update.tag) {
            return Err(io::Error::other("Invalid update tag"));
        }
        let session = if let Some(project) = project {
            if !project.is_absolute() {
                return Err(io::Error::other("Project path must be absolute"));
            }
            format!("{}\neditor\n{}\n", update.tag, project.to_string_lossy())
        } else {
            format!("{}\nproject-manager\n", update.tag)
        };
        fs::write(
            self.directory
                .join(format!("resume-{}.txt", std::process::id())),
            session,
        )
    }

    /// Whether another desktop window requested this same update.
    pub fn install_requested(&self, update: &DesktopUpdate) -> bool {
        fs::read_to_string(self.directory.join("install-request.txt"))
            .is_ok_and(|tag| tag.trim() == update.tag)
    }

    /// Cancels the shared graceful-close request without removing the update.
    ///
    /// # Errors
    /// Returns filesystem errors other than a missing request file.
    pub fn cancel_install(&self) -> io::Result<()> {
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("resume-") && name.ends_with(".txt") {
                fs::remove_file(entry.path())?;
            }
        }
        match fs::remove_file(self.directory.join("install-request.txt")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 80
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

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
        .args(["-StateDirectory"])
        .arg(
            DesktopUpdateClient::discover()?
                .ok_or_else(|| io::Error::other("Could not discover the installed updater"))?
                .directory,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP: survive the desktop exit.
        .creation_flags(0x0800_0200)
        .spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_claimed_once_and_install_requests_are_release_specific() {
        let temp = tempfile::tempdir().unwrap();
        let client = DesktopUpdateClient {
            directory: temp.path().to_owned(),
            build_commit: "a".repeat(40),
        };
        let update = DesktopUpdate {
            tag: "v2.0-Pre-Alpha".into(),
        };
        fs::write(
            temp.path().join("pending-update.txt"),
            format!("{}\n{}\n", "a".repeat(40), update.tag),
        )
        .unwrap();
        assert_eq!(client.pending().unwrap(), Some(update.clone()));
        assert!(client.claim_prompt(&update).unwrap());
        assert!(!client.claim_prompt(&update).unwrap());
        let other_window = DesktopUpdateClient {
            directory: temp.path().to_owned(),
            build_commit: "a".repeat(40),
        };
        assert!(!other_window.claim_prompt(&update).unwrap());
        client.prepare_restart(&update, Some(temp.path())).unwrap();
        let resume = temp
            .path()
            .join(format!("resume-{}.txt", std::process::id()));
        let contents = fs::read_to_string(&resume).unwrap();
        assert_eq!(contents.lines().nth(1), Some("editor"));
        assert_eq!(contents.lines().nth(2), Some(temp.path().to_str().unwrap()));
        client.request_install(&update).unwrap();
        assert!(other_window.install_requested(&update));
        assert!(!other_window.install_requested(&DesktopUpdate {
            tag: "v2.1-Pre-Alpha".into()
        }));
        other_window.cancel_install().unwrap();
        assert!(!resume.exists());
        assert!(!client.install_requested(&update));
        let new_build = DesktopUpdateClient {
            directory: temp.path().to_owned(),
            build_commit: "b".repeat(40),
        };
        assert_eq!(new_build.pending().unwrap(), None);
    }

    #[test]
    fn malformed_status_cannot_create_paths_or_trigger_updates() {
        let temp = tempfile::tempdir().unwrap();
        let client = DesktopUpdateClient {
            directory: temp.path().to_owned(),
            build_commit: "a".repeat(40),
        };
        fs::write(
            temp.path().join("pending-update.txt"),
            format!("{}\n../bad\n", "a".repeat(40)),
        )
        .unwrap();
        assert_eq!(client.pending().unwrap(), None);
        assert!(
            client
                .claim_prompt(&DesktopUpdate {
                    tag: "../bad".into()
                })
                .is_err()
        );
    }
}
