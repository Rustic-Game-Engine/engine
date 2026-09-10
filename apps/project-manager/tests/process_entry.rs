use std::process::Command;

#[test]
fn headless_smoke_success_and_failure_reach_the_process_exit_code() {
    let executable = env!("CARGO_BIN_EXE_rustic-project-manager");
    let success = Command::new(executable)
        .arg("--headless-smoke")
        .status()
        .expect("run success smoke");
    assert!(success.success());

    let failure = Command::new(executable)
        .arg("--headless-smoke-fail")
        .status()
        .expect("run failure smoke");
    assert!(!failure.success());
}
