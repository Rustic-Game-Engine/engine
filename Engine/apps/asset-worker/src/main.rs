use engine_assets::run_worker_stdio;
use engine_core::{ApplicationIdentity, ApplicationRole, run_headless_smoke_from_arguments};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use uuid::Uuid;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let identity = ApplicationIdentity::new("rustic-asset-worker", ApplicationRole::ScriptHost);
    match run_headless_smoke_from_arguments(&identity, &arguments) {
        Ok(true) => return ExitCode::SUCCESS,
        Ok(false) => {}
        Err(error) => {
            eprintln!("asset worker smoke failed: {error}");
            return ExitCode::FAILURE;
        }
    }

    match run(arguments.as_slice()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustic-asset-worker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[OsString]) -> Result<(), String> {
    if !arguments
        .iter()
        .any(|argument| argument == "--worker-stdio")
    {
        return Err("expected --worker-stdio --token <uuid>".to_owned());
    }
    let token = option_value(arguments, "--token")
        .and_then(OsStr::to_str)
        .ok_or_else(|| "missing or invalid --token".to_owned())?
        .parse::<Uuid>()
        .map_err(|error| format!("invalid worker token: {error}"))?;
    run_worker_stdio(token).map_err(|error| error.to_string())
}

fn option_value<'a>(arguments: &'a [OsString], name: &str) -> Option<&'a OsStr> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_direct_unframed_invocation() {
        assert!(run(&[]).is_err());
    }
}
