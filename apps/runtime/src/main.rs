use engine_core::{
    ApplicationIdentity, ApplicationRole, headless_smoke_requested, run_headless_smoke,
};
use engine_play::{
    AuthenticationToken, LocalEndpoint, PlayMode, RuntimeServerConfig, record_runtime_crash,
    run_runtime_server,
};
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
        run_runtime_server(config).map_err(|error| error.to_string())
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
    let server = thread::Builder::new()
        .name("rustic-runtime-server".to_owned())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_runtime_server(config).map_err(|error| error.to_string())
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
    let mut window_app = RuntimeWindowApp::new(mode, Arc::clone(&finished), title, size);
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
    mode: PlayMode,
    finished: Arc<AtomicBool>,
    title: &'static str,
    initial_size: [f64; 2],
    window: Option<Arc<Window>>,
    renderer: Option<SurfaceRenderer>,
    mesh: TexturedMesh,
    error: Option<String>,
}

impl RuntimeWindowApp {
    fn new(
        mode: PlayMode,
        finished: Arc<AtomicBool>,
        title: &'static str,
        initial_size: [f64; 2],
    ) -> Self {
        Self {
            mode,
            finished,
            title,
            initial_size,
            window: None,
            renderer: None,
            mesh: runtime_mesh(mode),
            error: None,
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: impl Into<String>) {
        self.error = Some(message.into());
        event_loop.exit();
    }

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
        }
    }
}

impl ApplicationHandler for RuntimeWindowApp {
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
        eprintln!(
            "native runtime surface: mode={} adapter={} backend={:?}",
            self.mode.as_str(),
            renderer.diagnostics().adapter_name,
            renderer.diagnostics().backend
        );
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

fn runtime_mesh(mode: PlayMode) -> TexturedMesh {
    let mut mesh = TexturedMesh::checkerboard_quad();
    let (bright, dark) = match mode {
        PlayMode::Play => ([70, 176, 94, 255], [30, 96, 52, 255]),
        PlayMode::NewWindow => ([208, 132, 64, 255], [118, 62, 26, 255]),
        PlayMode::Standalone => ([150, 92, 208, 255], [72, 38, 118, 255]),
    };
    mesh.texture_rgba8 = [bright, dark, dark, bright].concat();
    mesh
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
}
