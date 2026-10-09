use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, c};
use crate::ScriptLanguage;
use std::path::Path;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["clang++", "g++", "cl"],
    version_arguments: &["--version"],
    install_hint: "Clang, GCC, or MSVC",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
) -> Result<(), ExternalRuntimeError> {
    c::compile_native(program, source, ScriptLanguage::Cpp, "-std=c++20")
}
