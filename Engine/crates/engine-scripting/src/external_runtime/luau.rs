use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, run_checked};
use crate::ScriptLanguage;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["rustic-luau-host"],
    version_arguments: &["--version"],
    install_hint: "the bundled Rustic Luau host (rebuild or reinstall Rustic)",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
    _bytes: &[u8],
) -> Result<(), ExternalRuntimeError> {
    run_checked(
        ScriptLanguage::Luau,
        Command::new(&program.executable)
            .arg("--validate")
            .arg(source),
    )?;
    program.arguments = vec![OsString::from(source.as_os_str())];
    Ok(())
}
