use engine_play::{
    ControlRequest, PlayMode, RuntimeLaunch, RuntimeState, SnapshotBuilder, SnapshotInput,
    SupervisedRuntime, SupervisorExit,
};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

#[test]
fn real_runtime_child_authenticates_controls_all_modes_and_reaps() {
    let executable = Path::new(env!("CARGO_BIN_EXE_rustic-runtime"));
    for mode in [PlayMode::Play, PlayMode::NewWindow, PlayMode::Standalone] {
        let directory = tempfile::tempdir().unwrap();
        let play_directory = directory.path().join("temp/play");
        let logs_directory = directory.path().join("logs");
        let source_bytes = b"immutable authored scene".to_vec();
        let snapshot = SnapshotBuilder::new(&play_directory)
            .stage(
                mode,
                SnapshotInput::new("scenes/main.rscene", source_bytes.clone()),
                &[],
            )
            .unwrap();
        let mut launch = RuntimeLaunch::new(
            executable,
            snapshot.root(),
            &logs_directory,
            mode,
            directory.path().join("ipc"),
        );
        launch.connect_timeout = Duration::from_secs(10);
        launch.stop_grace_period = Duration::from_secs(3);
        // Native windows are a manual platform qualification. The real process,
        // authenticated IPC, simulation, frame transport, and cleanup remain active.
        launch.extra_arguments.push("--no-native-window".into());
        let mut runtime = SupervisedRuntime::spawn(&launch).unwrap();
        let initial_frame = runtime.take_latest_frame().unwrap().unwrap();
        assert_eq!((initial_frame.width, initial_frame.height), (64, 64));
        assert_eq!(
            initial_frame.pixels.len(),
            usize::try_from(initial_frame.stride_bytes * initial_frame.height).unwrap()
        );

        let running_deadline = std::time::Instant::now() + Duration::from_secs(2);
        let running = loop {
            let state = runtime
                .control(ControlRequest::QueryState, Duration::from_secs(2))
                .unwrap();
            if state.fixed_tick > 0 {
                break state;
            }
            assert!(
                std::time::Instant::now() < running_deadline,
                "fixed simulation did not begin"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(running.state, RuntimeState::Running);
        assert!(runtime.take_latest_frame().unwrap().is_some());

        let paused = runtime
            .control(ControlRequest::Pause, Duration::from_secs(2))
            .unwrap();
        thread::sleep(Duration::from_millis(50));
        let frozen = runtime
            .control(ControlRequest::QueryState, Duration::from_secs(2))
            .unwrap();
        assert_eq!(frozen.state, RuntimeState::Paused);
        assert_eq!(frozen.fixed_tick, paused.fixed_tick);

        let stepped = runtime
            .control(ControlRequest::FrameAdvance, Duration::from_secs(2))
            .unwrap();
        assert_eq!(stepped.fixed_tick, frozen.fixed_tick + 1);
        thread::sleep(Duration::from_millis(40));
        let still_frozen = runtime
            .control(ControlRequest::QueryState, Duration::from_secs(2))
            .unwrap();
        assert_eq!(still_frozen.fixed_tick, stepped.fixed_tick);
        assert_eq!(still_frozen.state, RuntimeState::Paused);

        runtime
            .control(ControlRequest::Resume, Duration::from_secs(2))
            .unwrap();
        thread::sleep(Duration::from_millis(40));
        let resumed = runtime
            .control(ControlRequest::QueryState, Duration::from_secs(2))
            .unwrap();
        assert!(resumed.fixed_tick > still_frozen.fixed_tick);

        let exit = runtime.stop().unwrap();
        assert!(matches!(exit, SupervisorExit::Graceful(_)));
        assert!(exit.status().success());
        let changes = runtime.take_runtime_changes().unwrap();
        assert!(changes.changes.is_empty());
        assert_eq!(snapshot.scene_bytes().unwrap(), source_bytes);
        assert!(
            fs::read_dir(&logs_directory)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with("runtime-"))
        );
        snapshot.remove().unwrap();
    }
}

#[test]
fn injected_runtime_crash_is_observed_and_persisted_without_touching_snapshot() {
    let executable = Path::new(env!("CARGO_BIN_EXE_rustic-runtime"));
    let directory = tempfile::tempdir().unwrap();
    let logs_directory = directory.path().join("logs");
    let source_bytes = b"saved source scene".to_vec();
    let snapshot = SnapshotBuilder::new(directory.path().join("temp/play"))
        .stage(
            PlayMode::Play,
            SnapshotInput::new("scenes/main.rscene", source_bytes.clone()),
            &[],
        )
        .unwrap();
    let mut launch = RuntimeLaunch::new(
        executable,
        snapshot.root(),
        &logs_directory,
        PlayMode::Play,
        directory.path().join("ipc"),
    );
    launch.connect_timeout = Duration::from_secs(10);
    launch.extra_arguments.push("--crash-after-ready".into());
    let mut runtime = SupervisedRuntime::spawn(&launch).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let exit = loop {
        if let Some(exit) = runtime.try_wait().unwrap() {
            break exit;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "runtime did not exit after injected crash"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(matches!(exit, SupervisorExit::Exited(_)));
    assert!(!exit.status().success());
    assert_eq!(snapshot.scene_bytes().unwrap(), source_bytes);
    assert!(
        fs::read_dir(&logs_directory)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
    );
    snapshot.remove().unwrap();
}

#[test]
fn unresponsive_runtime_is_force_killed_and_reaped_after_grace() {
    let executable = Path::new(env!("CARGO_BIN_EXE_rustic-runtime"));
    let directory = tempfile::tempdir().unwrap();
    let snapshot = SnapshotBuilder::new(directory.path().join("temp/play"))
        .stage(
            PlayMode::Play,
            SnapshotInput::new("scenes/main.rscene", b"scene".to_vec()),
            &[],
        )
        .unwrap();
    let mut launch = RuntimeLaunch::new(
        executable,
        snapshot.root(),
        directory.path().join("logs"),
        PlayMode::Play,
        directory.path().join("ipc"),
    );
    launch.connect_timeout = Duration::from_secs(10);
    launch.stop_grace_period = Duration::from_millis(100);
    launch.extra_arguments.push("--ignore-stop".into());
    let mut runtime = SupervisedRuntime::spawn(&launch).unwrap();

    let exit = runtime.stop().unwrap();
    assert!(matches!(exit, SupervisorExit::Forced(_)));
    snapshot.remove().unwrap();
}
