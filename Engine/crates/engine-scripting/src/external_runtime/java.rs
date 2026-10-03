use super::{
    ExternalRuntimeError, PreparedProgram, ToolchainSpec, find_executable, run_checked, runtime_io,
};
use crate::ScriptLanguage;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["java"],
    version_arguments: &["--version"],
    install_hint: "OpenJDK 11 or newer (java and javac)",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
) -> Result<(), ExternalRuntimeError> {
    let javac = find_executable(&["javac"]).ok_or(ExternalRuntimeError::ToolchainUnavailable(
        "Java",
        "OpenJDK javac",
    ))?;
    let classes = program.directory.path().join("classes");
    std::fs::create_dir(&classes).map_err(|error| runtime_io(ScriptLanguage::Java, error))?;
    run_checked(
        ScriptLanguage::Java,
        Command::new(javac).arg("-d").arg(&classes).arg(source),
    )?;
    program.arguments = vec![
        OsString::from("-cp"),
        classes.into_os_string(),
        OsString::from("RusticBehavior"),
    ];
    Ok(())
}
