#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows {
    use std::env;
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    // SW_HIDE only hides a console after Windows has initialized it. Avoid
    // console-host/terminal initialization entirely for installer background work.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn run(log: &mut File) -> io::Result<i32> {
        let executable = env::current_exe()?;
        let root = executable
            .parent()
            .ok_or_else(|| io::Error::other("Setup helper has no parent directory"))?;
        let mut arguments = env::args_os().skip(1);
        let action = arguments.next().unwrap_or_default();
        let mut command = if action == "--install-user-integrations" {
            if arguments.next().is_some() {
                return Err(io::Error::other("Unexpected integration arguments"));
            }
            let mut command = Command::new(root.join("rustic-agent-backend.exe"));
            command.arg("--install-user-integrations");
            command
        } else if action == "--install-language-toolchains" {
            let languages = arguments
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| io::Error::other("Missing language selection"))?;
            if arguments.next().is_some() {
                return Err(io::Error::other("Unexpected toolchain arguments"));
            }
            let system_root = env::var_os("SystemRoot")
                .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
            let mut command = Command::new(
                Path::new(&system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            );
            command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(root.join("tools/manage-language-toolchains.ps1"))
                .arg("-Languages")
                .arg(languages)
                .arg("-Setup");
            command
        } else {
            return Err(io::Error::other("Unknown setup helper action"));
        };

        writeln!(log, "Starting {}", action.display())?;
        let status = command
            .current_dir(root)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log.try_clone()?))
            .status()?;
        let code = status.code().unwrap_or(1);
        writeln!(log, "{} exited with code {code}", action.display())?;
        Ok(code)
    }

    pub fn main() -> i32 {
        let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
            return 1;
        };
        let directory = Path::new(&local_app_data).join("Rustic Game Engine/logs");
        if fs::create_dir_all(&directory).is_err() {
            return 1;
        }
        let Ok(mut log) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("setup-launch.log"))
        else {
            return 1;
        };
        match run(&mut log) {
            Ok(code) => code,
            Err(error) => {
                let _ = writeln!(log, "Setup helper failed: {error}");
                1
            }
        }
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    std::process::exit(windows::main());

    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("The setup helper requires Windows.");
        std::process::exit(1);
    }
}
