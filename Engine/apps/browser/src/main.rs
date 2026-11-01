use engine_play::game_session::GameSession;
use engine_play::pointer_input::PointerEvent;
use renderer_wgpu::{BackendRequest, SceneViewportRenderer};
use serde::Deserialize;
use std::cell::RefCell;
use std::path::Path;
#[path = "../../runtime/src/game_ui.rs"]
mod game_ui;

struct BrowserGame {
    session: GameSession,
    renderer: SceneViewportRenderer,
    models: std::collections::BTreeMap<engine_core::AssetId, engine_assets::ModelArtifact>,
    ui: game_ui::GameUi,
}
thread_local! { static GAME: RefCell<Option<BrowserGame>> = const { RefCell::new(None) }; }

#[derive(Default, Deserialize)]
#[serde(default)]
struct Input {
    keys: Vec<String>,
    pointers: Vec<PointerEvent>,
    reset: bool,
}

fn initialize() -> Result<(), String> {
    let root = Path::new("/game");
    let session = GameSession::load(root)?;
    let models = engine_assets::load_model_library(root)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(id, (_, model))| (id, model))
        .collect();
    let mut renderer =
        SceneViewportRenderer::new(BackendRequest::OpenGl).map_err(|e| e.to_string())?;
    renderer.set_environment_root(root);
    let ui = game_ui::GameUi::load(root)?;
    GAME.with(|slot| {
        *slot.borrow_mut() = Some(BrowserGame {
            session,
            renderer,
            models,
            ui,
        })
    });
    Ok(())
}
fn main() {
    if let Err(error) = initialize() {
        report_error(&error);
    }
}
fn report_error(error: &str) {
    eprintln!("{error}");
    let _ = std::fs::write("/error.txt", error);
}

/// Emscripten calls this synchronous, value-only entry point on the browser thread.
#[allow(
    unsafe_code,
    reason = "stable export name for a function taking only bounded scalar values"
)]
#[unsafe(no_mangle)]
pub extern "C" fn rustic_tick(width: u32, height: u32, seconds: f64) {
    let result = GAME.with(|slot| {
        let mut slot = slot.borrow_mut();
        let game = slot.as_mut().ok_or("Game did not initialize")?;
        let input: Input =
            serde_json::from_slice(&std::fs::read("/input.json").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        game.session
            .step(seconds, input.keys, input.pointers, input.reset)?;
        let [width, height] = engine_play::game_view_extent([width, height]);
        let scene = game.session.render_world(|world| {
            renderer_wgpu::game_scene_with_models(world, 16.0 / 9.0, &game.models)
        })?;
        #[cfg(target_os = "emscripten")]
        {
            let audio: Vec<u8> = game
                .session
                .take_audio()
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            std::fs::write("/audio.f32", audio).map_err(|e| e.to_string())?;
        }
        for (level, message) in game.session.drain_logs() {
            eprintln!("{level}: {message}");
        }
        let Some(mut frame) = game
            .renderer
            .render(width, height, &scene)
            .map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        game.ui.composite_rgba(width, height, &mut frame.rgba8)?;
        std::fs::write("/frame.rgba", frame.rgba8).map_err(|e| e.to_string())?;
        std::fs::write(
            "/frame.json",
            serde_json::to_vec(&[width, height]).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    });
    if let Err(error) = result {
        report_error(&error);
    }
}
