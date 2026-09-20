use engine_editor::AuthoringDocument;
use engine_play::{
    ControlAck, ControlRequest, PlayMode, PlaySnapshot, RuntimeChangeSet, RuntimeLaunch,
    RuntimeState, SnapshotBuilder, SnapshotInput, SupervisedRuntime,
};
use engine_project::VirtualDirectory;
use engine_scripting::{GAME_SETTINGS_FILE, GameSettings, load_manifest, validate_script};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct EditorPlaySession {
    runtime: SupervisedRuntime,
    snapshot: Option<PlaySnapshot>,
    mode: PlayMode,
    state: RuntimeState,
    fixed_tick: u64,
    last_frame_request: std::time::Instant,
}

impl EditorPlaySession {
    pub fn start(document: &AuthoringDocument, mode: PlayMode) -> Result<Self, String> {
        // Startup scripts may create or select the current camera. Let the live
        // runtime choose it after those scripts run, rather than rejecting the
        // authoring snapshot before initialization.
        let scene = document
            .snapshot_bytes()
            .map_err(|error| error.to_string())?;
        let temporary = document
            .project()
            .directory(VirtualDirectory::Temp)
            .map_err(|error| error.to_string())?;
        let logs = document
            .project()
            .directory(VirtualDirectory::Logs)
            .map_err(|error| error.to_string())?;
        let mut programming = Vec::new();
        GameSettings::load(document.project().root())?;
        let settings_bytes = std::fs::read(document.project().root().join(GAME_SETTINGS_FILE))
            .map_err(|error| error.to_string())?;
        programming.push(SnapshotInput::new(GAME_SETTINGS_FILE, settings_bytes));
        let manifest_path = document.project().root().join("config/scripts.ron");
        if manifest_path.is_file() {
            let manifest_bytes =
                std::fs::read(&manifest_path).map_err(|error| error.to_string())?;
            let manifest = load_manifest(&manifest_bytes).map_err(|error| error.to_string())?;
            programming.push(SnapshotInput::new("config/scripts.ron", manifest_bytes));
            for entry in manifest.manifest.scripts {
                let path = document.project().root().join(&entry.relative_path);
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("could not read {}: {error}", path.display()))?;
                validate_script(
                    entry.language,
                    &bytes,
                    &entry.relative_path.to_string_lossy(),
                    1024 * 1024,
                )?;
                programming.push(SnapshotInput::new(entry.relative_path, bytes));
            }
        }
        collect_snapshot_assets(document.project().root(), &mut programming)?;
        let snapshot = SnapshotBuilder::new(temporary.join("play"))
            .stage(
                mode,
                SnapshotInput::new(
                    document
                        .scene_path()
                        .strip_prefix(document.project().root())
                        .unwrap_or_else(|_| Path::new("scene/main.scene")),
                    scene,
                ),
                &programming,
            )
            .map_err(|error| error.to_string())?;
        let executable = runtime_executable().ok_or_else(|| {
            "rustic-runtime executable is not beside the editor; run `cargo build --workspace`"
                .to_owned()
        })?;
        let launch = RuntimeLaunch::new(
            executable,
            snapshot.root(),
            logs,
            mode,
            temporary.join("ipc"),
        );
        let runtime = match SupervisedRuntime::spawn(&launch) {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = snapshot.remove();
                return Err(error.to_string());
            }
        };
        Ok(Self {
            runtime,
            snapshot: Some(snapshot),
            mode,
            state: RuntimeState::Running,
            fixed_tick: 0,
            last_frame_request: std::time::Instant::now(),
        })
    }

    pub const fn mode(&self) -> PlayMode {
        self.mode
    }

    pub const fn state(&self) -> RuntimeState {
        self.state
    }

    pub const fn fixed_tick(&self) -> u64 {
        self.fixed_tick
    }

    pub fn control(&mut self, request: ControlRequest) -> Result<ControlAck, String> {
        let ack = self
            .runtime
            .control(request, Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        self.state = ack.state;
        self.fixed_tick = ack.fixed_tick;
        Ok(ack)
    }

    pub fn reload_scripts(&mut self, project_root: &Path) -> Result<Vec<String>, String> {
        let bytes = std::fs::read(project_root.join("config/scripts.ron"))
            .map_err(|error| error.to_string())?;
        let manifest = load_manifest(&bytes).map_err(|error| error.to_string())?;
        let mut outcomes = Vec::new();
        for entry in manifest.manifest.scripts {
            let source = std::fs::read(project_root.join(&entry.relative_path))
                .map_err(|error| error.to_string())?;
            validate_script(
                entry.language,
                &source,
                &entry.relative_path.to_string_lossy(),
                1024 * 1024,
            )?;
            outcomes.push(
                self.runtime
                    .reload_script(entry.id, source, Duration::from_secs(2))
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(outcomes)
    }

    pub fn stop(mut self) -> Result<Option<RuntimeChangeSet>, String> {
        self.runtime.stop().map_err(|error| error.to_string())?;
        let changes = self.runtime.take_runtime_changes();
        if let Some(snapshot) = self.snapshot.take() {
            snapshot.remove().map_err(|error| error.to_string())?;
        }
        Ok(changes)
    }

    pub fn drain_console(&mut self) -> impl Iterator<Item = engine_play::ConsoleEvent> + '_ {
        self.runtime.drain_console_events()
    }

    pub fn take_latest_frame(&mut self) -> Result<Option<engine_play::BgraFrame>, String> {
        if self.last_frame_request.elapsed() >= Duration::from_millis(33) {
            self.control(ControlRequest::QueryState)?;
            self.last_frame_request = std::time::Instant::now();
        }
        self.runtime
            .take_latest_frame()
            .map_err(|error| error.to_string())
    }

    pub fn has_exited(&mut self) -> Result<bool, String> {
        self.runtime
            .try_wait()
            .map(|exit| exit.is_some())
            .map_err(|error| error.to_string())
    }
}

fn collect_snapshot_assets(root: &Path, output: &mut Vec<SnapshotInput>) -> Result<(), String> {
    let mut pending = ["assets", "ui"]
        .into_iter()
        .map(|directory| root.join(directory))
        .filter(|directory| directory.is_dir())
        .collect::<Vec<_>>();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf();
                let bytes = std::fs::read(entry.path()).map_err(|error| error.to_string())?;
                if !output.iter().any(|input| input.relative_path == relative) {
                    output.push(SnapshotInput::new(relative, bytes));
                }
            }
        }
    }
    Ok(())
}

impl Drop for EditorPlaySession {
    fn drop(&mut self) {
        if let Some(snapshot) = self.snapshot.take() {
            let _ = snapshot.remove();
        }
    }
}

fn runtime_executable() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let directory = current.parent()?;
    let candidate = directory.join(executable_name("rustic-runtime"));
    candidate.is_file().then_some(candidate)
}

fn executable_name(stem: &str) -> PathBuf {
    if cfg!(windows) {
        Path::new(&format!("{stem}.exe")).to_path_buf()
    } else {
        Path::new(stem).to_path_buf()
    }
}
