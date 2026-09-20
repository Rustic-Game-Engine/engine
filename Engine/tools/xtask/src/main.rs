use engine_scripting::{ScriptLanguage, probe_language_toolchain};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        return Err(usage());
    };
    let remaining: Vec<_> = arguments.collect();
    match command.to_string_lossy().as_ref() {
        "doctor" => doctor(),
        "build" => build(&remaining),
        "test" => test(),
        "run" => run_application(&remaining),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n{}", usage())),
    }
}

fn doctor() -> Result<(), String> {
    println!("Rustic workspace doctor");
    let expected = pinned_toolchain_version()?;
    let rustc_version = tool_output("rustc", &["--version"])?;
    if !rustc_version
        .split_whitespace()
        .any(|part| part == expected)
    {
        return Err(format!(
            "rustc does not match pinned toolchain {expected}: {}",
            rustc_version.trim()
        ));
    }
    println!("rustc {}", rustc_version.trim());
    run_tool("cargo", &["--version"])?;
    run_tool("rustup", &["target", "list", "--installed"])?;
    run_tool("cargo", &["deny", "--version"])?;
    require_file("Cargo.lock")?;
    require_file("deny.toml")?;
    require_file("rust-toolchain.toml")?;
    require_file("docs/ARCHITECTURE.md")?;
    require_file("docs/ROADMAP.md")?;
    require_file(".github/workflows/ci.yml")?;
    run_tool_quiet("cargo", &["metadata", "--format-version", "1", "--no-deps"])?;
    run_tool("cargo", &["deny", "check", "--hide-inclusion-graph"])?;
    println!("Required Rust tools, target metadata, lockfile, and license policy are available.");
    println!("Gameplay language adapters:");
    for language in [
        ScriptLanguage::Lua54,
        ScriptLanguage::JavaScript,
        ScriptLanguage::Python,
        ScriptLanguage::CSharp,
        ScriptLanguage::C,
        ScriptLanguage::Cpp,
        ScriptLanguage::Java,
        ScriptLanguage::Php,
        ScriptLanguage::Web,
    ] {
        let probe = probe_language_toolchain(language);
        let status = if probe.available {
            "available"
        } else {
            "not installed"
        };
        println!(
            "  {:<12} {:<13} {}",
            language.display_name(),
            status,
            probe.version.as_deref().unwrap_or(&probe.detail)
        );
    }
    Ok(())
}

fn build(arguments: &[OsString]) -> Result<(), String> {
    let profile = option_value(arguments, "--profile").unwrap_or_else(|| "dev".into());
    let profile_name = profile.to_string_lossy();
    if !matches!(
        profile_name.as_ref(),
        "dev" | "dev-fast" | "release" | "distribution"
    ) {
        return Err(format!("unknown build profile `{profile_name}`"));
    }
    if arguments.len() > 2 || (arguments.len() == 1 && arguments[0] != "--profile") {
        return Err("build accepts only `--profile <name>`".to_owned());
    }
    run_owned(
        "cargo",
        [
            OsString::from("build"),
            OsString::from("--locked"),
            OsString::from("--workspace"),
            OsString::from("--all-targets"),
            OsString::from("--profile"),
            profile,
        ],
    )
}

fn test() -> Result<(), String> {
    doctor()?;
    run_tool("cargo", &["fmt", "--all", "--check"])?;
    run_tool(
        "cargo",
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_tool("cargo", &["test", "--locked", "--workspace"])?;
    run_tool(
        "cargo",
        &["check", "--locked", "--workspace", "--all-targets"],
    )?;
    run_tool("cargo", &["deny", "check", "--hide-inclusion-graph"])?;
    run_tool(
        "cargo",
        &[
            "run",
            "--locked",
            "-p",
            "rustic-project-manager",
            "--",
            "--headless-smoke",
        ],
    )?;
    run_tool(
        "cargo",
        &[
            "run",
            "--locked",
            "-p",
            "rustic-editor",
            "--",
            "--headless-smoke",
        ],
    )?;
    verify_expected_failure(
        "cargo",
        &[
            "run",
            "--locked",
            "-p",
            "rustic-project-manager",
            "--",
            "--headless-smoke-fail",
        ],
    )
}

fn run_application(arguments: &[OsString]) -> Result<(), String> {
    let Some(application) = arguments.first() else {
        return Err("run requires `project-manager` or `editor`".to_owned());
    };
    let package = match application.to_string_lossy().as_ref() {
        "project-manager" => "rustic-project-manager",
        "editor" => "rustic-editor",
        other => return Err(format!("unknown application `{other}`")),
    };
    if package == "rustic-project-manager" {
        run_tool(
            "cargo",
            &["build", "--locked", "--package", "rustic-editor"],
        )?;
    }
    let mut cargo_arguments = vec![
        OsString::from("run"),
        OsString::from("--locked"),
        OsString::from("--package"),
        OsString::from(package),
    ];
    if arguments.len() > 1 {
        cargo_arguments.push(OsString::from("--"));
        cargo_arguments.extend_from_slice(&arguments[1..]);
    }
    run_owned("cargo", cargo_arguments)
}

fn option_value(arguments: &[OsString], name: &str) -> Option<OsString> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn require_file(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    if path.is_file() {
        println!("found {}", path.display());
        Ok(())
    } else {
        Err(format!("required file is missing: {}", path.display()))
    }
}

fn run_tool(program: &str, arguments: &[&str]) -> Result<(), String> {
    run_owned(program, arguments.iter().map(OsString::from))
}

fn run_tool_quiet(program: &str, arguments: &[&str]) -> Result<(), String> {
    println!("> {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(workspace_root()?)
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("could not start {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn tool_output(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(workspace_root()?)
        .output()
        .map_err(|error| format!("could not start {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited with {}", output.status));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{program} produced non-UTF-8 version output: {error}"))
}

fn verify_expected_failure(program: &str, arguments: &[&str]) -> Result<(), String> {
    println!("> {program} {} (expect failure)", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(workspace_root()?)
        .status()
        .map_err(|error| format!("could not start {program}: {error}"))?;
    if status.success() {
        Err(format!("{program} unexpectedly returned success"))
    } else {
        Ok(())
    }
}

fn pinned_toolchain_version() -> Result<String, String> {
    let path = workspace_root()?.join("rust-toolchain.toml");
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    contents
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("channel")
                .and_then(|value| value.split_once('=').map(|(_, value)| value))
                .map(str::trim)
                .map(|value| value.trim_matches('"').to_owned())
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "rust-toolchain.toml has no channel".to_owned())
}

fn run_owned(
    program: impl AsRef<Path>,
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(), String> {
    let program = program.as_ref();
    let arguments: Vec<_> = arguments.into_iter().collect();
    println!(
        "> {} {}",
        program.display(),
        arguments
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let status = Command::new(program)
        .args(&arguments)
        .current_dir(workspace_root()?)
        .status()
        .map_err(|error| format!("could not start {}: {error}", program.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited with {status}", program.display()))
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest is not below the workspace root".to_owned())
}

fn usage() -> String {
    "Usage: cargo xtask <doctor|build|test|run>\n\
     \n\
     cargo xtask doctor\n\
     cargo xtask build [--profile dev|dev-fast|release|distribution]\n\
     cargo xtask test\n\
     cargo xtask run <project-manager|editor> [application arguments]"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_parser_retains_native_values() {
        let arguments = [OsString::from("--profile"), OsString::from("dev-fast")];
        assert_eq!(
            option_value(&arguments, "--profile"),
            Some(OsString::from("dev-fast"))
        );
    }

    #[test]
    fn workspace_root_contains_the_root_manifest() {
        assert!(workspace_root().unwrap().join("Cargo.toml").is_file());
    }

    #[test]
    fn pinned_version_is_read_from_the_checked_in_toolchain() {
        assert_eq!(pinned_toolchain_version().unwrap(), "1.98.0");
    }
}
