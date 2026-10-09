use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, run_checked, runtime_io};
use crate::ScriptLanguage;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["dotnet"],
    version_arguments: &["--version"],
    install_hint: ".NET SDK 10 or newer",
};

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
