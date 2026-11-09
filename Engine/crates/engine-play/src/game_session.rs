//! Single-threaded game execution shared by browser and embedded runtimes.
use crate::PlaySnapshot;
use crate::pointer_input::PointerEvent;
use crate::script_runtime::RuntimeScripts;
use engine_world::SceneWorld;
use std::path::Path;

pub struct GameSession {
    scripts: RuntimeScripts,
    accumulator: f64,
    stream_audio: Option<crate::audio_output::StreamAudio>,
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
            stream_audio: None,
        })
    }
    /// Load a native session whose mixed audio is returned to a website player.
    /// # Errors
    /// Returns the same validation and initialization errors as `load`.
    pub fn load_streamed(root: &Path) -> Result<Self, String> {
        let audio = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = crate::audio_output::with_stream_audio(&audio, || Self::load(root))?;
        session.stream_audio = Some(audio);
        Ok(session)
    }

    /// Drain bounded interleaved stereo 48 kHz samples for a hosted player.
    pub fn take_stream_audio(&self) -> Vec<f32> {
        self.stream_audio
            .as_ref()
            .and_then(|sink| {
                sink.lock()
                    .ok()
                    .map(|mut queue| std::mem::take(&mut *queue))
            })
            .unwrap_or_default()
    }

    /// Advance bounded real time using the configured gameplay language adapters.
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
        if let Some(audio) = self.stream_audio.clone() {
            crate::audio_output::with_stream_audio(&audio, || {
                self.step_inner(seconds, keys, pointers, reset);
            });
        } else {
            self.step_inner(seconds, keys, pointers, reset);
        }
        Ok(())
    }
    fn step_inner(
        &mut self,
        seconds: f64,
        keys: Vec<String>,
        pointers: Vec<PointerEvent>,
        reset: bool,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlayMode, SnapshotBuilder, SnapshotInput};
    use engine_scripting::{
        ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    };
    use engine_world::{EntitySnapshot, SceneDocument, ScriptComponent};

    #[test]
    fn streamed_sessions_execute_available_native_language_adapters() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54, "scripts/test.lua", br"return {Start=function() rustic.set_translation(1,2,3) end}"),
            (ScriptLanguage::Luau, "scripts/test.luau", br"return {Start=function() rustic.set_translation(1,2,3) end}"),
            (ScriptLanguage::JavaScript, "scripts/test.js", br"globalThis.behavior={Start(){rustic.set_translation(1,2,3);}};"),
            (ScriptLanguage::Web, "ui/test.html", br"<!doctype html><html><head><title>Test</title></head><body><script>globalThis.behavior={Start(){rustic.set_translation(1,2,3);}};</script></body></html>"),
            (ScriptLanguage::Python, "scripts/test.py", b"from rustic import rustic,run\ndef on_start():\n    rustic.set_translation(1,2,3)\nrun(globals())\n"),
            (ScriptLanguage::C, "scripts/test.c", br#"#include "rustic.h"
void start(void){rustic.set_translation(1,2,3);} int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::Cpp, "scripts/test.cpp", br#"#include "rustic.hpp"
void start(){rustic.set_translation(1,2,3);} int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::CSharp, "scripts/test.cs", br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start")rustic.set_translation(1,2,3);});"#),
            (ScriptLanguage::Java, "scripts/RusticBehavior.java", br#"class RusticBehavior extends Rustic{public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start"))rustic.set_translation(1,2,3);});}}"#),
            (ScriptLanguage::Php, "ui/test.php", br"<?php
require_once __DIR__.'/rustic.php';
rustic_run(['on_start'=>function(){global $rustic;$rustic->set_translation(1,2,3);}]);"),
        ];
        for &(language, path, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(language).available {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_API_ADAPTERS").is_none(),
                    "{} toolchain is required",
                    language.display_name()
                );
                eprintln!(
                    "SKIPPED hosted session {}: missing toolchain",
                    language.display_name()
                );
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            let script = ScriptId::new();
            let entity = EntitySnapshot {
                scripts: vec![ScriptComponent::new(script)],
                ..EntitySnapshot::default()
            };
            let id = entity.id;
            let mut scene = SceneDocument::new("Hosted language test");
            scene.entities.push(entity);
            let manifest = ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script,
                    language,
                    relative_path: path.into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: vec![],
                }],
            };
            let snapshot = SnapshotBuilder::new(temp.path().join("stage"))
                .stage(
                    PlayMode::Standalone,
                    SnapshotInput::new("scene/main.scene", scene.to_bytes().unwrap()),
                    &[
                        SnapshotInput::new(
                            "config/scripts.ron",
                            ron::ser::to_string(&manifest).unwrap().into_bytes(),
                        ),
                        SnapshotInput::new(path, source),
                    ],
                )
                .unwrap();
            let mut session = GameSession::load_streamed(snapshot.root()).unwrap();
            session.step(1.0 / 60.0, vec![], vec![], false).unwrap();
            let position = session
                .render_world(|world| world.local_transform(id).unwrap().translation)
                .unwrap();
            assert_eq!(
                position,
                glam::Vec3::new(1.0, 2.0, 3.0),
                "{}: {:?}",
                language.display_name(),
                session.drain_logs()
            );
        }
    }
}
