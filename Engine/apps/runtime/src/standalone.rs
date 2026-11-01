//! A distributable game uses this executable as its window and an isolated child simulation.
use super::RuntimeWindowApp;
use engine_play::pointer_input::{PointerDevice, PointerEvent, PointerPhase};
use engine_play::{ControlRequest, PlayMode, PlaySnapshot, RuntimeLaunch, SupervisedRuntime};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use winit::event::{ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

pub(super) struct Standalone {
    runtime: SupervisedRuntime,
    _session: tempfile::TempDir,
    keys: BTreeSet<String>,
    cursor: [f64; 2],
    mouse_down: bool,
}

pub(super) fn run(executable: &Path) -> Result<(), String> {
    let directory = executable
        .parent()
        .ok_or("game executable has no parent directory")?;
    let data = directory.join("data");
    PlaySnapshot::open(&data).map_err(|e| format!("Could not load packaged game data: {e}"))?;
    let session = tempfile::Builder::new()
        .prefix("rustic-game-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let log_root =
        std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let title = executable
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut launch = RuntimeLaunch::new(
        executable,
        &data,
        log_root.join("RusticGames").join(&title).join("logs"),
        PlayMode::Standalone,
        session.path(),
    );
    launch.extra_arguments.push("--no-native-window".into());
    let runtime = SupervisedRuntime::spawn(&launch).map_err(|e| e.to_string())?;
    let finished = Arc::new(AtomicBool::new(false));
    let mut app = RuntimeWindowApp::new(Arc::clone(&finished), title, [1280.0, 720.0]);
    app.standalone = Some(Standalone {
        runtime,
        _session: session,
        keys: BTreeSet::new(),
        cursor: [0.0; 2],
        mouse_down: false,
    });
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    if let Some(game) = &mut app.standalone {
        let stop_result = game.stop();
        if app.error.is_none() {
            stop_result?;
        }
    }
    finished.store(true, Ordering::Release);
    app.error.map_or(Ok(()), Err)
}

impl Standalone {
    pub(super) fn frame(&mut self) -> Result<Option<engine_play::BgraFrame>, String> {
        if self
            .runtime
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err(
                "Game runtime exited. Check the game logs in RusticGames/<game>/logs.".into(),
            );
        }
        self.runtime
            .control(ControlRequest::QueryState, Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        for event in self.runtime.drain_console_events() {
            eprintln!("{event:?}");
        }
        self.runtime.take_latest_frame().map_err(|e| e.to_string())
    }
    pub(super) fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.runtime
            .set_viewport_extent(engine_play::game_view_extent([width, height]))
            .map_err(|e| e.to_string())
    }
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.runtime.stop().map(|_| ()).map_err(|e| e.to_string())
    }
    pub(super) fn event(&mut self, event: &WindowEvent, size: [u32; 2]) -> Result<(), String> {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = key_name(&event.logical_key) {
                    if event.state == ElementState::Pressed {
                        self.keys.insert(key);
                    } else {
                        self.keys.remove(&key);
                    }
                    self.runtime
                        .set_input_keys(self.keys.iter().cloned().collect())
                        .map_err(|e| e.to_string())?;
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.mouse_down = false;
                self.runtime
                    .set_input_keys(Vec::new())
                    .map_err(|e| e.to_string())?;
                self.runtime
                    .reset_pointer_input()
                    .map_err(|e| e.to_string())?;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x, position.y];
                if self.mouse_down {
                    self.pointer(
                        PointerDevice::Mouse,
                        "mouse".into(),
                        PointerPhase::Move,
                        self.cursor,
                        size,
                    )?;
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let phase = if *state == ElementState::Pressed {
                    PointerPhase::Start
                } else {
                    PointerPhase::End
                };
                if phase == PointerPhase::Start {
                    self.mouse_down = normalized(self.cursor, size)
                        .is_some_and(|p| p.iter().all(|v| (0.0..=1.0).contains(v)));
                }
                if self.mouse_down {
                    self.pointer(
                        PointerDevice::Mouse,
                        "mouse".into(),
                        phase,
                        self.cursor,
                        size,
                    )?;
                }
                if phase == PointerPhase::End {
                    self.mouse_down = false;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.mouse_down = false;
                self.runtime
                    .reset_pointer_input()
                    .map_err(|e| e.to_string())?;
            }
            WindowEvent::Touch(touch) => {
                let phase = match touch.phase {
                    TouchPhase::Started => PointerPhase::Start,
                    TouchPhase::Moved => PointerPhase::Move,
                    TouchPhase::Ended => PointerPhase::End,
                    TouchPhase::Cancelled => PointerPhase::Cancel,
                };
                self.pointer(
                    PointerDevice::Touch,
                    touch.id.to_string(),
                    phase,
                    [touch.location.x, touch.location.y],
                    size,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    fn pointer(
        &mut self,
        device: PointerDevice,
        id: String,
        phase: PointerPhase,
        position: [f64; 2],
        size: [u32; 2],
    ) -> Result<(), String> {
        let Some(position) = normalized(position, size) else {
            return Ok(());
        };
        if phase == PointerPhase::Start && !position.iter().all(|v| (0.0..=1.0).contains(v)) {
            return Ok(());
        }
        self.runtime
            .send_pointer_input(vec![PointerEvent {
                device,
                id,
                phase,
                position,
                pressure: 1.0,
                aspect: 16.0 / 9.0,
            }])
            .map_err(|e| e.to_string())
    }
}

fn key_name(key: &Key) -> Option<String> {
    match key {
        Key::Character(value) => Some(value.to_ascii_uppercase()),
        Key::Named(NamedKey::Space) => Some("Space".into()),
        Key::Named(value) => Some(format!("{value:?}")),
        _ => None,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "physical cursor coordinates are normalized to bounded game image coordinates"
)]
fn normalized(position: [f64; 2], [width, height]: [u32; 2]) -> Option<[f32; 2]> {
    if width == 0 || height == 0 {
        return None;
    }
    let width = f64::from(width);
    let height = f64::from(height);
    let game_width = width.min(height * 16.0 / 9.0);
    let game_height = height.min(width * 9.0 / 16.0);
    Some([
        ((position[0] - (width - game_width) / 2.0) / game_width) as f32,
        ((position[1] - (height - game_height) / 2.0) / game_height) as f32,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_mapping_matches_letterboxed_game_image() {
        assert_eq!(normalized([500.0, 500.0], [1000, 1000]), Some([0.5, 0.5]));
        assert!(normalized([500.0, 0.0], [1000, 1000]).unwrap()[1] < 0.0);
        assert_eq!(normalized([0.0, 0.0], [0, 0]), None);
    }
    #[test]
    fn missing_packaged_data_fails_before_starting_a_child() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            run(&temp.path().join("My Game.exe"))
                .unwrap_err()
                .contains("packaged game data")
        );
    }

    #[test]
    fn standalone_keys_match_editor_key_names() {
        assert_eq!(key_name(&Key::Character("w".into())), Some("W".into()));
        assert_eq!(key_name(&Key::Named(NamedKey::Space)), Some("Space".into()));
        assert_eq!(
            key_name(&Key::Named(NamedKey::ArrowUp)),
            Some("ArrowUp".into())
        );
    }
}
