use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, run_checked};
use crate::ScriptLanguage;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["php"],
    version_arguments: &["--version"],
    install_hint: "PHP CLI",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
) -> Result<(), ExternalRuntimeError> {
    run_checked(
        ScriptLanguage::Php,
        Command::new(&program.executable).arg("-l").arg(source),
    )?;
    program.arguments = vec![source.as_os_str().to_os_string()];
    Ok(())
}
