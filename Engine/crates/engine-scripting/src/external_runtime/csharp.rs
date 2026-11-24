use super::{
    ExternalRuntimeError, LanguageAvailability, PreparedProgram, ToolchainSpec, run_checked,
    runtime_io, toolchain::find_executable_matching,
};
use crate::ScriptLanguage;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &[if cfg!(windows) {
        "dotnet.exe"
    } else {
        "dotnet"
    }],
    version_arguments: &["--list-sdks"],
    install_hint: ".NET SDK 10 or newer",
};

pub(super) fn probe() -> LanguageAvailability {
    let (executable, version) =
        find_dotnet_sdk(10).map_or((None, None), |(path, version)| (Some(path), Some(version)));
    LanguageAvailability {
        language: ScriptLanguage::CSharp,
        available: executable.is_some(),
        executable,
        version,
        detail: format!("expected {}", SPEC.install_hint),
    }
}

/// Finds a .NET CLI with an SDK at or above the requested major version.
pub fn find_dotnet_sdk(minimum_major: u32) -> Option<(PathBuf, String)> {
    let mut version = None;
    let path = find_executable_matching(SPEC.candidates, install_directories, |path| {
        version = Command::new(path)
            .args(SPEC.version_arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                sdk_version(&String::from_utf8_lossy(&output.stdout), minimum_major)
                    .map(str::to_owned)
            });
        version.is_some()
    })?;
    Some((std::path::absolute(path).ok()?, version?))
}

fn sdk_version(list: &str, minimum_major: u32) -> Option<&str> {
    list.lines()
        .filter_map(|line| {
            let (version, location) = line.trim().split_once(" [")?;
            if !location.ends_with(']') {
                return None;
            }
            let major = version.split_once('.')?.0.parse::<u32>().ok()?;
            (major >= minimum_major).then_some((major, version))
        })
        .max_by_key(|&(major, _)| major)
        .map(|(_, version)| version)
}

fn install_directories() -> Vec<PathBuf> {
    let mut directories: Vec<_> = std::env::var_os("DOTNET_ROOT")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    #[cfg(windows)]
    {
        for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(variable) {
                let dotnet = PathBuf::from(root).join("dotnet");
                directories.push(dotnet.clone());
                directories.push(dotnet.join("x64"));
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            directories.push(PathBuf::from(home).join(".dotnet"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            directories.push(PathBuf::from(local).join("Microsoft/dotnet"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            directories.push(PathBuf::from(home).join(".dotnet"));
        }
        directories.extend(
            [
                "/usr/share/dotnet",
                "/usr/lib/dotnet",
                "/usr/local/share/dotnet",
            ]
            .map(PathBuf::from),
        );
    }
    directories
}

pub(super) fn prepare(
    program: &mut PreparedProgram,
    _source: &Path,
    version: Option<String>,
) -> Result<(), ExternalRuntimeError> {
    let version = version.unwrap_or_else(|| "10.0".into());
    let major = version.split('.').next().unwrap_or("10");
    let project = program.directory.path().join("RusticBehavior.csproj");
    std::fs::write(
        &project,
        format!("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net{major}.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup></Project>"),
    ).map_err(|error| runtime_io(ScriptLanguage::CSharp, error))?;
    run_checked(
        ScriptLanguage::CSharp,
        Command::new(&program.executable)
            .current_dir(program.directory.path())
            .args(["build", "-c", "Release", "--nologo"])
            .arg(&project),
    )?;
    let dll = program
        .directory
        .path()
        .join(format!("bin/Release/net{major}.0/RusticBehavior.dll"));
    program.arguments = vec![dll.into_os_string()];
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sdk_version;

    #[test]
    fn sdk_listing_requires_a_supported_sdk() {
        assert_eq!(sdk_version("", 10), None);
        assert_eq!(sdk_version("9.0.103 [/usr/share/dotnet/sdk]\n", 10), None);
        assert_eq!(
            sdk_version("8.0.419 [/usr/share/dotnet/sdk]\n", 8),
            Some("8.0.419")
        );
        assert_eq!(
            sdk_version(
                "9.0.103 [C:\\Program Files\\dotnet\\sdk]\r\n10.0.205 [C:\\Program Files\\dotnet\\sdk]\r\n",
                10,
            ),
            Some("10.0.205")
        );
        assert_eq!(
            sdk_version("11.0.100-preview.1 [/sdk]\n10.0.201 [/sdk]\n", 10),
            Some("11.0.100-preview.1")
        );
    }
}
