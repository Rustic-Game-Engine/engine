use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, validate_with_stdin};
use crate::ScriptLanguage;
use std::path::Path;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["luau"],
    version_arguments: &["--version"],
    install_hint: "the Luau CLI",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
    bytes: &[u8],
) -> Result<(), ExternalRuntimeError> {
    validate_with_stdin(ScriptLanguage::Luau, bytes, &["--compile=-", "-"])?;
    program.arguments = vec![source.as_os_str().to_os_string()];
    Ok(())
}
