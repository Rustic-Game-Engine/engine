use super::{ExternalRuntimeError, PreparedProgram, ToolchainSpec, run_checked};
use crate::ScriptLanguage;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub(super) const SPEC: ToolchainSpec = ToolchainSpec {
    candidates: &["clang", "gcc", "cl"],
    version_arguments: &["--version"],
    install_hint: "Clang, GCC, or MSVC",
};

pub(super) fn prepare(
    program: &mut PreparedProgram,
    source: &Path,
) -> Result<(), ExternalRuntimeError> {
    compile_native(program, source, ScriptLanguage::C, "-std=c17")
}

pub(super) fn compile_native(
    program: &mut PreparedProgram,
    source: &Path,
    language: ScriptLanguage,
    standard: &str,
) -> Result<(), ExternalRuntimeError> {
    let output = program.directory.path().join(if cfg!(windows) {
        "behavior.exe"
    } else {
        "behavior"
    });
    let compiler_name = program
        .executable
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut command = Command::new(&program.executable);
    if compiler_name == "cl" {
        command
            .args(["/nologo", "/W4"])
            .arg(source)
            .arg(format!("/Fe:{}", output.display()));
    } else {
        command
            .args(["-Wall", "-Wextra", "-Werror", standard])
            .arg(source)
            .arg("-o")
            .arg(&output);
    }
    run_checked(language, &mut command)?;
    program.executable = output;
    Ok(())
}
