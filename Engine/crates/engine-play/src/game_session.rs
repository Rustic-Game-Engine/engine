//! Single-threaded game execution shared by browser and embedded runtimes.
use crate::PlaySnapshot;
use crate::pointer_input::PointerEvent;
use crate::script_runtime::RuntimeScripts;
use engine_world::SceneWorld;
use std::path::Path;

pub struct GameSession {
    scripts: RuntimeScripts,
    accumulator: f64,
}
impl GameSession {
    /// Load verified packaged data and initialize the same gameplay callbacks as local play.
    /// # Errors
    /// Returns snapshot, scene, settings or script initialization errors.
    pub fn load(root: &Path) -> Result<Self, String> {
        let snapshot = PlaySnapshot::open(root).map_err(|e| e.to_string())?;
        let source = snapshot.scene_bytes().map_err(|e| e.to_string())?;
        Ok(Self {
            scripts: RuntimeScripts::load(root, &source)?,
            accumulator: 0.0,
        })
    }
    /// Advance bounded real time without launching threads or child processes.
    /// # Errors
    /// Rejects non-finite or negative frame intervals.
    pub fn step(
        &mut self,
        seconds: f64,
        keys: Vec<String>,
        pointers: Vec<PointerEvent>,
        reset: bool,
    ) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid frame interval".into());
        }
        self.scripts.set_input_keys(keys);
        if reset {
            self.scripts.reset_pointer_input();
        }
        self.scripts.pointer_input(pointers);
        let seconds = seconds.min(0.25);
        self.accumulator += seconds;
        while self.accumulator >= 1.0 / 60.0 {
            self.scripts.fixed_update(1.0 / 60.0);
            self.accumulator -= 1.0 / 60.0;
        }
        self.scripts.frame_update(seconds);
        Ok(())
    }
    /// Extract an immutable view of the live game after script and physics updates.
    /// # Errors
    /// Returns an error if the scene state cannot be locked.
    pub fn render_world<T>(&self, render: impl FnOnce(&SceneWorld) -> T) -> Result<T, String> {
        self.scripts.render_world(render)
    }
    #[cfg(target_os = "emscripten")]
    pub fn take_audio(&self) -> Vec<f32> {
        crate::audio_output::take_browser_audio()
    }

    pub fn drain_logs(&self) -> Vec<(String, String)> {
        self.scripts.drain_logs()
    }
}
impl Drop for GameSession {
    fn drop(&mut self) {
        self.scripts.stop();
    }
}
