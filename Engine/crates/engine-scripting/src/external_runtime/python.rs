use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, run_checked};
use crate::ScriptLanguage;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["python", "python3"],
    version_arguments: &["--version"],
    install_hint: "Python 3",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
) -> Result<(), ExternalRuntimeError> {
    run_checked(
        ScriptLanguage::Python,
        Command::new(&program.executable)
            .args(["-I", "-m", "py_compile"])
            .arg(source),
    )?;
    program.arguments = vec![OsString::from("-I"), source.as_os_str().to_os_string()];
    Ok(())
}
