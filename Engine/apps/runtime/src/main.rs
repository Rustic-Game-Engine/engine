use engine_core::{
    ApplicationIdentity, ApplicationRole, headless_smoke_requested, run_headless_smoke,
};
use engine_play::{
    AuthenticationToken, LocalEndpoint, PlayMode, RuntimeServerConfig, record_runtime_crash,
    run_runtime_server_with_renderer,
};
use glam::Mat4;

use renderer_wgpu::MeshVertex;
use renderer_wgpu::{BackendRequest, SurfaceRenderer, TexturedMesh};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

mod game_ui;

const TOKEN_ENVIRONMENT_VARIABLE: &str = "RUSTIC_RUNTIME_IPC_TOKEN";

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if headless_smoke_requested(&arguments) {
        let identity = ApplicationIdentity::new("rustic-runtime", ApplicationRole::Runtime);
        return match run_headless_smoke(&identity) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Headless smoke failed: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let logs_directory = option_value(&arguments, "--logs").map(PathBuf::from);
    let outcome = std::panic::catch_unwind(|| run(&arguments));
    match outcome {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(error)) => {
            if let Some(directory) = logs_directory {
                record_runtime_crash(&directory, error.clone());
            }
            eprintln!("rustic-runtime: {error}");
            ExitCode::FAILURE
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map_or_else(
                    || {
                        payload
                            .downcast_ref::<String>()
                            .map_or("runtime panicked", String::as_str)
                    },
                    |message| *message,
                )
                .to_owned();
            if let Some(directory) = logs_directory {
                record_runtime_crash(&directory, &message);
            }
            eprintln!("rustic-runtime panic: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[OsString]) -> Result<(), String> {
    if !arguments.iter().any(|argument| argument == "--ipc-server") {
        return Err(usage());
    }
    let endpoint_kind = required_utf8(arguments, "--ipc-kind")?;
    let endpoint_address = required_value(arguments, "--ipc-address")?;
    let endpoint = LocalEndpoint::from_arguments(endpoint_kind, endpoint_address)
        .map_err(|error| format!("invalid local IPC endpoint: {error}"))?;
    let snapshot_root = PathBuf::from(required_value(arguments, "--snapshot")?);
    let logs_directory = PathBuf::from(required_value(arguments, "--logs")?);
    let mode = required_utf8(arguments, "--mode")?
        .parse::<PlayMode>()
        .map_err(|error| error.to_string())?;
    let fixed_step_nanos = required_utf8(arguments, "--fixed-step-nanos")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --fixed-step-nanos: {error}"))?;
    let token_text = std::env::var(TOKEN_ENVIRONMENT_VARIABLE)
        .map_err(|_| format!("missing {TOKEN_ENVIRONMENT_VARIABLE}"))?;
    let authentication_token =
        AuthenticationToken::from_hex(&token_text).map_err(|error| error.to_string())?;
    let crash_after_ready = arguments
        .iter()
        .any(|argument| argument == "--crash-after-ready");
    let ignore_stop = arguments.iter().any(|argument| argument == "--ignore-stop");
    let suppress_native_window = arguments
        .iter()
        .any(|argument| argument == "--no-native-window");
    let config = RuntimeServerConfig {
        endpoint,
        authentication_token,
        snapshot_root,
        logs_directory,
        mode,
        fixed_delta: Duration::from_nanos(fixed_step_nanos),
        crash_after_ready,
        ignore_stop,
    };
    if requires_native_window(mode, suppress_native_window) {
        run_windowed_runtime(config)
    } else {
        render_runtime(config, None)
    }
}

const fn requires_native_window(mode: PlayMode, suppress_native_window: bool) -> bool {
    !suppress_native_window && matches!(mode, PlayMode::NewWindow | PlayMode::Standalone)
}

fn run_windowed_runtime(config: RuntimeServerConfig) -> Result<(), String> {
    let mode = config.mode;
    let finished = Arc::new(AtomicBool::new(false));
    let server_result = Arc::new(Mutex::new(None));
    let server_finished = Arc::clone(&finished);
    let result_slot = Arc::clone(&server_result);
    let latest_frame = Arc::new(Mutex::new(None));
    let server_frames = Arc::clone(&latest_frame);
    let server = thread::Builder::new()
        .name("rustic-runtime-server".to_owned())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_runtime(config, Some(server_frames))
            }))
            .unwrap_or_else(|payload| {
                Err(format!(
                    "runtime server panicked: {}",
                    panic_payload_message(payload.as_ref())
                ))
            });
            if let Ok(mut slot) = result_slot.lock() {
                *slot = Some(outcome);
            }
            server_finished.store(true, Ordering::Release);
        })
        .map_err(|error| format!("could not start runtime server thread: {error}"))?;

    let (title, size) = match mode {
        PlayMode::Play => unreachable!("embedded play does not create a runtime window"),
        PlayMode::NewWindow => ("Rustic Runtime — New Window", [960.0, 540.0]),
        PlayMode::Standalone => ("Rustic Game — Standalone", [1_280.0, 720.0]),
    };
    let event_loop = EventLoop::new()
        .map_err(|error| format!("could not create native runtime event loop: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut window_app = RuntimeWindowApp::new(Arc::clone(&finished), title, size);
    window_app.latest_frame = latest_frame;
    event_loop
        .run_app(&mut window_app)
        .map_err(|error| format!("native runtime event loop failed: {error}"))?;
    if let Some(error) = window_app.error.take() {
        return Err(error);
    }
    if !finished.load(Ordering::Acquire) {
        return Err("native runtime window was closed before the editor stopped play".to_owned());
    }
    server.join().map_err(|payload| {
        format!(
            "runtime server thread panicked: {}",
            panic_payload_message(payload.as_ref())
        )
    })?;
    server_result
        .lock()
        .map_err(|_| "runtime server result synchronization was poisoned".to_owned())?
        .take()
        .unwrap_or_else(|| Err("runtime server exited without a result".to_owned()))
}

struct RuntimeWindowApp {
    finished: Arc<AtomicBool>,
    title: &'static str,
    initial_size: [f64; 2],
    window: Option<Arc<Window>>,
    renderer: Option<SurfaceRenderer>,
    mesh: TexturedMesh,
    latest_frame: Arc<Mutex<Option<engine_play::BgraFrame>>>,
    error: Option<String>,
}

impl RuntimeWindowApp {
    fn new(finished: Arc<AtomicBool>, title: &'static str, initial_size: [f64; 2]) -> Self {
        Self {
            finished,
            title,
            initial_size,
            window: None,
            renderer: None,
            mesh: frame_quad(),
            latest_frame: Arc::new(Mutex::new(None)),
            error: None,
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: impl Into<String>) {
        self.error = Some(message.into());
        event_loop.exit();
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are bounded by GPU texture limits and converted to float layout coordinates"
    )]
    fn resize_surface(&mut self, event_loop: &ActiveEventLoop, width: u32, height: u32) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        if let Err(error) = renderer.resize(width, height, window.scale_factor()) {
            self.fail(
                event_loop,
                format!("native runtime surface resize failed: {error}"),
            );
        } else if width > 0 && height > 0 {
            self.mesh.model_view_projection = frame_fit(width as f32 / height as f32);
        }
    }
}

impl ApplicationHandler for RuntimeWindowApp {
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are bounded by GPU texture limits and converted to float layout coordinates"
    )]
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(LogicalSize::new(self.initial_size[0], self.initial_size[1]))
            .with_min_inner_size(LogicalSize::new(640.0, 360.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(
                    event_loop,
                    format!("could not create native runtime window: {error}"),
                );
                return;
            }
        };
        let size = window.inner_size();
        if size.width > 0 && size.height > 0 {
            self.mesh.model_view_projection = frame_fit(size.width as f32 / size.height as f32);
        }
        let renderer = match SurfaceRenderer::new(
            Arc::clone(&window),
            BackendRequest::Auto,
            size.width,
            size.height,
            window.scale_factor(),
        ) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.fail(
                    event_loop,
                    format!("could not initialize native runtime surface: {error}"),
                );
                return;
            }
        };
        window.request_redraw();
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.fail(
                    event_loop,
                    "native runtime window was closed before the editor stopped play",
                );
            }
            WindowEvent::Resized(size) => {
                self.resize_surface(event_loop, size.width, size.height);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = self.window.clone() {
                    let size = window.inner_size();
                    self.resize_surface(event_loop, size.width, size.height);
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Ok(mut latest) = self.latest_frame.lock()
                    && let Some(frame) = latest.take()
                {
                    self.mesh.texture_width = frame.width;
                    self.mesh.texture_height = frame.height;
                    self.mesh.texture_rgba8 = frame.pixels;
                    for pixel in self.mesh.texture_rgba8.as_chunks_mut::<4>().0 {
                        pixel.swap(0, 2);
                    }
                }

                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                if let Err(error) = renderer.render(&self.mesh) {
                    self.fail(
                        event_loop,
                        format!("native runtime surface render failed: {error}"),
                    );
                }
            }
            WindowEvent::Occluded(false) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.load(Ordering::Acquire) {
            event_loop.exit();
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_millis(16),
        ));
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn frame_fit(aspect: f32) -> [f32; 16] {
    let game_aspect = 16.0 / 9.0;
    let scale = if aspect > game_aspect {
        glam::Vec3::new(game_aspect / aspect, 1.0, 1.0)
    } else {
        glam::Vec3::new(1.0, aspect / game_aspect, 1.0)
    };
    Mat4::from_scale(scale).to_cols_array()
}

fn frame_quad() -> TexturedMesh {
    TexturedMesh {
        vertices: vec![
            MeshVertex {
                position: [-1.0, -1.0, 0.0],
                uv: [0.0, 1.0],
            },
            MeshVertex {
                position: [1.0, -1.0, 0.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [1.0, 1.0, 0.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [-1.0, 1.0, 0.0],
                uv: [0.0, 0.0],
            },
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture_width: 1,
        texture_height: 1,
        texture_rgba8: vec![12, 16, 22, 255],
        model_view_projection: Mat4::IDENTITY.to_cols_array(),
    }
}

fn render_runtime(
    config: RuntimeServerConfig,
    latest: Option<Arc<Mutex<Option<engine_play::BgraFrame>>>>,
) -> Result<(), String> {
    let models = engine_assets::load_model_library(&config.snapshot_root)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(id, (_, model))| (id, model))
        .collect();
    let game_ui = game_ui::GameUi::load(&config.snapshot_root)?;
    let mut renderer = renderer_wgpu::SceneViewportRenderer::new(BackendRequest::Auto)
        .map_err(|e| e.to_string())?;
    run_runtime_server_with_renderer(config, move |world, tick| {
        let scene = renderer_wgpu::game_scene_with_models(world, 16.0 / 9.0, &models);
        let frame = renderer
            .render(640, 360, &scene)
            .map_err(|e| e.to_string())?
            .ok_or("empty game frame")?;
        let mut pixels = frame.rgba8;
        game_ui.composite_rgba(640, 360, &mut pixels)?;
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let frame = engine_play::BgraFrame {
            sequence: tick,
            width: 640,
            height: 360,
            stride_bytes: 640 * 4,
            pixels,
        };
        if let Some(latest) = &latest {
            *latest.lock().map_err(|_| "frame lock poisoned")? = Some(frame.clone());
        }
        Ok(frame)
    })
    .map_err(|e| e.to_string())
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload.downcast_ref::<&str>().map_or_else(
        || {
            payload
                .downcast_ref::<String>()
                .map_or("runtime panicked", String::as_str)
        },
        |message| *message,
    )
}

fn required_value<'a>(arguments: &'a [OsString], name: &str) -> Result<&'a OsStr, String> {
    option_value(arguments, name).ok_or_else(|| format!("missing required {name}"))
}

fn required_utf8<'a>(arguments: &'a [OsString], name: &str) -> Result<&'a str, String> {
    required_value(arguments, name)?
        .to_str()
        .ok_or_else(|| format!("{name} is not valid UTF-8"))
}

fn option_value<'a>(arguments: &'a [OsString], name: &str) -> Option<&'a OsStr> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_os_str())
}

fn usage() -> String {
    "Usage: rustic-runtime --ipc-server --ipc-kind <namespaced|filesystem> \
     --ipc-address <name> --snapshot <path> --logs <path> \
     --mode <play|new-window|standalone> --fixed-step-nanos <n>"
        .to_owned()
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
mod tests {
    use super::*;

    #[test]
    fn native_argument_parser_preserves_paths() {
        let arguments = vec![
            OsString::from("--snapshot"),
            OsString::from(r"C:\Projects\My Game\temp\play"),
        ];
        assert_eq!(
            option_value(&arguments, "--snapshot"),
            Some(OsStr::new(r"C:\Projects\My Game\temp\play"))
        );
    }

    #[test]
    fn every_play_mode_has_a_stable_command_name() {
        for (text, mode) in [
            ("play", PlayMode::Play),
            ("new-window", PlayMode::NewWindow),
            ("standalone", PlayMode::Standalone),
        ] {
            assert_eq!(text.parse::<PlayMode>().unwrap(), mode);
        }
    }

    #[test]
    fn external_play_modes_require_native_windows_unless_explicitly_suppressed() {
        assert!(!requires_native_window(PlayMode::Play, false));
        assert!(requires_native_window(PlayMode::NewWindow, false));
        assert!(requires_native_window(PlayMode::Standalone, false));
        assert!(!requires_native_window(PlayMode::NewWindow, true));
        assert!(!requires_native_window(PlayMode::Standalone, true));
    }

    #[test]
    fn native_frame_preserves_game_aspect_ratio() {
        let mesh = frame_quad();
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert_eq!(frame_fit(16.0 / 9.0), Mat4::IDENTITY.to_cols_array());
        assert_ne!(frame_fit(1.0), frame_fit(2.0));
    }
}
