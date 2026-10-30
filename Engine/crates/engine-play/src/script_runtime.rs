#[path = "scene_lifecycle.rs"]
mod scenes;
use crate::{
    LiveEntityProperties, RuntimeChange, RuntimeChangeSet, RuntimeChangeTarget, RuntimeValue,
};
use engine_core::gameplay::{Delivery, GameplayRuntime, OperationState, Owner, Request};
use engine_scripting::{
    ActionState, EngineValue, ExternalBehavior, GameSettings, GameplayHost, InputFrame,
    JavaScriptBehavior, LuaBehavior, ScriptId, ScriptLanguage, ScriptReference, WebBehavior,
    load_manifest,
};
use engine_world::{EntityId, Mesh, SceneWorld, WorldCommand, load_scene};
pub(crate) use scenes::RuntimeScripts;
use scenes::{LoadedScene, SceneIo, SceneRegistry};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_SCRIPT_LOGS: usize = 1_024;

enum RuntimeBehavior {
    Lua(LuaBehavior),
    JavaScript(JavaScriptBehavior),
    External(ExternalBehavior),
    Web(WebBehavior),
}

impl RuntimeBehavior {
    fn load(
        language: ScriptLanguage,
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
    ) -> Result<Self, String> {
        match language {
            ScriptLanguage::Lua54 => {
                LuaBehavior::load(script_id, source, source_name, host, 200_000)
                    .map(Self::Lua)
                    .map_err(|error| error.to_string())
            }
            ScriptLanguage::JavaScript => {
                JavaScriptBehavior::load(script_id, source, source_name, host, 200_000)
                    .map(Self::JavaScript)
                    .map_err(|error| error.to_string())
            }
            ScriptLanguage::Web => WebBehavior::load(script_id, source, source_name, host, 200_000)
                .map(Self::Web)
                .map_err(|error| error.to_string()),
            other => ExternalBehavior::load(script_id, other, source, source_name, host)
                .map(Self::External)
                .map_err(|error| error.to_string()),
        }
    }

    fn host(&self) -> Arc<Mutex<Box<dyn GameplayHost>>> {
        match self {
            Self::Lua(v) => v.host(),
            Self::JavaScript(v) => v.host(),
            Self::External(v) => v.host(),
            Self::Web(v) => v.host(),
        }
    }
    fn replacement(&self, source: &[u8]) -> Result<Self, String> {
        let original = self.host();
        let host = original
            .lock()
            .map_err(|_| "gameplay host lock poisoned")?
            .fork_for_reload()
            .map_or_else(|| Arc::clone(&original), |host| Arc::new(Mutex::new(host)));
        let result = match self {
            Self::Lua(old) => LuaBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.lua",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::Lua)
            .map_err(|e| e.to_string()),
            Self::JavaScript(old) => JavaScriptBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.js",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::JavaScript)
            .map_err(|e| e.to_string()),
            Self::External(old) => ExternalBehavior::load_shared(
                old.script_id(),
                old.language(),
                source,
                "hot-reload.source",
                Arc::clone(&host),
            )
            .map(Self::External)
            .map_err(|e| e.to_string()),
            Self::Web(old) => WebBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.html",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::Web)
            .map_err(|e| e.to_string()),
        };
        if result.is_err()
            && !Arc::ptr_eq(&host, &original)
            && let Ok(mut host) = host.lock()
        {
            host.cleanup_gameplay();
        }
        result
    }

    fn script_id(&self) -> ScriptId {
        match self {
            Self::Lua(value) => value.script_id(),
            Self::JavaScript(value) => value.script_id(),
            Self::External(value) => value.script_id(),
            Self::Web(value) => value.script_id(),
        }
    }

    fn cleanup_gameplay(&mut self) {
        let host = match self {
            Self::Lua(v) => v.host(),
            Self::JavaScript(v) => v.host(),
            Self::External(v) => v.host(),
            Self::Web(v) => v.host(),
        };
        if let Ok(mut host) = host.lock() {
            host.cleanup_gameplay();
        }
    }
    fn on_create(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_create().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_create().map_err(|error| error.to_string()),
            Self::External(value) => value.on_create().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_create().map_err(|error| error.to_string()),
        }
    }

    fn on_start(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_start().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_start().map_err(|error| error.to_string()),
            Self::External(value) => value.on_start().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_start().map_err(|error| error.to_string()),
        }
    }

    fn on_enable(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::External(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_enable().map_err(|error| error.to_string()),
        }
    }

    fn on_disable(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::External(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_disable().map_err(|error| error.to_string()),
        }
    }

    fn active(&self) -> bool {
        let (runtime_enabled, host) = match self {
            Self::Lua(value) => (value.enabled(), value.host()),
            Self::JavaScript(value) => (value.enabled(), value.host()),
            Self::External(value) => (value.enabled(), value.host()),
            Self::Web(value) => (value.enabled(), value.host()),
        };
        runtime_enabled && host.lock().is_ok_and(|host| host.enabled())
    }

    fn fixed_update(&mut self, delta: f64) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::External(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::Web(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
        }
    }

    fn update(&mut self, delta: f64) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::External(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::Web(value) => value.update(delta).map_err(|error| error.to_string()),
        }
    }

    fn on_destroy(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::External(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_destroy().map_err(|error| error.to_string()),
        }
    }

    fn on_stop(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::External(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_stop().map_err(|error| error.to_string()),
        }
    }
}

struct GameplayContext {
    pointer_input: crate::pointer_input::PointerInput,
    scene: engine_core::SceneId,
    runtime: GameplayRuntime,
    physics: crate::physics::PhysicsWorld,
    audio: engine_core::gameplay::audio::Mixer,
    audio_output: crate::audio_output::AudioOutput,
    audio_capture: crate::audio_capture::AudioCapture,
    clips: BTreeMap<(EntityId, String), engine_core::gameplay::animation::Clip>,
    frame_delta: f64,
    fixed_delta: f64,
}

impl Default for GameplayContext {
    fn default() -> Self {
        Self {
            pointer_input: crate::pointer_input::PointerInput::default(),
            scene: engine_core::SceneId::new(),
            runtime: GameplayRuntime::default(),
            physics: crate::physics::PhysicsWorld::default(),
            audio: engine_core::gameplay::audio::Mixer::default(),
            audio_output: crate::audio_output::AudioOutput::new(),
            audio_capture: crate::audio_capture::AudioCapture::default(),
            clips: BTreeMap::new(),
            frame_delta: 1.0 / 60.0,
            fixed_delta: 1.0 / 60.0,
        }
    }
}

struct SceneRuntime {
    gameplay: Arc<Mutex<GameplayContext>>,
    behaviors: Vec<RuntimeBehavior>,
    world: Arc<Mutex<SceneWorld>>,
    input_keys: Arc<Mutex<BTreeSet<String>>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    valid_scene: bool,
    name: String,
    global_behaviors: Vec<RuntimeBehavior>,
}

impl SceneRuntime {
    pub fn live_entity(&self, id: EntityId) -> Result<Option<LiveEntityProperties>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        if !world.contains(id) {
            return Ok(None);
        }
        let snapshot = world.snapshot(id).map_err(|error| error.to_string())?;
        drop(world);
        let mut scripts = snapshot.scripts;
        for behavior in &self.behaviors {
            let host = match behavior {
                RuntimeBehavior::Lua(value) => value.host(),
                RuntimeBehavior::JavaScript(value) => value.host(),
                RuntimeBehavior::External(value) => value.host(),
                RuntimeBehavior::Web(value) => value.host(),
            };
            if let Ok(host) = host.lock()
                && host.entity_id() == id
                && let Some(script) = scripts
                    .iter_mut()
                    .find(|script| script.script_id == behavior.script_id())
            {
                script.properties = host.properties();
                script.enabled = host.enabled();
            }
        }
        Ok(Some(LiveEntityProperties {
            entity_id: id.to_string(),
            name: snapshot.name,
            translation: snapshot.local_transform.translation.to_array(),
            rotation: snapshot.local_transform.rotation.to_array(),
            scale: snapshot.local_transform.scale.to_array(),
            part_attributes: snapshot.part_attributes,
            scripts,
        }))
    }
    #[allow(clippy::too_many_lines)]
    fn load(
        snapshot_root: &Path,
        loaded: &LoadedScene,
        scenes: &Arc<Mutex<SceneRegistry>>,
        io: &SceneIo,
        include_globals: bool,
    ) -> Result<Self, String> {
        let manifest_path = snapshot_root.join("config/scripts.ron");
        let valid_scene = loaded.valid;
        let scene_name = loaded.name.clone();
        let mut scene_startup_scripts = loaded.startup_scripts.clone();
        sort_script_references(&mut scene_startup_scripts);
        let world = Arc::clone(&loaded.world);
        let gameplay = Arc::clone(&loaded.gameplay);
        let SceneIo {
            logs,
            dropped_logs,
            input_keys,
        } = io.clone();
        let model_library =
            engine_assets::load_model_library(snapshot_root).map_err(|e| e.to_string())?;
        {
            let mut world = world.lock().map_err(|_| "world lock poisoned")?;
            let mut context = gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let entities = world.entity_ids().collect::<Vec<_>>();
            for entity in entities {
                let Some(mesh) = world.mesh(entity).map_err(|e| e.to_string())? else {
                    continue;
                };
                let Some((_, model)) = model_library.get(&mesh.asset) else {
                    continue;
                };
                bind_model(&mut world, &mut context, entity, model)?;
            }
        }
        let entries = if manifest_path.is_file() {
            load_manifest(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
                .manifest
                .scripts
        } else {
            Vec::new()
        };
        let entries: BTreeMap<_, _> = entries.into_iter().map(|entry| (entry.id, entry)).collect();
        let mut snapshots = {
            let guard = world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?;
            guard
                .entity_ids()
                .map(|id| guard.snapshot(id).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?
        };
        snapshots.sort_by_key(|snapshot| snapshot.id);
        let mut behaviors = Vec::new();
        let settings_path = snapshot_root.join(engine_scripting::GAME_SETTINGS_FILE);
        let settings = if settings_path.is_file() {
            // Never guess an entry point from manifest ordering. In particular, UUID-keyed
            // manifest maps do not preserve the script order shown by the editor.
            Some(GameSettings::load(snapshot_root)?)
        } else {
            None
        };
        let mut global_behaviors = Vec::new();
        if let Some(settings) = &settings
            && include_globals
        {
            let mut global_scripts = settings.global_scripts.clone();
            sort_script_references(&mut global_scripts);
            load_startup_references(
                &global_scripts,
                &scene_name,
                &entries,
                snapshot_root,
                &world,
                &input_keys,
                &logs,
                &dropped_logs,
                &gameplay,
                &mut global_behaviors,
                scenes,
                true,
            )?;
        }
        load_startup_references(
            &scene_startup_scripts,
            &scene_name,
            &entries,
            snapshot_root,
            &world,
            &input_keys,
            &logs,
            &dropped_logs,
            &gameplay,
            &mut behaviors,
            scenes,
            false,
        )?;
        for entity in snapshots {
            for component in entity.scripts {
                if !component.enabled {
                    continue;
                }
                let Some(entry) = entries.get(&component.script_id) else {
                    push_log(
                        &logs,
                        &dropped_logs,
                        "warn",
                        &format!(
                            "entity {} references missing script {}; skipped for this play session",
                            entity.id, component.script_id
                        ),
                    );
                    continue;
                };
                let Some(source) = read_script_source(snapshot_root, entry, &logs, &dropped_logs)?
                else {
                    continue;
                };
                let host = RuntimeHost {
                    gameplay: Arc::clone(&gameplay),
                    owner_script: ScriptId::new(),
                    entity_bound: true,
                    scene_name: scene_name.clone(),
                    world: Arc::clone(&world),
                    input_keys: Arc::clone(&input_keys),
                    snapshot_root: snapshot_root.to_path_buf(),
                    entity: entity.id,
                    properties: Arc::new(Mutex::new(component.properties)),
                    logs: Arc::clone(&logs),
                    dropped_logs: Arc::clone(&dropped_logs),
                    enabled: true,
                    scenes: Arc::clone(scenes),
                    global: false,
                };
                let mut behavior = RuntimeBehavior::load(
                    entry.language,
                    component.script_id,
                    &source,
                    &entry.relative_path.to_string_lossy(),
                    Box::new(host),
                )?;
                initialize_behavior(&mut behavior, &logs, &dropped_logs);
                behaviors.push(behavior);
            }
        }
        Ok(Self {
            gameplay,
            behaviors,
            world,
            input_keys,
            logs,
            dropped_logs,
            valid_scene,
            name: scene_name,
            global_behaviors,
        })
    }

    pub fn render_world<T>(
        &self,
        render: impl FnOnce(&engine_world::SceneWorld) -> T,
    ) -> Result<T, String> {
        let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
        world.propagate_transforms();
        Ok(render(&world))
    }
    pub fn set_input_keys(&mut self, keys: Vec<String>) {
        if let Ok(mut held) = self.input_keys.lock() {
            *held = keys
                .into_iter()
                .take(128)
                .filter(|key| key.len() <= 64)
                .collect();
        }
    }
    pub fn fixed_update(&mut self, delta: f64) {
        if let Ok(mut context) = self.gameplay.lock() {
            context.fixed_delta = delta;
        }
        self.unregister_disabled();
        for behavior in &mut self.behaviors {
            if !behavior.active() {
                continue;
            }
            if let Err(error) = behavior.fixed_update(delta) {
                push_log(
                    &self.logs,
                    &self.dropped_logs,
                    "error",
                    &format!(
                        "script {} fixed_update failed and was disabled: {error}",
                        behavior.script_id()
                    ),
                );
            }
        }
        self.unregister_disabled();
        if let Ok(mut world) = self.world.lock()
            && let Err(error) = self
                .gameplay
                .lock()
                .map_err(|_| "gameplay lock poisoned".to_owned())
                .and_then(|mut context| {
                    let scaled = context.runtime.scaled_delta(delta)?;
                    world
                        .tick_entity_lifetimes(scaled)
                        .map_err(|e| e.to_string())?;
                    context
                        .physics
                        .step(&mut world, scaled)
                        .map_err(|e| e.to_string())
                })
        {
            push_log(
                &self.logs,
                &self.dropped_logs,
                "error",
                &format!("physics step failed: {error}"),
            );
        }
    }
    pub fn frame_update(&mut self, delta: f64) {
        if let Ok(mut context) = self.gameplay.lock() {
            context.frame_delta = delta;
        }
        if let (Ok(mut context), Ok(mut world)) = (self.gameplay.lock(), self.world.lock()) {
            let GameplayContext {
                pointer_input,
                runtime,
                ..
            } = &mut *context;
            pointer_input.begin_frame(&mut world, &mut runtime.signals);
        }
        // Tick once for the scene, independent of how many behaviors are attached.
        if let (Ok(mut gameplay), Ok(mut world)) = (self.gameplay.lock(), self.world.lock()) {
            gameplay
                .audio
                .cleanup(|owner| owner.entity_bound && !world.contains(owner.entity));
            gameplay
                .audio_capture
                .cleanup(|owner| owner.entity_bound && !world.contains(owner.entity));
            gameplay
                .clips
                .retain(|(entity, _), _| world.contains(*entity));
            world.propagate_transforms();
            if let Some(camera) = world
                .entity_ids()
                .filter(|id| world.camera(*id).ok().flatten().is_some_and(|c| c.active))
                .last()
                && let Ok(transform) = world.world_transform(camera)
            {
                gameplay.audio.listener = transform.0.w_axis.truncate().as_dvec3().to_array();
            }
            let GameplayContext { runtime, audio, .. } = &mut *gameplay;
            if let Err(error) = runtime.tick(
                delta,
                &mut crate::gameplay_world::GameplayScene(&mut world, audio),
            ) {
                push_log(&self.logs, &self.dropped_logs, "error", &error);
            }
        }
        if let Ok(mut context) = self.gameplay.lock() {
            let scale = if context.runtime.paused {
                0.0
            } else {
                context.runtime.time_scale()
            };
            match context.audio.mix_scaled(delta, scale) {
                Ok(samples) => {
                    context.audio_capture.submit(&samples);
                    context.audio_output.submit(samples);
                }
                Err(error) => push_log(&self.logs, &self.dropped_logs, "error", &error),
            }
        }
        self.unregister_disabled();
        for behavior in &mut self.behaviors {
            if !behavior.active() {
                continue;
            }
            if let Err(error) = behavior.update(delta) {
                push_log(
                    &self.logs,
                    &self.dropped_logs,
                    "error",
                    &format!(
                        "script {} update failed and was disabled: {error}",
                        behavior.script_id()
                    ),
                );
            }
        }
        self.unregister_disabled();
    }
    pub fn stop(&mut self) {
        for behavior in &mut self.behaviors {
            let _ = behavior.on_disable();
            let _ = behavior.on_destroy();
            let _ = behavior.on_stop();
            behavior.cleanup_gameplay();
        }
    }
    fn unregister_disabled(&mut self) {
        let mut active = Vec::with_capacity(self.behaviors.len());
        for mut behavior in self.behaviors.drain(..) {
            if behavior.active() {
                active.push(behavior);
            } else {
                let _ = behavior.on_disable();
                let host = behavior.host();
                if host.lock().is_ok_and(|host| host.entity_lifetime_ended()) {
                    let _ = behavior.on_destroy();
                }
                behavior.cleanup_gameplay();
            }
        }
        self.behaviors = active;
    }
    pub fn drain_logs(&self) -> Vec<(String, String)> {
        self.logs
            .lock()
            .map_or_else(|_| Vec::new(), |mut logs| logs.drain(..).collect())
    }
    pub fn runtime_changes(
        &self,
        base_hash: &str,
        original_scene: &[u8],
    ) -> Result<RuntimeChangeSet, String> {
        if !self.valid_scene {
            return Ok(RuntimeChangeSet::new(base_hash, Vec::new()));
        }
        let original = load_scene(original_scene)
            .map_err(|error| error.to_string())?
            .document;
        let before: BTreeMap<_, _> = original
            .entities
            .into_iter()
            .map(|entity| {
                (
                    entity.id,
                    (
                        entity.parent,
                        entity.local_transform.translation.to_array().map(f64::from),
                    ),
                )
            })
            .collect();
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        let mut changes = Vec::new();
        for (entity, (original_parent, original)) in before {
            // Structural edits are runtime-only: the review/apply format can
            // represent translations, but not deletions or changed parents.
            // Never dereference a deleted original, or apply a rewritten local
            // translation while the editor retains the authored parent.
            if !world.contains(entity)
                || world.parent(entity).map_err(|error| error.to_string())? != original_parent
            {
                continue;
            }
            let current = world
                .local_transform(entity)
                .map_err(|error| error.to_string())?
                .translation
                .to_array()
                .map(f64::from);
            if current
                .iter()
                .zip(original.iter())
                .any(|(left, right)| (left - right).abs() > f64::EPSILON)
            {
                changes.push(RuntimeChange {
                    target: RuntimeChangeTarget {
                        entity_id: entity.to_string(),
                        component_schema: "transform.v1".into(),
                        property: "translation".into(),
                    },
                    before: RuntimeValue::Vector3(original),
                    after: RuntimeValue::Vector3(current),
                });
            }
        }
        Ok(RuntimeChangeSet::new(base_hash, changes))
    }
}

fn reload_behaviors<'a>(
    behaviors: impl Iterator<Item = &'a mut RuntimeBehavior>,
    script_id: ScriptId,
    source: &[u8],
) -> Result<usize, String> {
    let mut instances: Vec<_> = behaviors
        .filter(|behavior| behavior.script_id() == script_id)
        .collect();
    if instances.is_empty() {
        return Err(format!("script {script_id} has no running instances"));
    }
    let mut replacements: Vec<RuntimeBehavior> = Vec::with_capacity(instances.len());
    for instance in &instances {
        let result = (|| {
            let mut replacement = instance.replacement(source)?;
            if let Err(error) = replacement
                .on_create()
                .and_then(|()| replacement.on_enable())
                .and_then(|()| replacement.on_start())
            {
                replacement.cleanup_gameplay();
                return Err(error);
            }
            Ok(replacement)
        })();
        match result {
            Ok(replacement) => replacements.push(replacement),
            Err(error) => {
                for replacement in &mut replacements {
                    replacement.cleanup_gameplay();
                }
                return Err(error);
            }
        }
    }
    let count = instances.len();
    for (instance, replacement) in instances.iter_mut().zip(replacements) {
        instance.cleanup_gameplay();
        **instance = replacement;
    }
    Ok(count)
}

fn sort_script_references(references: &mut [ScriptReference]) {
    // Stable sort preserves editor list order when explicit orders are equal.
    references.sort_by_key(|reference| reference.execution_order);
}

#[allow(
    clippy::too_many_arguments,
    reason = "startup loading shares the existing scene, input, logs, and gameplay state"
)]
fn load_startup_references(
    references: &[ScriptReference],
    scene_name: &str,
    entries: &BTreeMap<ScriptId, engine_scripting::ScriptManifestEntry>,
    snapshot_root: &Path,
    world: &Arc<Mutex<SceneWorld>>,
    input_keys: &Arc<Mutex<BTreeSet<String>>>,
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
    gameplay: &Arc<Mutex<GameplayContext>>,
    behaviors: &mut Vec<RuntimeBehavior>,
    scenes: &Arc<Mutex<SceneRegistry>>,
    global: bool,
) -> Result<(), String> {
    let entity = world
        .lock()
        .map_err(|_| "runtime world lock poisoned".to_owned())?
        .entity_ids()
        .next()
        .unwrap_or_else(EntityId::new);
    for reference in references.iter().filter(|reference| reference.enabled) {
        let Some(entry) = entries.get(&reference.asset_id) else {
            push_log(
                logs,
                dropped_logs,
                "warn",
                &format!(
                    "startup scope references missing script {}; skipped for this play session",
                    reference.asset_id
                ),
            );
            continue;
        };
        let Some(source) = read_script_source(snapshot_root, entry, logs, dropped_logs)? else {
            continue;
        };
        let host = RuntimeHost {
            gameplay: Arc::clone(gameplay),
            owner_script: ScriptId::new(),
            entity_bound: false,
            scene_name: scene_name.to_owned(),
            world: Arc::clone(world),
            input_keys: Arc::clone(input_keys),
            snapshot_root: snapshot_root.to_path_buf(),
            entity,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::clone(logs),
            dropped_logs: Arc::clone(dropped_logs),
            enabled: true,
            scenes: Arc::clone(scenes),
            global,
        };
        let mut behavior = RuntimeBehavior::load(
            entry.language,
            reference.asset_id,
            &source,
            &entry.relative_path.to_string_lossy(),
            Box::new(host),
        )?;
        initialize_behavior(&mut behavior, logs, dropped_logs);
        behaviors.push(behavior);
    }
    Ok(())
}

fn read_script_source(
    snapshot_root: &Path,
    entry: &engine_scripting::ScriptManifestEntry,
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
) -> Result<Option<Vec<u8>>, String> {
    let source_path = snapshot_root.join(&entry.relative_path);
    let source = match std::fs::read(&source_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            push_log(
                logs,
                dropped_logs,
                "warn",
                &format!(
                    "script {} source {} is missing; skipped for this play session",
                    entry.id,
                    entry.relative_path.display()
                ),
            );
            return Ok(None);
        }
        Err(error) => return Err(format!("could not read {}: {error}", source_path.display())),
    };
    if entry.language == ScriptLanguage::Web {
        resolve_linked_web_scripts(
            snapshot_root,
            &entry.relative_path,
            &source,
            logs,
            dropped_logs,
        )
        .map(Some)
    } else {
        Ok(Some(source))
    }
}

fn resolve_linked_web_scripts(
    snapshot_root: &Path,
    document_path: &Path,
    source: &[u8],
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(source).map_err(|error| error.to_string())?;
    let lower = text.to_ascii_lowercase();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(relative_open) = lower[cursor..].find("<script") {
        let open = cursor + relative_open;
        let tag_end = lower[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| "unterminated <script> tag".to_owned())?;
        let close = lower[tag_end + 1..]
            .find("</script>")
            .map(|offset| tag_end + 1 + offset)
            .ok_or_else(|| "missing </script> tag".to_owned())?;
        output.push_str(&text[cursor..=tag_end]);
        let tag = &text[open..=tag_end];
        if let Some(link) = script_src(tag) {
            let relative = document_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(link);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(format!(
                    "linked script path escapes the play snapshot: {}",
                    relative.display()
                ));
            }
            let linked_path = snapshot_root.join(&relative);
            match std::fs::read_to_string(&linked_path) {
                Ok(linked) => output.push_str(&linked),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    push_log(
                        logs,
                        dropped_logs,
                        "warn",
                        &format!(
                            "linked script {} is missing; skipped for this play session",
                            relative.display()
                        ),
                    );
                }
                Err(error) => {
                    return Err(format!(
                        "could not read linked script {}: {error}",
                        linked_path.display()
                    ));
                }
            }
        } else {
            output.push_str(&text[tag_end + 1..close]);
        }
        output.push_str(&text[close..close + "</script>".len()]);
        cursor = close + "</script>".len();
    }
    output.push_str(&text[cursor..]);
    Ok(output.into_bytes())
}

fn script_src(tag: &str) -> Option<&str> {
    let lower = tag.to_ascii_lowercase();
    let index = lower.find("src")? + 3;
    let rest = tag.get(index..)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &rest[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(&value[..end])
}

fn initialize_behavior(
    behavior: &mut RuntimeBehavior,
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
) {
    if let Err(error) = behavior.on_create() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_create failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
        return;
    }
    if let Err(error) = behavior.on_enable() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_enable failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
    } else if let Err(error) = behavior.on_start() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_start failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
    }
}

fn push_log(
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped: &Arc<Mutex<u64>>,
    level: &str,
    message: &str,
) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() >= MAX_SCRIPT_LOGS {
            logs.pop_front();
            if let Ok(mut count) = dropped.lock() {
                *count = count.saturating_add(1);
            }
        }
        logs.push_back((level.into(), message.chars().take(4096).collect()));
    }
}

#[derive(Clone)]
struct RuntimeHost {
    gameplay: Arc<Mutex<GameplayContext>>,
    owner_script: ScriptId,
    entity_bound: bool,
    scene_name: String,
    world: Arc<Mutex<SceneWorld>>,
    input_keys: Arc<Mutex<BTreeSet<String>>>,
    snapshot_root: PathBuf,
    entity: EntityId,
    properties: Arc<Mutex<BTreeMap<String, EngineValue>>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    enabled: bool,
    scenes: Arc<Mutex<SceneRegistry>>,
    global: bool,
}

impl GameplayHost for RuntimeHost {
    fn persists_across_scenes(&self) -> bool {
        self.global
    }
    fn refresh_scene_context(&mut self) {
        if !self.global {
            return;
        }
        if let Ok(registry) = self.scenes.lock() {
            if let Some(scene) = registry
                .active
                .as_ref()
                .and_then(|name| registry.loaded.get(name))
            {
                self.world = Arc::clone(&scene.world);
                self.gameplay = Arc::clone(&scene.gameplay);
                self.scene_name.clone_from(&scene.name);
                if let Ok(world) = self.world.lock() {
                    self.entity = world.entity_ids().next().unwrap_or_else(EntityId::new);
                }
            } else {
                self.world = Arc::clone(&registry.empty_world);
                self.gameplay = Arc::clone(&registry.empty_gameplay);
                self.scene_name.clear();
            }
        }
    }

    fn fork_for_reload(&self) -> Option<Box<dyn GameplayHost>> {
        let mut host = self.clone();
        host.owner_script = ScriptId::new();
        Some(Box::new(host))
    }
    #[allow(
        clippy::too_many_lines,
        reason = "keep exhaustive gameplay query dispatch in one match"
    )]
    fn gameplay_query(&mut self, query: serde_json::Value) -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct Args {
            #[serde(default)]
            entity: Option<EntityId>,
            #[serde(default)]
            origin: [f64; 3],
            #[serde(default)]
            direction: [f64; 3],
            #[serde(default)]
            center: [f64; 3],
            #[serde(default)]
            vector: [f64; 3],
            #[serde(default)]
            distance: f64,
            #[serde(default)]
            radius: f64,
            #[serde(default)]
            strength: f64,
            #[serde(default)]
            delta: f64,
            #[serde(default)]
            ignore: Vec<EntityId>,
        }
        let op = query
            .get("op")
            .and_then(serde_json::Value::as_str)
            .ok_or("query operation is missing")?;
        if op.starts_with("input_") {
            let context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            return match op {
                "input_touches" => serde_json::to_value(context.pointer_input.touches()),
                "input_touch_events" => serde_json::to_value(
                    context
                        .pointer_input
                        .events
                        .iter()
                        .filter(|e| e.device == crate::pointer_input::PointerDevice::Touch)
                        .collect::<Vec<_>>(),
                ),
                "input_clicks" => {
                    let entity = query
                        .get("entity")
                        .filter(|v| !v.is_null())
                        .map(|v| serde_json::from_value::<EntityId>(v.clone()))
                        .transpose()
                        .map_err(|e| e.to_string())?;
                    let world = self.world.lock().map_err(|_| "world lock poisoned")?;
                    serde_json::to_value(
                        context
                            .pointer_input
                            .clicks
                            .iter()
                            .filter(|c| {
                                entity.is_none_or(|id| id == c.entity)
                                    && world.contains(c.entity)
                                    && world.part_attributes(c.entity).is_ok_and(|p| p.can_touch)
                            })
                            .collect::<Vec<_>>(),
                    )
                }
                _ => return Err("unknown input query".into()),
            }
            .map_err(|e| e.to_string());
        }
        if matches!(
            op,
            "scene_load"
                | "scene_unload"
                | "scene_switch"
                | "scene_is_loaded"
                | "scene_loaded"
                | "scene_active"
        ) {
            return self
                .scenes
                .lock()
                .map_err(|_| "scene lock poisoned")?
                .query(&self.snapshot_root, &query);
        }
        if op == "scene_object" {
            let source = query
                .get("source")
                .and_then(serde_json::Value::as_str)
                .ok_or("scene object source is missing")?;
            return Ok(serde_json::Value::String(
                self.object_entity(source)?.to_string(),
            ));
        }
        if op == "find_entity" {
            let source = query["source"]
                .as_str()
                .ok_or("entity source must be a string")?;
            return serde_json::to_value(self.find_entity(source)?).map_err(|e| e.to_string());
        }
        if op == "object_attribute" {
            let source = query["source"]
                .as_str()
                .ok_or("object source must be a string")?;
            let name = query["name"]
                .as_str()
                .ok_or("attribute name must be a string")?;
            return Ok(match self.object_attribute(source, name)? {
                None | Some(EngineValue::Entity(None)) => serde_json::Value::Null,
                Some(EngineValue::Boolean(value)) => serde_json::json!(value),
                Some(EngineValue::Integer(value)) => serde_json::json!(value),
                Some(EngineValue::Number(value)) => serde_json::json!(value),
                Some(EngineValue::String(value)) => serde_json::json!(value),
                Some(EngineValue::Vec2(value)) => serde_json::json!(value),
                Some(EngineValue::Vec3(value)) => serde_json::json!(value),
                Some(EngineValue::Entity(Some(value))) => serde_json::json!(value),
            });
        }
        if crate::entity_services::handles(op) {
            let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
            return crate::entity_services::query(&mut world, query);
        }
        if op == "scene_environment_field" {
            let world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            let value = serde_json::to_value(world.environment()).map_err(|e| e.to_string())?;
            let field = query["field"]
                .as_str()
                .ok_or("environment field must be a string")?;
            return value
                .get(field)
                .cloned()
                .ok_or_else(|| format!("unknown environment setting `{field}`"));
        }
        if op == "scene_environment" {
            let mut world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            let mut value = serde_json::to_value(world.environment()).map_err(|e| e.to_string())?;
            if let Some(patch) = query.get("settings") {
                let fields = patch
                    .as_object()
                    .ok_or("environment settings must be an object")?;
                for (name, setting) in fields {
                    let slot = value
                        .get_mut(name)
                        .ok_or_else(|| format!("unknown environment setting `{name}`"))?;
                    *slot = setting.clone();
                }
                let environment: engine_world::SceneEnvironment = serde_json::from_value(value)
                    .map_err(|e| format!("invalid environment settings: {e}"))?;
                world
                    .apply_commands(&[WorldCommand::SetEnvironment(environment)])
                    .map_err(|e| e.to_string())?;
            }
            return serde_json::to_value(world.environment()).map_err(|e| e.to_string());
        }
        if op == "operation_state" {
            let context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let owner = Owner {
                entity_bound: self.entity_bound,
                scene: context.scene,
                entity: self.entity,
                script: self.owner_script,
            };
            let handle = query["handle"]
                .as_str()
                .ok_or("operation handle is missing")?;
            return serde_json::to_value(
                context
                    .runtime
                    .states(owner)
                    .into_iter()
                    .find(|state| state.handle == handle),
            )
            .map_err(|e| e.to_string());
        }
        if op == "clock" {
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            if let Some(scale) = query.get("scale") {
                context
                    .runtime
                    .set_time_scale(scale.as_f64().ok_or("invalid time scale")?)?;
            }
            if let Some(paused) = query.get("paused") {
                context.runtime.paused = paused.as_bool().ok_or("invalid pause value")?;
            }
            return Ok(
                serde_json::json!({"scale":context.runtime.time_scale(), "paused":context.runtime.paused}),
            );
        }
        if op == "camera_current" {
            let world = self.world.lock().map_err(|_| "world lock poisoned")?;
            let camera = world
                .entity_ids()
                .filter(|id| world.camera(*id).ok().flatten().is_some_and(|c| c.active))
                .last()
                .ok_or("scene has no active camera")?;
            return Ok(serde_json::json!(camera));
        }
        if op == "animation_ik" {
            let decode = |key: &str| {
                serde_json::from_value::<EntityId>(query[key].clone()).map_err(|e| e.to_string())
            };
            let root = decode("root")?;
            let middle = decode("middle")?;
            let tip = decode("tip")?;
            let target: [f64; 3] =
                serde_json::from_value(query["target"].clone()).map_err(|e| e.to_string())?;
            let pole: [f64; 3] = query
                .get("pole")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or([0., 0., 1.]);
            let duration = query["duration"].as_f64().unwrap_or(0.0);
            let weight = query["weight"].as_f64().unwrap_or(1.0);
            if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
                return Err("IK weight must be in [0,1]".into());
            }
            let ease: engine_core::gameplay::Ease = query
                .get("easing")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
            world.propagate_transforms();
            if world.parent(middle).map_err(|e| e.to_string())? != Some(root)
                || world.parent(tip).map_err(|e| e.to_string())? != Some(middle)
            {
                return Err("IK requires a direct two-bone hierarchy".into());
            }
            let matrix = |id| {
                world
                    .world_transform(id)
                    .map(|v| v.0.as_dmat4())
                    .map_err(|e| e.to_string())
            };
            let root_world = matrix(root)?;
            let middle_world = matrix(middle)?;
            let solution = engine_core::gameplay::procedural::two_bone(
                root_world.w_axis.truncate(),
                middle_world.w_axis.truncate(),
                matrix(tip)?.w_axis.truncate(),
                glam::DVec3::from_array(target),
                glam::DVec3::from_array(pole),
            )?;
            let parent_rotation = world
                .parent(root)
                .map_err(|e| e.to_string())?
                .map(|id| matrix(id).map(|m| m.to_scale_rotation_translation().1))
                .transpose()?
                .unwrap_or(glam::DQuat::IDENTITY);
            let root_rotation = root_world.to_scale_rotation_translation().1;
            let middle_rotation = middle_world.to_scale_rotation_translation().1;
            let desired_root = parent_rotation.inverse() * solution.root_delta * root_rotation;
            let desired_middle = (solution.root_delta * root_rotation).inverse()
                * solution.middle_delta
                * solution.root_delta
                * middle_rotation;
            let actions = [(root, desired_root), (middle, desired_middle)]
                .into_iter()
                .map(|(entity, rotation)| {
                    let current = world
                        .local_transform(entity)
                        .map_err(|e| e.to_string())?
                        .rotation
                        .as_dquat();
                    Ok(engine_core::gameplay::Action::tween(
                        engine_core::gameplay::Target {
                            entity,
                            property: "Rotation".into(),
                        },
                        engine_core::gameplay::Value::Rotation(
                            current.slerp(rotation, weight).normalize().to_array(),
                        ),
                        duration,
                        ease,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let action = engine_core::gameplay::Action::Parallel { actions };
            action.validate()?;
            return serde_json::to_value(action).map_err(|e| e.to_string());
        }
        if op.starts_with("capture_") {
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let owner = Owner {
                entity_bound: self.entity_bound,
                scene: context.scene,
                entity: self.entity,
                script: self.owner_script,
            };
            let id = query
                .get("id")
                .filter(|v| !v.is_null())
                .map(|v| v.as_str().ok_or("capture id must be a string"))
                .transpose()?;
            match op {
                "capture_recording_start" | "capture_mic_start" => {
                    let recording = op == "capture_recording_start";
                    let source = query
                        .get("source")
                        .map(|v| v.as_str().ok_or("recording source must be a string"))
                        .transpose()?
                        .unwrap_or("game");
                    if source != "game" && source != "microphone" {
                        return Err("recording source must be game or microphone".into());
                    }
                    let path = match query.get("path").filter(|v| !v.is_null()) {
                        Some(value) => {
                            let path = value
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .ok_or("recording path must be a nonempty string")?;
                            if !path.to_ascii_lowercase().ends_with(".wav") {
                                return Err("recording path must end in .wav".into());
                            }
                            let path = PathBuf::from(path);
                            Some(if path.is_absolute() {
                                path
                            } else {
                                self.snapshot_root.join(path)
                            })
                        }
                        None => None,
                    };
                    let id = context.audio_capture.start(
                        owner,
                        recording,
                        !recording || source == "microphone",
                        path.as_deref(),
                    )?;
                    return Ok(serde_json::json!(id));
                }
                "capture_recording_stop" | "capture_mic_stop" => {
                    context
                        .audio_capture
                        .stop(owner, id, op == "capture_recording_stop")?;
                }
                "capture_read" => {
                    return context
                        .audio_capture
                        .read(owner, id.ok_or("capture id is required")?);
                }
                "capture_unload" => context
                    .audio_capture
                    .unload(owner, id.ok_or("capture id is required")?)?,
                _ => return Err("unknown capture operation".into()),
            }
            return Ok(serde_json::Value::Null);
        }
        if op.starts_with("audio_") {
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            match op {
                "audio_get_output_devices" | "audio_get_input_devices" => {
                    return context
                        .audio_output
                        .devices(op == "audio_get_input_devices");
                }
                "audio_set_output_device" | "audio_set_input_device" => {
                    let device = query.get("device").filter(|v| !v.is_null());
                    let id = device
                        .map(|v| {
                            v.as_str()
                                .ok_or("audio device must be an ID string or null")
                        })
                        .transpose()?;
                    context
                        .audio_output
                        .select(op == "audio_set_input_device", id)?;
                    return Ok(serde_json::Value::Null);
                }
                _ => {}
            }
            let owner = Owner {
                entity_bound: self.entity_bound,
                scene: context.scene,
                entity: self.entity,
                script: self.owner_script,
            };
            if op == "audio_play" || op == "audio_load" || op == "audio_loaded_play" {
                let clip = if op == "audio_loaded_play" {
                    None
                } else {
                    let source = query["source"]
                        .as_str()
                        .ok_or("audio source must be an assets-relative file path")?;
                    let clip = match self.import_source(source)? {
                        engine_assets::DerivedArtifact::Audio(a) => {
                            engine_core::gameplay::audio::AudioClip {
                                channels: a.channels,
                                sample_rate: a.sample_rate,
                                samples: a.samples,
                            }
                        }
                        _ => return Err("source is not an audio clip".into()),
                    };
                    Some(Arc::new(clip))
                };
                if op == "audio_load" {
                    let id = context
                        .audio
                        .load(owner, clip.expect("load imports a clip"))?;
                    return Ok(serde_json::json!(id));
                }
                let position = query
                    .get("position")
                    .filter(|v| !v.is_null())
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| e.to_string())?;
                let volume = query["volume"].as_f64().unwrap_or(1.0);
                let pitch = query["pitch"].as_f64().unwrap_or(1.0);
                let looping = query["loop"].as_bool().unwrap_or(false);
                let id = if op == "audio_loaded_play" {
                    let id = serde_json::from_value(query["entity"].clone())
                        .map_err(|e| e.to_string())?;
                    context
                        .audio
                        .play_loaded(owner, id, volume, pitch, looping, position)?
                } else {
                    context.audio.play(
                        owner,
                        clip.expect("play imports a clip"),
                        volume,
                        pitch,
                        looping,
                        position,
                    )?
                };
                return Ok(serde_json::json!(id));
            }
            let id: EntityId =
                serde_json::from_value(query["entity"].clone()).map_err(|e| e.to_string())?;
            if op == "audio_unload" {
                context.audio.unload(owner, id)?;
                return Ok(serde_json::Value::Null);
            }
            if let Some(operation) = op.strip_prefix("audio_loaded_") {
                context.audio.control_loaded(owner, id, operation)?;
                return Ok(serde_json::Value::Null);
            }
            if op == "audio_stop" {
                context.audio.voices.remove(&id);
                return Ok(serde_json::Value::Null);
            }
            let voice = context
                .audio
                .voices
                .get_mut(&id)
                .ok_or("audio voice has finished or was stopped")?;
            match op {
                "audio_pause" => voice.paused = true,
                "audio_resume" => voice.paused = false,
                "audio_volume" => {
                    let v = query["value"].as_f64().ok_or("missing volume")?;
                    if !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX) {
                        return Err("invalid volume".into());
                    }
                    voice.volume = v;
                }
                "audio_pitch" => {
                    let v = query["value"].as_f64().ok_or("missing pitch")?;
                    if !v.is_finite() || v <= 0.0 {
                        return Err("invalid pitch".into());
                    }
                    voice.pitch = v;
                }
                _ => return Err("unknown audio operation".into()),
            }
            return Ok(serde_json::Value::Null);
        }
        if op.starts_with("animation_") {
            let entity = query
                .get("entity")
                .map(|v| serde_json::from_value::<EntityId>(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(self.entity);
            if op == "animation_load" {
                let source = query["source"]
                    .as_str()
                    .ok_or("animation source path is missing")?;
                let engine_assets::DerivedArtifact::Model(model) = self.import_source(source)?
                else {
                    return Err("animation source is not a model".into());
                };
                let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
                let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
                if !world.contains(entity) {
                    return Err("animation owner no longer exists".into());
                }
                let names = model
                    .animations
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>();
                if context.clips.keys().any(|(id, _)| *id == entity) {
                    return Err(
                        "model animations are already bound; use Animation.play or a new target"
                            .into(),
                    );
                }
                bind_model(&mut world, &mut context, entity, &model)?;
                return Ok(serde_json::json!(names));
            }
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            match op {
                "animation_marker" => {
                    let name = query["clip"].as_str().ok_or("clip name is missing")?;
                    let marker = query["name"].as_str().ok_or("marker name is missing")?;
                    let time = query["time"].as_f64().ok_or("marker time is missing")?;
                    context
                        .clips
                        .get_mut(&(entity, name.into()))
                        .ok_or("clip is not registered")?
                        .add_marker(time, marker.into())?;
                    return Ok(serde_json::Value::Null);
                }
                "animation_register" => {
                    let clip: engine_core::gameplay::animation::Clip =
                        serde_json::from_value(query["clip"].clone()).map_err(|e| e.to_string())?;
                    clip.validate()?;
                    if context.clips.len() >= 4096 {
                        return Err("clip registry limit exceeded".into());
                    }
                    context.clips.insert((entity, clip.name.clone()), clip);
                    return Ok(serde_json::Value::Null);
                }
                "animation_ref" => {
                    let name = query["name"].as_str().ok_or("clip name is missing")?;
                    if !context.clips.contains_key(&(entity, name.into())) {
                        return Err(format!("clip {name} is not registered for {entity}"));
                    }
                    return Ok(serde_json::json!(name));
                }
                "animation_clip" => {
                    let name = query["name"].as_str().ok_or("clip name is missing")?;
                    return serde_json::to_value(
                        context
                            .clips
                            .get(&(entity, name.into()))
                            .ok_or_else(|| format!("clip {name} is not registered for {entity}"))?,
                    )
                    .map_err(|e| e.to_string());
                }
                "animation_list" => {
                    return Ok(serde_json::json!(
                        context
                            .clips
                            .keys()
                            .filter(|(id, _)| *id == entity)
                            .map(|(_, name)| name)
                            .collect::<Vec<_>>()
                    ));
                }
                _ => return Err("unknown animation query".into()),
            }
        }
        if !op.starts_with("physics_") {
            let q: engine_core::gameplay::query::Query =
                serde_json::from_value(query).map_err(|e| e.to_string())?;
            return serde_json::to_value(q.evaluate()?).map_err(|e| e.to_string());
        }
        let args: Args = serde_json::from_value(query.clone()).map_err(|e| e.to_string())?;
        let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
        let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
        match op {
            "physics_raycast" | "physics_sphere_cast" => {
                let colliders = crate::physics::PhysicsWorld::colliders(&mut world)?;
                serde_json::to_value(engine_core::gameplay::physics::cast(
                    &colliders,
                    args.origin,
                    args.direction,
                    args.distance,
                    if op == "physics_raycast" {
                        0.0
                    } else {
                        args.radius
                    },
                    &args.ignore,
                )?)
                .map_err(|e| e.to_string())
            }
            "physics_overlap" => {
                let c = crate::physics::PhysicsWorld::colliders(&mut world)?;
                serde_json::to_value(engine_core::gameplay::physics::overlap(
                    &c,
                    args.center,
                    args.radius,
                    &args.ignore,
                )?)
                .map_err(|e| e.to_string())
            }
            "physics_impulse" | "physics_force" | "physics_launch" => {
                let id = args.entity.unwrap_or(self.entity);
                if op == "physics_launch" {
                    context.physics.launch(&world, id, args.vector)?;
                } else {
                    let vector = if op == "physics_force" {
                        if !args.delta.is_finite() || args.delta < 0.0 {
                            return Err("force delta must be finite and non-negative".into());
                        }
                        let interval = context.runtime.scaled_delta(args.delta)?;
                        args.vector.map(|v| v * interval)
                    } else {
                        args.vector
                    };
                    context.physics.impulse(&world, id, vector)?;
                }
                Ok(serde_json::Value::Null)
            }
            "physics_explosion" => {
                if !args.strength.is_finite() || args.strength < 0.0 {
                    return Err("explosion strength must be non-negative".into());
                }
                let c = crate::physics::PhysicsWorld::colliders(&mut world)?;
                let ids = engine_core::gameplay::physics::overlap(
                    &c,
                    args.center,
                    args.radius,
                    &args.ignore,
                )?;
                for id in &ids {
                    if world
                        .part_attributes(*id)
                        .map_err(|e| e.to_string())?
                        .anchored
                    {
                        continue;
                    }
                    let center = c
                        .iter()
                        .find(|c| c.entity == *id)
                        .ok_or("collider disappeared")?
                        .center;
                    let d = glam::DVec3::from_array(center) - glam::DVec3::from_array(args.center);
                    let falloff = if args.radius > 0.0 {
                        (1.0 - d.length() / args.radius).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let direction = if d.length_squared() > 1e-12 {
                        d.normalize()
                    } else {
                        glam::DVec3::Y
                    };
                    context.physics.impulse(
                        &world,
                        *id,
                        (direction * args.strength * falloff).to_array(),
                    )?;
                }
                serde_json::to_value(ids).map_err(|e| e.to_string())
            }
            _ => Err(format!("unknown physics operation: {op}")),
        }
    }
    fn gameplay_request(&mut self, mut request: Request) -> Result<(), String> {
        if let Request::Emit { signal, .. } = &request
            && matches!(
                signal.name.as_str(),
                "Click" | "TouchStart" | "TouchMove" | "TouchEnd" | "TouchCancel"
            )
        {
            return Err("input event names are reserved for the engine".into());
        }
        let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
        let owner = Owner {
            entity_bound: self.entity_bound,
            scene: context.scene,
            entity: self.entity,
            script: self.owner_script,
        };
        if let Request::Start { action, .. } = &mut request {
            action.resolve_clips(&|entity, name| {
                context
                    .clips
                    .get(&(entity, name.to_owned()))
                    .cloned()
                    .ok_or_else(|| format!("clip {name} is not registered for {entity}"))
            })?;
        }
        context.runtime.request(owner, request)
    }
    fn gameplay_connections(&self) -> Vec<String> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |context| {
                context.runtime.signals.tokens(Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                })
            },
        )
    }
    fn gameplay_states(&self) -> Vec<OperationState> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |context| {
                context.runtime.states(Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                })
            },
        )
    }
    fn gameplay_callbacks(&mut self) -> Vec<Delivery> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |mut context| {
                let owner = Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                };
                let deliveries = context.runtime.drain_callbacks(owner);
                let Ok(world) = self.world.lock() else {
                    return Vec::new();
                };
                deliveries
                    .into_iter()
                    .filter(|d| {
                        d.signal.as_deref() != Some("Click")
                            || d.source.is_some_and(|id| {
                                world.contains(id)
                                    && world.part_attributes(id).is_ok_and(|p| p.can_touch)
                            })
                    })
                    .collect()
            },
        )
    }
    fn cleanup_gameplay(&mut self) {
        if let Ok(mut context) = self.gameplay.lock() {
            context
                .runtime
                .cleanup(|owner| owner.script == self.owner_script);
            context
                .audio
                .cleanup(|owner| owner.script == self.owner_script);
            context
                .audio_capture
                .cleanup(|owner| owner.script == self.owner_script);
        }
    }
    fn set_current_camera(&mut self, source: &str) -> Result<(), String> {
        let id = self
            .find_entity(source)?
            .ok_or_else(|| format!("camera `{source}` was not found"))?;
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        world
            .camera(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("entity `{source}` is not a camera"))?;
        let mut commands = Vec::new();
        for entity in world.entity_ids() {
            if let Some(mut camera) = world.camera(entity).map_err(|e| e.to_string())? {
                let active = entity == id;
                if camera.active != active {
                    camera.active = active;
                    commands.push(WorldCommand::SetCamera {
                        entity,
                        value: Some(camera),
                    });
                }
            }
        }
        world.apply_commands(&commands).map_err(|e| e.to_string())
    }
    fn entity_id(&self) -> EntityId {
        self.entity
    }
    fn delta_time(&self) -> f64 {
        self.gameplay
            .lock()
            .map_or(0.0, |context| context.frame_delta)
    }
    fn fixed_delta_time(&self) -> f64 {
        self.gameplay
            .lock()
            .map_or(0.0, |context| context.fixed_delta)
    }
    fn translation(&self) -> [f64; 3] {
        self.world
            .lock()
            .ok()
            .and_then(|world| world.local_transform(self.entity).ok())
            .map_or([0.0; 3], |value| value.translation.as_dvec3().to_array())
    }
    fn set_translation(&mut self, value: [f64; 3]) -> Result<(), String> {
        if !value.iter().all(|value| value.is_finite()) {
            return Err("transform contains a non-finite value".into());
        }
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        let mut transform = world
            .local_transform(self.entity)
            .map_err(|error| error.to_string())?;
        transform.translation = glam::DVec3::from_array(value).as_vec3();
        world
            .apply_commands(&[WorldCommand::SetLocalTransform {
                entity: self.entity,
                value: transform,
            }])
            .map_err(|error| error.to_string())
    }
    fn find_entity(&self, query: &str) -> Result<Option<EntityId>, String> {
        if query.starts_with("rustic.game.") || query.starts_with("game.scene.") {
            return self.object_entity(query).map(Some);
        }
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        if let Ok(id) = query.parse::<EntityId>() {
            return Ok(world.contains(id).then_some(id));
        }
        if let Some(id) = world.find_path(query).map_err(|error| error.to_string())? {
            return Ok(Some(id));
        }
        for id in world.entity_ids() {
            if world
                .snapshot(id)
                .map_err(|error| error.to_string())?
                .name
                .as_deref()
                == Some(query)
            {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
    fn list_entities(&self, path: &str) -> Result<Vec<EntityId>, String> {
        self.world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?
            .list_path(path)
            .map_err(|error| error.to_string())
    }
    fn scene_paths(&self) -> BTreeMap<String, EntityId> {
        let Ok(world) = self.world.lock() else {
            return BTreeMap::new();
        };
        let mut paths = BTreeMap::new();
        for id in world.entity_ids() {
            if let Ok(path) = world.entity_path(id) {
                paths.insert(path.clone(), id);
                paths.insert(format!("Game.scene.{path}"), id);
                paths.insert(format!("rustic.game.{}.{path}", self.scene_name), id);
                paths.insert(format!("game.scene.{}.{path}", self.scene_name), id);
            }
        }
        paths
    }
    fn add_instance(&mut self, source: &str, parent: Option<EntityId>) -> Result<EntityId, String> {
        if self.resolve_entity(source)?.is_some() {
            return self.clone_instance(source, parent);
        }
        if let Some(snapshot) = self.asset_instance(source, parent)? {
            let engine_assets::DerivedArtifact::Model(model) = self.import_source(source)? else {
                return Err("instance asset is not a model".into());
            };
            let id = snapshot.id;
            self.world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?
                .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
                .map_err(|error| error.to_string())?;
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
            bind_model(&mut world, &mut context, id, &model)?;
            return Ok(id);
        }
        let name = source.rsplit(['/', '.']).next().unwrap_or(source);
        let primitive = match name.to_ascii_lowercase().as_str() {
            "part" | "cube" => engine_world::Primitive::Cube { size: 1.0 },
            "sphere" => engine_world::Primitive::Sphere {
                radius: 0.5,
                segments: 24,
                rings: 12,
            },
            "cylinder" => engine_world::Primitive::Cylinder {
                radius: 0.5,
                height: 1.0,
                segments: 24,
            },
            "plane" => engine_world::Primitive::Plane { size: [1.0, 1.0] },
            "rectangle" | "rectangle2d" => {
                engine_world::Primitive::Rectangle2d { size: [1.0, 1.0] }
            }
            "circle" | "circle2d" => engine_world::Primitive::Circle2d {
                radius: 0.5,
                segments: 24,
            },
            _ => {
                return Err(format!(
                    "instance source `{source}` was not found; use an entity ID, scene/explorer path, or built-in object name"
                ));
            }
        };
        let snapshot = engine_world::EntitySnapshot {
            name: Some(name.to_owned()),
            parent,
            primitive: Some(primitive),
            ..engine_world::EntitySnapshot::default()
        };
        let id = snapshot.id;
        self.world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .map_err(|error| error.to_string())?;
        Ok(id)
    }
    fn clone_instance(
        &mut self,
        source: &str,
        parent: Option<EntityId>,
    ) -> Result<EntityId, String> {
        let source_id = self
            .resolve_entity(source)?
            .ok_or_else(|| format!("instance source `{source}` was not found"))?;
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let mut snapshot = world
            .snapshot(source_id)
            .map_err(|error| error.to_string())?;
        snapshot.id = EntityId::new();
        snapshot.parent = parent.or(snapshot.parent);
        let id = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .map_err(|error| error.to_string())?;
        Ok(id)
    }
    fn object_attribute(&self, source: &str, name: &str) -> Result<Option<EngineValue>, String> {
        let mut target = self.clone();
        target.entity = self.object_entity(source)?;
        target.attribute(name)
    }
    fn edit_object_attribute(
        &mut self,
        source: &str,
        name: &str,
        value: EngineValue,
    ) -> Result<(), String> {
        let mut target = self.clone();
        target.entity = self.object_entity(source)?;
        target.edit_attribute(name, value)
    }
    fn attribute(&self, name: &str) -> Result<Option<EngineValue>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let snapshot = world
            .snapshot(self.entity)
            .map_err(|error| error.to_string())?;
        let value = match name
            .to_ascii_lowercase()
            .replace(['_', '-', ' '], "")
            .as_str()
        {
            "name" => EngineValue::String(snapshot.name.unwrap_or_default()),
            "position" => EngineValue::Vec3(
                snapshot
                    .local_transform
                    .translation
                    .to_array()
                    .map(f64::from),
            ),
            "size" => EngineValue::Vec3(snapshot.local_transform.scale.to_array().map(f64::from)),
            "color" => EngineValue::Vec3(
                snapshot.part_attributes.color[..3]
                    .try_into()
                    .unwrap_or([1.0; 3])
                    .map(f64::from),
            ),
            "cantouch" => EngineValue::Boolean(snapshot.part_attributes.can_touch),
            "cancollide" => EngineValue::Boolean(snapshot.part_attributes.can_collide),
            "anchored" => EngineValue::Boolean(snapshot.part_attributes.anchored),
            "parent" => EngineValue::Entity(snapshot.parent),
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    fn edit_attribute(&mut self, name: &str, value: EngineValue) -> Result<(), String> {
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let key = name.to_ascii_lowercase().replace(['_', '-', ' '], "");
        match (key.as_str(), value) {
            ("name", EngineValue::String(value)) => {
                world.apply_commands(&[WorldCommand::SetName {
                    entity: self.entity,
                    value: Some(value),
                }])
            }
            ("position" | "size", EngineValue::Vec3(value)) => {
                if !value.iter().all(|value| value.is_finite()) {
                    return Err("attribute contains a non-finite value".into());
                }
                let mut transform = world
                    .local_transform(self.entity)
                    .map_err(|error| error.to_string())?;
                let converted = glam::DVec3::from_array(value).as_vec3();
                if key == "position" {
                    transform.translation = converted;
                } else {
                    transform.scale = converted;
                }
                world.apply_commands(&[WorldCommand::SetLocalTransform {
                    entity: self.entity,
                    value: transform,
                }])
            }
            ("parent", EngineValue::Entity(parent)) => {
                world.apply_commands(&[WorldCommand::SetParent {
                    child: self.entity,
                    parent,
                }])
            }
            (property @ ("color" | "cantouch" | "cancollide" | "anchored"), value) => {
                let mut attributes = world
                    .part_attributes(self.entity)
                    .map_err(|error| error.to_string())?;
                match (property, value) {
                    ("color", EngineValue::Vec3(value)) if value.iter().all(|v| v.is_finite()) => {
                        attributes.color[..3]
                            .copy_from_slice(&glam::DVec3::from_array(value).as_vec3().to_array());
                    }
                    ("cantouch", EngineValue::Boolean(value)) => attributes.can_touch = value,
                    ("cancollide", EngineValue::Boolean(value)) => attributes.can_collide = value,
                    ("anchored", EngineValue::Boolean(value)) => attributes.anchored = value,
                    _ => return Err(format!("attribute `{name}` type mismatch")),
                }
                world.apply_commands(&[WorldCommand::SetPartAttributes {
                    entity: self.entity,
                    value: attributes,
                }])
            }
            _ => return Err(format!("unknown attribute `{name}` or type mismatch")),
        }
        .map_err(|error| error.to_string())?;
        world.propagate_transforms();
        Ok(())
    }
    fn input_action(&self, _name: &str) -> ActionState {
        ActionState::default()
    }
    fn input_frame(&self) -> InputFrame {
        InputFrame {
            keys: self
                .input_keys
                .lock()
                .map_or_else(|_| BTreeSet::new(), |keys| keys.clone()),
            ..InputFrame::default()
        }
    }
    fn log(&mut self, level: &str, message: &str) -> Result<(), String> {
        let mut logs = self.logs.lock().map_err(|_| "script log lock poisoned")?;
        if logs.len() >= MAX_SCRIPT_LOGS {
            logs.pop_front();
            if let Ok(mut dropped) = self.dropped_logs.lock() {
                *dropped = dropped.saturating_add(1);
            }
        }
        logs.push_back((level.to_owned(), message.chars().take(4096).collect()));
        Ok(())
    }
    fn property(&self, name: &str) -> Option<EngineValue> {
        self.properties.lock().ok()?.get(name).cloned()
    }
    fn properties(&self) -> BTreeMap<String, EngineValue> {
        self.properties
            .lock()
            .map_or_else(|_| BTreeMap::new(), |properties| properties.clone())
    }
    fn set_property(&mut self, name: &str, value: EngineValue) -> Result<(), String> {
        let mut properties = self
            .properties
            .lock()
            .map_err(|_| "property lock poisoned")?;
        let old = properties
            .get(name)
            .ok_or_else(|| format!("property `{name}` is not declared"))?;
        if !old.same_type(&value) {
            return Err(format!("property `{name}` type mismatch"));
        }
        properties.insert(name.into(), value);
        Ok(())
    }
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    fn entity_lifetime_ended(&self) -> bool {
        self.entity_bound
            && self
                .world
                .lock()
                .is_ok_and(|world| !world.contains(self.entity))
    }
    fn enabled(&self) -> bool {
        self.enabled
            && (!self.entity_bound
                || self
                    .world
                    .lock()
                    .is_ok_and(|world| world.contains(self.entity)))
    }
}

impl RuntimeHost {
    fn object_entity(&self, source: &str) -> Result<EntityId, String> {
        if let Some(rest) = source
            .strip_prefix("game.scene.")
            .or_else(|| source.strip_prefix("rustic.game."))
        {
            let prefix = format!("{}.", self.scene_name);
            let path = rest.strip_prefix(&prefix).ok_or_else(|| {
                format!(
                    "scene in `{source}` is not loaded (loaded scene: `{}`)",
                    self.scene_name
                )
            })?;
            let world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            // Compare full hierarchy paths, rejecting duplicates instead of choosing arbitrarily.
            let matches: Vec<_> = world
                .entity_ids()
                .filter(|id| world.entity_path(*id).ok().as_deref() == Some(path))
                .collect();
            return match matches.as_slice() {
                [id] => Ok(*id),
                [] => Err(format!("scene object `{source}` was not found")),
                _ => Err(format!(
                    "scene object `{source}` is ambiguous; give siblings unique names"
                )),
            };
        }
        self.find_entity(source)?
            .ok_or_else(|| format!("scene object `{source}` was not found"))
    }

    fn resolve_entity(&self, source: &str) -> Result<Option<EntityId>, String> {
        if source.starts_with("game.scene.") || source.starts_with("rustic.game.") {
            return self.object_entity(source).map(Some);
        }
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        if let Ok(id) = source.parse::<EntityId>() {
            return Ok(world.contains(id).then_some(id));
        }
        world.find_path(source).map_err(|error| error.to_string())
    }

    fn import_source(&self, source: &str) -> Result<engine_assets::DerivedArtifact, String> {
        let relative = Path::new(source);
        if relative.is_absolute()
            || relative
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || relative
                .components()
                .next()
                .is_none_or(|p| p.as_os_str() != "assets")
        {
            return Err("source must be a path inside assets".into());
        }
        let path = self.snapshot_root.join(relative);
        let assets = self
            .snapshot_root
            .join("assets")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&assets)
        {
            return Err("source escapes the asset directory".into());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let mut request = engine_assets::ImportRequest::new(relative, bytes);
        if matches!(
            relative.extension().and_then(|s| s.to_str()),
            Some("gltf" | "glb")
        ) {
            // Resolve only validated, bounded dependencies inside the source directory.
            for uri in engine_assets::model_dependencies(&request.source_bytes)
                .map_err(|e| e.to_string())?
            {
                let dependency = path.parent().ok_or("source parent missing")?.join(&uri);
                if !dependency
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(&assets)
                {
                    return Err("model dependency escapes the asset directory".into());
                }
                request
                    .external_bytes
                    .insert(uri, std::fs::read(dependency).map_err(|e| e.to_string())?);
            }
        }
        engine_assets::ImporterRegistry::import(&request).map_err(|e| e.to_string())
    }
    fn asset_instance(
        &self,
        source: &str,
        parent: Option<EntityId>,
    ) -> Result<Option<engine_world::EntitySnapshot>, String> {
        let relative = Path::new(source);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || relative
                .components()
                .next()
                .is_none_or(|part| part.as_os_str() != "assets")
        {
            return Ok(None);
        }
        let extension = relative
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "obj" | "gltf" | "glb" | "fbx") {
            return Err(format!(
                "asset `{source}` is not a supported model (expected .obj, .gltf, .glb, or .fbx)"
            ));
        }
        let asset_path = self.snapshot_root.join(relative);
        if !asset_path.is_file() {
            return Err(format!(
                "asset `{source}` is not present in the play snapshot"
            ));
        }
        let meta_path = engine_assets::sidecar_path(&asset_path);
        let meta = engine_assets::load_meta(&meta_path)
            .map_err(|error| format!("asset `{source}` has no valid import metadata: {error}"))?;
        Ok(Some(engine_world::EntitySnapshot {
            name: relative
                .file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned),
            parent,
            mesh: Some(Mesh {
                asset: meta.asset_id,
            }),
            ..engine_world::EntitySnapshot::default()
        }))
    }
}

fn bind_model(
    world: &mut SceneWorld,
    context: &mut GameplayContext,
    entity: EntityId,
    model: &engine_assets::ModelArtifact,
) -> Result<(), String> {
    model.validate().map_err(|e| e.to_string())?;
    if context.clips.len() + model.animations.len() > 4096 {
        return Err("clip registry limit exceeded".into());
    }
    let ids = model
        .nodes
        .iter()
        .map(|_| EntityId::new())
        .collect::<Vec<_>>();
    let mut commands = Vec::new();
    for (index, node) in model.nodes.iter().enumerate() {
        commands.push(WorldCommand::Spawn(Box::new(
            engine_world::EntitySnapshot {
                id: ids[index],
                name: Some(format!("node_{index}")),
                parent: Some(entity),
                local_transform: engine_world::LocalTransform {
                    translation: glam::DVec3::from_array(node.translation).as_vec3(),
                    rotation: glam::DQuat::from_array(node.rotation).as_quat(),
                    scale: glam::DVec3::from_array(node.scale).as_vec3(),
                },
                ..Default::default()
            },
        )));
    }
    for (index, node) in model.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            commands.push(WorldCommand::SetParent {
                child: ids[index],
                parent: Some(*ids.get(parent).ok_or("invalid model parent index")?),
            });
        }
    }
    world.apply_commands(&commands).map_err(|e| e.to_string())?;
    for clip in &model.animations {
        context
            .clips
            .insert((entity, clip.name.clone()), clip.clone());
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "fixtures start from defaults and vary only the authored values under test"
)]
mod tests {
    use super::*;
    use engine_scripting::{
        ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
        save_manifest_atomic,
    };
    use engine_world::{EntitySnapshot, SceneDocument, ScriptComponent};

    #[test]
    fn queued_clicks_recheck_can_touch_and_scripts_cannot_forge_input_events() {
        use crate::pointer_input::{PointerDevice, PointerEvent, PointerPhase};
        let mut scene = SceneWorld::new();
        let camera = EntitySnapshot {
            camera: Some(engine_world::Camera::default()),
            ..EntitySnapshot::default()
        };
        let mut target = EntitySnapshot {
            primitive: Some(engine_world::Primitive::Cube { size: 2.0 }),
            ..EntitySnapshot::default()
        };
        target.local_transform.translation.z = 5.0;
        let id = target.id;
        scene
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(camera)),
                WorldCommand::Spawn(Box::new(target)),
            ])
            .unwrap();
        let world = Arc::new(Mutex::new(scene));
        let mut host = environment_host(&world);
        host.gameplay_request(Request::Connect {
            token: "click-test".into(),
            signal: engine_core::gameplay::Signal {
                name: "Click".into(),
                source: Some(id),
            },
            once: false,
        })
        .unwrap();
        let tap = || {
            [PointerPhase::Start, PointerPhase::End]
                .into_iter()
                .map(|phase| PointerEvent {
                    id: "finger".into(),
                    device: PointerDevice::Touch,
                    phase,
                    position: [0.5, 0.5],
                    pressure: 0.5,
                    aspect: 16.0 / 9.0,
                })
                .collect()
        };
        {
            let mut context = host.gameplay.lock().unwrap();
            let GameplayContext {
                pointer_input,
                runtime,
                ..
            } = &mut *context;
            pointer_input.enqueue(tap());
            pointer_input.begin_frame(&mut world.lock().unwrap(), &mut runtime.signals);
        }
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"input_clicks"}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        {
            let mut world = world.lock().unwrap();
            let mut attrs = world.part_attributes(id).unwrap();
            attrs.can_touch = false;
            world
                .apply_commands(&[WorldCommand::SetPartAttributes {
                    entity: id,
                    value: attrs,
                }])
                .unwrap();
        }
        assert!(host.gameplay_callbacks().is_empty());
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"input_clicks"}))
                .unwrap(),
            serde_json::json!([])
        );
        assert!(
            host.gameplay_request(Request::Emit {
                signal: engine_core::gameplay::Signal {
                    name: "Click".into(),
                    source: Some(id)
                },
                arguments: Vec::new()
            })
            .is_err()
        );
        assert!(
            host.gameplay_query(serde_json::json!({"op":"input_clicks","entity":"malformed"}))
                .is_err()
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Exercise the same input contract in every language adapter"
    )]
    fn pointer_input_bindings_deliver_clicks_and_touch_queries_in_available_adapters() {
        use crate::pointer_input::{PointerDevice, PointerEvent, PointerPhase};
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54,"input.lua",br"return {Start=function()
                Input.onClick(rustic.entity_id(),function(id,pos,device,pressure) assert(id=='finger' and pos[1]==0.5 and device=='touch');rustic.log('info','clicked') end)
                Input.onTouchStart(function() rustic.log('info','touch start') end)
            end,Update=function() assert(#Input.touches()==0 and #Input.touchEvents()==2 and #Input.clicks(rustic.entity_id())==1) end}"),
            (ScriptLanguage::Luau,"input.luau",br"return {Start=function()
                Input.onClick(rustic.entity_id(),function() rustic.log('info','clicked') end)
                Input.onTouchStart(function() rustic.log('info','touch start') end)
            end,Update=function() assert(#Input.touches()==0 and #Input.touchEvents()==2 and #Input.clicks(rustic.entity_id())==1) end}"),
            (ScriptLanguage::JavaScript,"input.js",br"globalThis.behavior={Start(){Input.onClick(rustic.entity_id(),(id,pos,device)=>{if(id!=='finger'||pos[0]!==0.5||device!=='touch')throw Error('payload');rustic.log('info','clicked');});Input.onTouchStart(()=>rustic.log('info','touch start'));},Update(){if(Input.touches().length!==0||Input.touchEvents().length!==2||Input.clicks(rustic.entity_id()).length!==1)throw Error('input snapshot');}};"),
            (ScriptLanguage::Web,"input.html",br"<!doctype html><html><head><title>Input</title></head><body><script>globalThis.behavior={Start(){Input.onClick(rustic.entity_id(),()=>rustic.log('info','clicked'));Input.onTouchStart(()=>rustic.log('info','touch start'));},Update(){if(Input.touchEvents().length!==2||Input.clicks().length!==1)throw Error('input snapshot');}};</script></body></html>"),
            (ScriptLanguage::Python,"input.py",br"from rustic import rustic,Input,run
def on_start():
    Input.onClick(rustic.entity_id(),lambda *args: rustic.log('info','clicked'))
    Input.onTouchStart(lambda *args: rustic.log('info','touch start'))
def update(dt):
    assert len(Input.touches())==0 and len(Input.touchEvents())==2 and len(Input.clicks(rustic.entity_id()))==1
run(globals())"),
            (ScriptLanguage::C,"input.c",br#"#include "rustic.h"
void clicked(const RusticValue *args,size_t n){if(n!=4||strcmp(args[0].string,"finger")||args[1].vector[0]!=0.5)r_fail("payload");rustic.log("info","clicked");}
void touch(const RusticValue *args,size_t n){(void)args;(void)n;rustic.log("info","touch start");}
void start(void){Input.onClick(rustic.entity_id(),clicked);Input.onTouchStart(touch);}
void update(double dt){(void)dt;RusticPointerEvents events=Input.touchEvents();RusticObjectClicks clicks=Input.clicks(NULL);if(Input.touches().count!=0||events.count!=2||clicks.count!=1||strcmp(events.items[0].id,"finger")||strcmp(clicks.items[0].pointer.device,"touch"))r_fail("input snapshot");}
int main(void){return rustic_run((RusticBehavior){.on_start=start,.update=update});}"#),
            (ScriptLanguage::Cpp,"input.cpp",br#"#include "rustic.hpp"
void start(){Input::onClick(rustic.entity_id(),[](const RusticValue::Array&){rustic.log("info","clicked");});Input::onTouchStart([](const RusticValue::Array&){rustic.log("info","touch start");});}
void update(double){if(Input::touches().array().size()!=0||Input::touchEvents().array().size()!=2||Input::clicks().array().size()!=1)throw std::runtime_error("input snapshot");}
int main(){return rustic_run(RusticBehavior{.on_start=start,.update=update});}"#),
            (ScriptLanguage::CSharp,"input.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){Input.onClick(rustic.entity_id(),a=>rustic.log("info","clicked"));Input.onTouchStart(a=>rustic.log("info","touch start"));}if(callback=="update"&&(Input.touches().GetArrayLength()!=0||Input.touchEvents().GetArrayLength()!=2||Input.clicks().GetArrayLength()!=1))throw new System.Exception("input snapshot");});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic{public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){Input.onClick(rustic.entity_id(),a->rustic.log("info","clicked"));Input.onTouchStart(a->rustic.log("info","touch start"));}if(callback.equals("update")&&(((java.util.List<?>)Input.touches()).size()!=0||((java.util.List<?>)Input.touchEvents()).size()!=2||((java.util.List<?>)Input.clicks()).size()!=1))throw new RuntimeException("input snapshot");});}}"#),
            (ScriptLanguage::Php,"input.php",br"<?php
require_once __DIR__.'/rustic.php';
rustic_run(['on_start'=>function(){global $rustic;Input::onClick($rustic->entity_id(),fn(...$args)=>$rustic->log('info','clicked'));Input::onTouchStart(fn(...$args)=>$rustic->log('info','touch start'));},'update'=>function($dt){if(count(Input::touches())!==0||count(Input::touchEvents())!==2||count(Input::clicks())!==1)throw new Exception('input snapshot');}]);"),
        ];
        for &(language, name, source) in fixtures {
            if !matches!(
                language,
                ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
            ) && !engine_scripting::probe_language_toolchain(language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_API_ADAPTERS").is_none(),
                    "{} toolchain required for input conformance",
                    language.display_name()
                );
                eprintln!(
                    "SKIPPED pointer input {}: missing toolchain",
                    language.display_name()
                );
                continue;
            }
            let mut scene = SceneWorld::new();
            let camera = EntitySnapshot {
                camera: Some(engine_world::Camera::default()),
                ..EntitySnapshot::default()
            };
            let mut target = EntitySnapshot {
                primitive: Some(engine_world::Primitive::Cube { size: 2.0 }),
                ..EntitySnapshot::default()
            };
            target.local_transform.translation.z = 5.0;
            let id = target.id;
            scene
                .apply_commands(&[
                    WorldCommand::Spawn(Box::new(camera)),
                    WorldCommand::Spawn(Box::new(target)),
                ])
                .unwrap();
            let world = Arc::new(Mutex::new(scene));
            let mut host = environment_host(&world);
            host.entity = id;
            host.entity_bound = true;
            let gameplay = Arc::clone(&host.gameplay);
            let logs = Arc::clone(&host.logs);
            let mut behavior =
                RuntimeBehavior::load(language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap();
            behavior.on_start().unwrap();
            let events = [PointerPhase::Start, PointerPhase::End]
                .into_iter()
                .map(|phase| PointerEvent {
                    id: "finger".into(),
                    device: PointerDevice::Touch,
                    phase,
                    position: [0.5, 0.5],
                    pressure: 0.7,
                    aspect: 16.0 / 9.0,
                })
                .collect();
            {
                let mut context = gameplay.lock().unwrap();
                let GameplayContext {
                    pointer_input,
                    runtime,
                    ..
                } = &mut *context;
                pointer_input.enqueue(events);
                pointer_input.begin_frame(&mut world.lock().unwrap(), &mut runtime.signals);
            }
            behavior
                .update(1.0 / 60.0)
                .unwrap_or_else(|e| panic!("{}: {e}", language.display_name()));
            let logs = logs.lock().unwrap();
            assert!(
                logs.iter().any(|(_, m)| m == "clicked"),
                "{}: {logs:?}",
                language.display_name()
            );
            assert!(
                logs.iter().any(|(_, m)| m == "touch start"),
                "{}",
                language.display_name()
            );
        }
    }

    #[test]
    fn scene_lifecycle_works_in_every_available_language() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54, "scenes.lua", br"return {Start=function()
                local level=game.scene.load('scene/level.scene')
                assert(level=='Level' and game.scene.isLoaded(level))
                assert(not pcall(function() game.scene.switchTo('Missing') end))
                game.scene.switchTo(level)
                assert(game.scene.getActive()==level and #game.scene.getLoaded()==2)
                game.scene.unload(level)
                assert(not game.scene.isLoaded(level) and game.scene.getActive()==nil)
                game.scene.switchTo(level);assert(game.scene.isLoaded(level))
                game.scene.switchTo('Menu');assert(not game.scene.isLoaded(level))
            end}"),
            (ScriptLanguage::Luau, "scenes.luau", br"return {Start=function()
                local level=game.scene.load('scene/level.scene');assert(game.scene.isLoaded(level))
                game.scene.switchTo(level);assert(game.scene.getActive()==level and #game.scene.getLoaded()==2)
                game.scene.unload(level);assert(not game.scene.isLoaded(level))
                game.scene.switchTo(level);assert(game.scene.isLoaded(level));game.scene.switchTo('Menu');assert(not game.scene.isLoaded(level))
            end}"),
            (ScriptLanguage::JavaScript, "scenes.js", br"globalThis.behavior={Start(){
                const level=game.scene.load('scene/level.scene');
                if(level!=='Level'||!game.scene.isLoaded(level))throw Error('load failed');
                game.scene.switchTo(level);
                if(game.scene.getActive()!==level||game.scene.getLoaded().length!==2)throw Error('switch failed');
                game.scene.unload(level);
                if(game.scene.isLoaded(level)||game.scene.getActive()!==null)throw Error('unload failed');
                game.scene.switchTo(level);if(!game.scene.isLoaded(level))throw Error('autoload failed');
                game.scene.switchTo('Menu');if(game.scene.isLoaded(level))throw Error('auto unload failed');
            }};"),
            (ScriptLanguage::Web, "scenes.html", br"<!doctype html><html><head><title>Scene lifecycle</title></head><body><script>globalThis.behavior={Start(){
                const level=game.scene.load('scene/level.scene');game.scene.switchTo(level);
                if(!game.scene.isLoaded(level)||game.scene.getActive()!==level||game.scene.getLoaded().length!==2)throw Error('load/switch failed');
                game.scene.unload(level);if(game.scene.isLoaded(level))throw Error('unload failed');
                game.scene.switchTo(level);if(!game.scene.isLoaded(level))throw Error('autoload');game.scene.switchTo('Menu');if(game.scene.isLoaded(level))throw Error('auto unload');
            }};</script></body></html>"),
            (ScriptLanguage::Python, "scenes.py", br"from rustic import *
def on_start():
    level=game.scene.load('scene/level.scene')
    assert level=='Level' and game.scene.isLoaded(level)
    game.scene.switchTo(level)
    assert game.scene.getActive()==level and len(game.scene.getLoaded())==2
    game.scene.unload(level)
    assert not game.scene.isLoaded(level) and game.scene.getActive() is None
    game.scene.switchTo(level)
    assert game.scene.isLoaded(level)
    game.scene.switchTo('Menu')
    assert not game.scene.isLoaded(level)
run(globals())"),
            (ScriptLanguage::Cpp, "scenes.cpp", br#"#include "rustic.hpp"
void start(){auto level=game.scene.load("scene/level.scene");if(!game.scene.isLoaded(level))throw std::runtime_error("load");game.scene.switchTo(level);if(game.scene.getActive().string()!=level||game.scene.getLoaded().array().size()!=2)throw std::runtime_error("switch");game.scene.unload(level);if(game.scene.isLoaded(level))throw std::runtime_error("unload");game.scene.switchTo(level);if(!game.scene.isLoaded(level))throw std::runtime_error("autoload");game.scene.switchTo("Menu");if(game.scene.isLoaded(level))throw std::runtime_error("auto unload");}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C, "scenes.c", br#"#include "rustic.h"
void start(void){const char *level=Game.scene.load("scene/level.scene");if(!Game.scene.isLoaded(level))r_fail("load");Game.scene.switchTo(level);if(strcmp(Game.scene.getActive(),level)||Game.scene.getLoaded().count!=2)r_fail("switch");Game.scene.unload(level);if(Game.scene.isLoaded(level)||Game.scene.getActive())r_fail("unload");Game.scene.switchTo(level);if(!Game.scene.isLoaded(level))r_fail("autoload");Game.scene.switchTo("Menu");if(Game.scene.isLoaded(level))r_fail("auto unload");}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp, "scenes.cs", br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){var level=game.scene.load("scene/level.scene");if(!game.scene.isLoaded(level))throw new System.Exception("load");game.scene.switchTo(level);if(game.scene.getActive()!=level||game.scene.getLoaded().Length!=2)throw new System.Exception("switch");game.scene.unload(level);if(game.scene.isLoaded(level))throw new System.Exception("unload");game.scene.switchTo(level);if(!game.scene.isLoaded(level))throw new System.Exception("autoload");game.scene.switchTo("Menu");if(game.scene.isLoaded(level))throw new System.Exception("auto unload");}});"#),
            (ScriptLanguage::Java, "RusticBehavior.java", br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){String level=game.scene.load("scene/level.scene");if(!game.scene.isLoaded(level))throw new RuntimeException("load");game.scene.switchTo(level);if(!game.scene.getActive().equals(level)||game.scene.getLoaded().size()!=2)throw new RuntimeException("switch");game.scene.unload(level);if(game.scene.isLoaded(level))throw new RuntimeException("unload");game.scene.switchTo(level);if(!game.scene.isLoaded(level))throw new RuntimeException("autoload");game.scene.switchTo("Menu");if(game.scene.isLoaded(level))throw new RuntimeException("auto unload");}});}}"#),
            (ScriptLanguage::Php, "scenes.php", br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $game;$level=$game->scene->load("scene/level.scene");if(!$game->scene->isLoaded($level))throw new Exception("load");$game->scene->switchTo($level);if($game->scene->getActive()!==$level||count($game->scene->getLoaded())!==2)throw new Exception("switch");$game->scene->unload($level);if($game->scene->isLoaded($level))throw new Exception("unload");$game->scene->switchTo($level);if(!$game->scene->isLoaded($level))throw new Exception("autoload");$game->scene->switchTo("Menu");if($game->scene->isLoaded($level))throw new Exception("auto unload");}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(*language).available {
                eprintln!(
                    "skipped scene lifecycle: {} (toolchain unavailable)",
                    language.display_name()
                );
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("scene")).unwrap();
            engine_world::save_scene_atomic(
                temp.path().join("scene/level.scene"),
                &SceneDocument::new("Level"),
            )
            .unwrap();
            let mut runtime =
                RuntimeScripts::load(temp.path(), &SceneDocument::new("Menu").to_bytes().unwrap())
                    .unwrap();
            let mut host = environment_host(&runtime.active.world);
            host.snapshot_root = temp.path().to_owned();
            host.scenes = Arc::clone(&runtime.scenes);
            let mut behavior =
                RuntimeBehavior::load(*language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap();
            behavior.on_start().unwrap();
            assert!(
                !runtime.scenes.lock().unwrap().loaded.contains_key("Level"),
                "{}",
                language.display_name()
            );
            drop(behavior);
            runtime.frame_update(0.01);
            assert_eq!(runtime.active.name, "Menu");
            eprintln!("validated scene lifecycle: {}", language.display_name());
        }
    }

    #[test]
    fn scene_unload_drops_worlds_contexts_and_behaviors_and_globals_survive() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("scene")).unwrap();
        std::fs::create_dir(temp.path().join("scripts")).unwrap();
        let global = ScriptId::new();
        let local = ScriptId::new();
        std::fs::write(
            temp.path().join("scripts/global.lua"),
            br"
            local step=0
            return {Update=function()
                step=step+1
                if step==1 then
                    local next=game.scene.load('scene/level.scene')
                    game.scene.switchTo(next)
                    game.scene.unload('Menu')
                elseif step==2 then
                    assert(game.scene.getActive()=='Level')
                    game.scene.unload('Level')
                elseif step==3 then
                    assert(game.scene.getActive()==nil)
                    game.scene.switchTo(game.scene.load('scene/level.scene'))
                end
                rustic.log('info','global '..step)
            end, FixedUpdate=function()
                if step==2 then Timer.after(0,function() rustic.log('info','empty timer') end) end
            end}
        ",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/local.lua"),
            br"return {
            Start=function() rustic.log('info','level start') end,
            OnDestroy=function() rustic.log('info','level destroyed') end
        }",
        )
        .unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: [(global, "scripts/global.lua"), (local, "scripts/local.lua")]
                    .into_iter()
                    .map(|(id, path)| ScriptManifestEntry {
                        id,
                        language: ScriptLanguage::Lua54,
                        relative_path: path.into(),
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    })
                    .collect(),
            },
        )
        .unwrap();
        GameSettings {
            global_scripts: vec![ScriptReference::new(global)],
            ..GameSettings::default()
        }
        .save(temp.path())
        .unwrap();
        let mut level = SceneDocument::new("Level");
        level.startup_scripts.push(ScriptReference::new(local));
        engine_world::save_scene_atomic(temp.path().join("scene/level.scene"), &level).unwrap();
        let mut runtime =
            RuntimeScripts::load(temp.path(), &SceneDocument::new("Menu").to_bytes().unwrap())
                .unwrap();
        let menu_world = Arc::downgrade(&runtime.active.world);
        let menu_context = Arc::downgrade(&runtime.active.gameplay);
        runtime.frame_update(0.01);
        assert_eq!(runtime.active.name, "Level");
        assert!(menu_world.upgrade().is_none());
        assert!(menu_context.upgrade().is_none());
        let level_world = Arc::downgrade(&runtime.active.world);
        let level_context = Arc::downgrade(&runtime.active.gameplay);
        runtime.frame_update(0.01);
        assert!(runtime.scenes.lock().unwrap().loaded.is_empty());
        assert!(runtime.inactive.is_empty());
        assert!(level_world.upgrade().is_none());
        assert!(level_context.upgrade().is_none());
        assert!(runtime.active.behaviors.is_empty());
        assert_eq!(runtime.globals.len(), 1);
        let empty_world = Arc::clone(&runtime.active.world);
        runtime.fixed_update(0.01);
        assert!(Arc::ptr_eq(&empty_world, &runtime.active.world));
        runtime.frame_update(0.01);
        assert_eq!(runtime.active.name, "Level");
        let logs = runtime.drain_logs();
        assert_eq!(logs.iter().filter(|(_, m)| m == "level start").count(), 2);
        assert_eq!(
            logs.iter().filter(|(_, m)| m == "level destroyed").count(),
            1
        );
        assert!(logs.iter().any(|(_, m)| m == "global 3"));
        assert!(logs.iter().any(|(_, m)| m == "empty timer"));
        assert!(!logs.iter().any(|(l, _)| l == "error"), "{logs:?}");
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "runs the shipped examples through all ten adapters"
    )]
    fn shipped_api_smoke_entries_move_only_while_w_is_held() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (
                ScriptLanguage::Lua54,
                "entry.lua",
                include_bytes!("../../../examples/api-smoke-test/entry.lua").as_slice(),
            ),
            (
                ScriptLanguage::Luau,
                "entry.luau",
                include_bytes!("../../../examples/api-smoke-test/entry.luau").as_slice(),
            ),
            (
                ScriptLanguage::JavaScript,
                "entry.js",
                include_bytes!("../../../examples/api-smoke-test/entry.js").as_slice(),
            ),
            (
                ScriptLanguage::Web,
                "entry.html",
                include_bytes!("../../../examples/api-smoke-test/entry.html").as_slice(),
            ),
            (
                ScriptLanguage::Python,
                "entry.py",
                include_bytes!("../../../examples/api-smoke-test/entry.py").as_slice(),
            ),
            (
                ScriptLanguage::C,
                "entry.c",
                include_bytes!("../../../examples/api-smoke-test/entry.c").as_slice(),
            ),
            (
                ScriptLanguage::Cpp,
                "entry.cpp",
                include_bytes!("../../../examples/api-smoke-test/entry.cpp").as_slice(),
            ),
            (
                ScriptLanguage::CSharp,
                "entry.cs",
                include_bytes!("../../../examples/api-smoke-test/entry.cs").as_slice(),
            ),
            (
                ScriptLanguage::Java,
                "RusticBehavior.java",
                include_bytes!("../../../examples/api-smoke-test/RusticBehavior.java").as_slice(),
            ),
            (
                ScriptLanguage::Php,
                "entry.php",
                include_bytes!("../../../examples/api-smoke-test/entry.php").as_slice(),
            ),
        ];
        for &(language, name, source) in fixtures {
            if !matches!(
                language,
                ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
            ) && !engine_scripting::probe_language_toolchain(language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_API_ADAPTERS").is_none(),
                    "{} toolchain required for smoke tests",
                    language.display_name()
                );
                eprintln!(
                    "SKIPPED API smoke {}: missing toolchain",
                    language.display_name()
                );
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            let mut scene = SceneWorld::new();
            let entity = EntitySnapshot::default();
            let id = entity.id;
            scene
                .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
                .unwrap();
            let world = Arc::new(Mutex::new(scene));
            let mut host = environment_host(&world);
            host.entity = id;
            host.entity_bound = true;
            host.snapshot_root = temp.path().into();
            host.set_translation([3.0, 2.0, 1.0]).unwrap();
            let input = Arc::clone(&host.input_keys);
            let logs = Arc::clone(&host.logs);
            let observer = host.clone();
            let mut behavior =
                RuntimeBehavior::load(language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|error| panic!("{name} load: {error}"));
            behavior
                .on_create()
                .unwrap_or_else(|error| panic!("{name} create: {error}"));
            behavior
                .on_start()
                .unwrap_or_else(|error| panic!("{name} start: {error}"));
            behavior.fixed_update(0.25).unwrap();
            assert_eq!(observer.translation(), [3.0, 2.0, 1.0], "{name} idle");
            input.lock().unwrap().insert("KeyW".into());
            behavior.fixed_update(0.25).unwrap();
            behavior.fixed_update(0.25).unwrap();
            behavior.update(0.25).unwrap();
            assert_eq!(observer.translation(), [3.0, 2.0, 1.5], "{name} held");
            input.lock().unwrap().clear();
            behavior.fixed_update(0.25).unwrap();
            assert_eq!(observer.translation(), [3.0, 2.0, 1.5], "{name} released");
            assert_eq!(
                world.lock().unwrap().entity_ids().count(),
                1,
                "{name} spawned extra objects"
            );
            assert_eq!(
                logs.lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, message)| message == "Behavior started")
                    .count(),
                1,
                "{name} start log"
            );
            behavior.on_stop().unwrap();
            behavior.on_destroy().unwrap();
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one shared fixture harness verifies all ten adapters"
    )]
    fn audio_capture_and_live_reads_conform_in_every_available_adapter() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (
                ScriptLanguage::Lua54,
                "parity.lua",
                include_bytes!("../tests/fixtures/api-parity/parity.lua").as_slice(),
            ),
            (
                ScriptLanguage::Luau,
                "parity.luau",
                include_bytes!("../tests/fixtures/api-parity/parity.luau").as_slice(),
            ),
            (
                ScriptLanguage::JavaScript,
                "parity.js",
                include_bytes!("../tests/fixtures/api-parity/parity.js").as_slice(),
            ),
            (
                ScriptLanguage::Web,
                "parity.html",
                include_bytes!("../tests/fixtures/api-parity/parity.html").as_slice(),
            ),
            (
                ScriptLanguage::Python,
                "parity.py",
                include_bytes!("../tests/fixtures/api-parity/parity.py").as_slice(),
            ),
            (
                ScriptLanguage::C,
                "parity.c",
                include_bytes!("../tests/fixtures/api-parity/parity.c").as_slice(),
            ),
            (
                ScriptLanguage::Cpp,
                "parity.cpp",
                include_bytes!("../tests/fixtures/api-parity/parity.cpp").as_slice(),
            ),
            (
                ScriptLanguage::CSharp,
                "parity.cs",
                include_bytes!("../tests/fixtures/api-parity/parity.cs").as_slice(),
            ),
            (
                ScriptLanguage::Java,
                "parity.java",
                include_bytes!("../tests/fixtures/api-parity/parity.java").as_slice(),
            ),
            (
                ScriptLanguage::Php,
                "parity.php",
                include_bytes!("../tests/fixtures/api-parity/parity.php").as_slice(),
            ),
        ];
        for &(language, name, source) in fixtures {
            if !matches!(
                language,
                ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
            ) && !engine_scripting::probe_language_toolchain(language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_API_ADAPTERS").is_none(),
                    "{} toolchain required for API conformance",
                    language.display_name()
                );
                eprintln!(
                    "SKIPPED API conformance {}: missing toolchain",
                    language.display_name()
                );
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("assets")).unwrap();
            let path = temp.path().join("assets/tone.wav");
            let mut wav = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 8000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            wav.write_sample(1000_i16).unwrap();
            wav.finalize().unwrap();
            let mut world = SceneWorld::new();
            let entity = EntitySnapshot::default();
            let id = entity.id;
            world
                .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
                .unwrap();
            let world = Arc::new(Mutex::new(world));
            let mut host = environment_host(&world);
            host.entity = id;
            host.snapshot_root = temp.path().into();
            let context = Arc::clone(&host.gameplay);
            let mut behavior =
                RuntimeBehavior::load(language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{} load: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} start: {e}", language.display_name()));
            assert_eq!(
                context.lock().unwrap().audio.voices.len(),
                1,
                "{} playback",
                language.display_name()
            );
            std::fs::remove_file(path).unwrap();
            context
                .lock()
                .unwrap()
                .audio_capture
                .submit(&[0.25, -0.25, 0.5, -0.5]);
            behavior
                .update(0.01)
                .unwrap_or_else(|e| panic!("{} update: {e}", language.display_name()));
            assert!(context.lock().unwrap().audio.voices.is_empty());
            let mut wav = hound::WavReader::open(temp.path().join("capture.wav")).unwrap();
            assert_eq!(
                wav.samples::<f32>().collect::<Result<Vec<_>, _>>().unwrap(),
                vec![0.25, -0.25, 0.5, -0.5]
            );
            eprintln!("PASSED API conformance {}", language.display_name());
        }
    }

    #[test]
    fn lua_and_luau_recording_handles_retain_samples_and_save_wav() {
        for language in [ScriptLanguage::Lua54, ScriptLanguage::Luau] {
            let temp = tempfile::tempdir().unwrap();
            let world = Arc::new(Mutex::new(SceneWorld::new()));
            let mut host = environment_host(&world);
            host.snapshot_root = temp.path().into();
            let context = Arc::clone(&host.gameplay);
            let mut behavior = RuntimeBehavior::load(language, ScriptId::new(), br"
                local rec, file
                return {
                    Start=function()
                        assert(not pcall(function() rustic.game:startRecording('bad.mp3') end))
                        assert(not pcall(function() rustic.game:startRecording(nil, {source='bad'}) end))
                        rec=rustic.game:startRecording()
                        file=rustic.game:startRecording('capture.wav')
                    end,
                    Update=function()
                        assert(rec:stopRecording()==rec)
                        rustic.game:stopRecording(file)
                        local packet=rec:read()
                        assert(packet.sampleRate==48000 and packet.channels==2 and not packet.active)
                        assert(#packet.samples==4 and packet.samples[1]==0.25 and packet.samples[4]==-0.5)
                        assert(#rec:read().samples==0)
                        rec:stopRecording()
                        rec:unload()
                        assert(not pcall(function() rec:read() end))
                        file:unload()
                    end
                }
            ", "capture.lua", Box::new(host)).unwrap();
            behavior.on_start().unwrap();
            context
                .lock()
                .unwrap()
                .audio_capture
                .submit(&[0.25, -0.25, 0.5, -0.5]);
            behavior.update(0.01).unwrap();
            let mut wav = hound::WavReader::open(temp.path().join("capture.wav")).unwrap();
            assert_eq!(
                wav.samples::<f32>().collect::<Result<Vec<_>, _>>().unwrap(),
                vec![0.25, -0.25, 0.5, -0.5]
            );
        }
    }

    #[test]
    fn compressed_audio_formats_load_play_and_release_from_lua() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("assets")).unwrap();
        let fixtures: &[(&str, &[u8])] = &[
            (
                "tone.mp3",
                include_bytes!("../../engine-assets/tests/fixtures/audio/tone.mp3"),
            ),
            (
                "tone.aac",
                include_bytes!("../../engine-assets/tests/fixtures/audio/tone.aac"),
            ),
            (
                "tone.m4a",
                include_bytes!("../../engine-assets/tests/fixtures/audio/tone.m4a"),
            ),
            (
                "tone.flac",
                include_bytes!("../../engine-assets/tests/fixtures/audio/tone.flac"),
            ),
            (
                "vorbis.ogg",
                include_bytes!("../../engine-assets/tests/fixtures/audio/vorbis.ogg"),
            ),
            (
                "opus.ogg",
                include_bytes!("../../engine-assets/tests/fixtures/audio/opus.ogg"),
            ),
        ];
        for &(name, bytes) in fixtures {
            std::fs::write(temp.path().join("assets").join(name), bytes).unwrap();
        }
        let world = Arc::new(Mutex::new(SceneWorld::new()));
        let mut host = environment_host(&world);
        host.snapshot_root = temp.path().into();
        let context = Arc::clone(&host.gameplay);
        let mut behavior = LuaBehavior::load(ScriptId::new(), br"
            local tracks={}
            return {Start=function()
                for _,name in ipairs({'tone.mp3','tone.aac','tone.m4a','tone.flac','vorbis.ogg','opus.ogg'}) do
                    local audio=rustic.game.assets[name]:load()
                    audio:play({volume=0.1})
                    tracks[#tracks+1]=audio
                end
            end,Update=function()
                for _,audio in ipairs(tracks) do audio:Unload() end
            end}
        ", "formats.lua", Box::new(host), 100_000).unwrap();
        behavior.on_start().unwrap();
        let mut gameplay = context.lock().unwrap();
        assert_eq!(gameplay.audio.voices.len(), fixtures.len());
        let pcm = gameplay.audio.mix(0.05).unwrap();
        assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
        drop(gameplay);
        behavior.update(0.01).unwrap();
        assert!(context.lock().unwrap().audio.voices.is_empty());
    }

    #[test]
    fn lua_audio_objects_require_load_and_reuse_decoded_audio() {
        audio_objects_require_load_and_reuse_decoded_audio(ScriptLanguage::Lua54);
    }

    #[test]
    fn luau_audio_objects_require_load_and_reuse_decoded_audio() {
        if !engine_scripting::probe_language_toolchain(ScriptLanguage::Luau).available {
            return;
        }
        audio_objects_require_load_and_reuse_decoded_audio(ScriptLanguage::Luau);
    }

    fn audio_objects_require_load_and_reuse_decoded_audio(language: ScriptLanguage) {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("assets/sounds")).unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&38u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&2u32.to_le_bytes());
        wav.extend_from_slice(&1000i16.to_le_bytes());
        let path = temp.path().join("assets/sounds/crash.wav");
        std::fs::write(&path, wav).unwrap();
        let world = Arc::new(Mutex::new(SceneWorld::new()));
        let mut host = environment_host(&world);
        host.snapshot_root = temp.path().into();
        let context = Arc::clone(&host.gameplay);
        let mut behavior = RuntimeBehavior::load(
            language,
            ScriptId::new(),
            br#"
            local crash
            return {Start=function()
                local ok,err=pcall(function() game.rustic.files.assets.sounds["crash.wav"]:play() end)
                assert(not ok and string.find(err,'load',1,true))
                assert(not pcall(function() Audio.load('assets/missing.wav') end))
                assert(not pcall(function() game.rustic.files.assets["../outside.wav"]:load() end))
                local asset=game.rustic.files.assets.sounds["crash.wav"]
                crash=asset:load()
                assert(crash:load()==crash)
                assert(game.rustic.files.assets.sounds['crash.wav']:load()==crash)
                game.rustic.files.assets.sounds['crash.wav']:play({loop=true})
                crash:Unload()
                assert(not pcall(function() crash:play() end))
                assert(crash:load()==crash)
                local other=Audio.load('assets/sounds/crash.wav')
                other:unload()
                crash:play({loop=true})
                crash:pause()
            end,Update=function()
                crash:resume()
                crash:stop()
                crash:play()
                crash:Unload()
                crash:Unload()
                assert(not pcall(function() crash:play() end))
                assert(not pcall(function() crash:load() end))
            end}
        "#,
            "audio.lua",
            Box::new(host),
        )
        .unwrap();
        behavior.on_start().unwrap();
        assert_eq!(context.lock().unwrap().audio.voices.len(), 1);
        assert!(
            context
                .lock()
                .unwrap()
                .audio
                .voices
                .values()
                .next()
                .unwrap()
                .paused
        );
        // Replaying must use retained PCM even if the source file disappears.
        std::fs::remove_file(path).unwrap();
        behavior.update(0.01).unwrap();
        assert!(context.lock().unwrap().audio.voices.is_empty());
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn model_clips_bind_before_start_and_audio_physics_follow_the_scene_clock() {
        let temp = tempfile::tempdir().unwrap();
        for folder in ["assets", "config", "scripts"] {
            std::fs::create_dir_all(temp.path().join(folder)).unwrap();
        }
        let source = temp.path().join("assets/rig.gltf");
        std::fs::write(
            &source,
            include_bytes!("../../engine-assets/tests/fixtures/skinned_animation.gltf"),
        )
        .unwrap();
        let meta = engine_assets::AssetMeta::new("rustic.gltf", 2);
        engine_assets::save_meta(&engine_assets::sidecar_path(&source), &meta).unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 32000).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&32000u32.to_le_bytes());
        for _ in 0..16000 {
            wav.extend_from_slice(&1000i16.to_le_bytes());
        }
        std::fs::write(temp.path().join("assets/tone.wav"), wav).unwrap();
        let script_id = ScriptId::new();
        std::fs::write(temp.path().join("scripts/actor.lua"), br"return {Start=function()
            assert(Animation.clips(nil)[1]=='Slide')
            local hit=Physics.raycast(Vector3(0,2,0),Vector3(0,-1,0),4)
            assert(hit and hit.entity==rustic.entity_id())
            Physics.launch(nil,Vector3(2,0,0))
            Animation.addMarker(nil,'Slide',0.5,'Mid')
            Animation.play(nil,'Slide').onMarker('Mid',function() rustic.log('info','imported marker') end).onFinished(function() rustic.log('info','clip complete') end)
            local voice=Audio.play('assets/tone.wav',{volume=0})
            Audio.fadeIn(voice,1,Ease.InQuad)
            Clock.timeScale(0.5)
            Timer.after(0.5,function() rustic.log('info','scaled timer') end)
        end}").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/actor.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let entity = EntitySnapshot {
            mesh: Some(Mesh {
                asset: meta.asset_id,
            }),
            primitive: Some(engine_world::Primitive::Cube { size: 1. }),
            scripts: vec![ScriptComponent::new(script_id)],
            ..Default::default()
        };
        let mut scene = SceneDocument::new("Rig");
        scene.entities.push(entity.clone());
        let mut runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        runtime.fixed_update(0.1);
        assert!(
            (runtime
                .active
                .world
                .lock()
                .unwrap()
                .local_transform(entity.id)
                .unwrap()
                .translation
                .x
                - 0.1)
                .abs()
                < 1e-5
        );
        runtime.frame_update(1.0);
        let world = runtime.active.world.lock().unwrap();
        let joint = world
            .entity_ids()
            .find(|id| world.snapshot(*id).unwrap().name.as_deref() == Some("node_1"))
            .unwrap();
        assert_eq!(world.local_transform(joint).unwrap().translation.x, 2.0);
        drop(world);
        let context = runtime.active.gameplay.lock().unwrap();
        assert_eq!(context.audio.voices.len(), 1);
        assert!((context.audio.voices.values().next().unwrap().volume - 0.25).abs() < 1e-9);
        drop(context);
        assert!(
            runtime
                .active
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|(_, message)| message == "scaled timer")
        );
        assert!(
            runtime
                .active
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|(_, message)| message == "imported marker")
        );
        runtime.stop();
        assert!(
            runtime
                .active
                .gameplay
                .lock()
                .unwrap()
                .audio
                .voices
                .is_empty()
        );
        assert!(
            runtime
                .active
                .gameplay
                .lock()
                .unwrap()
                .runtime
                .states_for_entities()
                .is_empty()
        );
    }

    #[test]
    fn physics_runs_without_scripts_and_resets_with_new_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let mut scene = SceneDocument::new("Physics");
        let mut floor = EntitySnapshot::default();
        floor.primitive = Some(engine_world::Primitive::RectangularPrism {
            size: [20.0, 1.0, 20.0],
        });
        floor.part_attributes.anchored = true;
        let mut falling = EntitySnapshot::default();
        falling.primitive = Some(engine_world::Primitive::Cube { size: 1.0 });
        falling.local_transform.translation.y = 5.0;
        let id = falling.id;
        scene.entities = vec![floor, falling];
        let bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &bytes).unwrap();
        for _ in 0..180 {
            runtime.fixed_update(1.0 / 60.0);
        }
        assert!((runtime.live_entity(id).unwrap().unwrap().translation[1] - 1.0).abs() < 0.001);
        let restarted = RuntimeScripts::load(temp.path(), &bytes).unwrap();
        assert_eq!(
            restarted.live_entity(id).unwrap().unwrap().translation[1],
            5.0
        );
    }

    #[test]
    fn script_attribute_edits_affect_physics_in_same_fixed_tick() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let mut entity = EntitySnapshot::default();
        entity.primitive = Some(engine_world::Primitive::Cube { size: 1.0 });
        entity.part_attributes.anchored = true;
        entity.scripts.push(ScriptComponent::new(script_id));
        let id = entity.id;
        let mut scene = SceneDocument::new("Physics");
        scene.entities.push(entity);
        std::fs::write(
            temp.path().join("scripts/release.lua"),
            b"return { FixedUpdate=function() rustic.EditAttribute('Anchored',false) end }",
        )
        .unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/release.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        runtime.fixed_update(1.0 / 60.0);
        assert!(runtime.live_entity(id).unwrap().unwrap().translation[1] < 0.0);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn named_scene_object_calls_mutate_target_in_lua_and_javascript() {
        let root = EntitySnapshot {
            name: Some("Room".into()),
            ..Default::default()
        };
        let target = EntitySnapshot {
            name: Some("Player".into()),
            parent: Some(root.id),
            ..Default::default()
        };
        let owner = EntitySnapshot {
            name: Some("Controller".into()),
            ..Default::default()
        };
        let mut scene = SceneDocument::new("Demo");
        scene.entities = vec![root, target.clone(), owner.clone()];
        let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
        let make_host = || RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "Demo".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: owner.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
            scenes: Arc::default(),
            global: false,
        };
        let mut lua = LuaBehavior::load(ScriptId::new(),
            b"return {Start=function() rustic.game.Demo.Room.Player:EditAttribute('Position',{1,2,3}); assert(rustic.game.Demo.Room.Player:GetAttribute('Position')[1]==1) end}",
            "target.lua", Box::new(make_host()), 100_000).unwrap();
        lua.on_start().unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(target.id)
                .unwrap()
                .translation
                .to_array(),
            [1., 2., 3.]
        );
        let mut js = JavaScriptBehavior::load(ScriptId::new(),
            b"globalThis.behavior={Start(){rustic.game.Demo.Room.Player.EditAttribute('Color',[1,0,0]);rustic.game['Demo']['Room']['Player'].EditAttribute('Anchored',true);}};",
            "target.js", Box::new(make_host()), 100_000).unwrap();
        js.on_start().unwrap();
        assert!(
            world
                .lock()
                .unwrap()
                .part_attributes(target.id)
                .unwrap()
                .anchored
        );
        assert_eq!(
            world
                .lock()
                .unwrap()
                .part_attributes(target.id)
                .unwrap()
                .color[..3],
            [1., 0., 0.]
        );
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(owner.id)
                .unwrap()
                .translation
                .to_array(),
            [0.; 3]
        );
        let mut web = WebBehavior::load(ScriptId::new(),
            br"<!doctype html><html><body><script>globalThis.behavior={Start(){rustic.game.Demo.Room.Player.EditAttribute('Size',[2,3,4]);}};</script></body></html>",
            "ui/target.html", Box::new(make_host()),100_000).unwrap();
        web.on_start().unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(target.id)
                .unwrap()
                .scale
                .to_array(),
            [2., 3., 4.]
        );
        let mut host = make_host();
        for source in [
            "rustic.game.Other.Room.Player",
            "rustic.game.Demo.Room.Missing",
            "game.scene.Other.Room.Player",
            "game.scene.Demo.Room.Missing",
        ] {
            assert!(
                host.edit_object_attribute(source, "Anchored", EngineValue::Boolean(false))
                    .is_err()
            );
        }
        assert!(
            host.edit_object_attribute(
                "rustic.game.Demo.Room.Player",
                "Position",
                EngineValue::Boolean(true)
            )
            .is_err()
        );
        host.edit_object_attribute(
            "rustic.game.Demo.Room.Player",
            "Name",
            EngineValue::String("Renamed".into()),
        )
        .unwrap();
        assert!(
            host.object_attribute("rustic.game.Demo.Room.Player", "Name")
                .is_err()
        );
        assert_eq!(
            host.object_attribute("rustic.game.Demo.Room.Renamed", "Name")
                .unwrap(),
            Some(EngineValue::String("Renamed".into()))
        );
        assert_eq!(
            host.object_attribute("game.scene.Demo.Room.Renamed", "Name")
                .unwrap(),
            Some(EngineValue::String("Renamed".into()))
        );
        assert!(
            host.object_attribute("game.scene.Demo.Room.Player", "Name")
                .is_err()
        );
        let duplicate = EntitySnapshot {
            name: Some("Renamed".into()),
            parent: target.parent,
            ..Default::default()
        };
        world
            .lock()
            .unwrap()
            .apply_commands(&[WorldCommand::Spawn(Box::new(duplicate))])
            .unwrap();
        assert!(
            host.object_attribute("rustic.game.Demo.Room.Renamed", "Name")
                .unwrap_err()
                .contains("ambiguous")
        );
        assert!(
            host.object_attribute("game.scene.Demo.Room.Renamed", "Name")
                .unwrap_err()
                .contains("ambiguous")
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "cross-language fixtures exercise the same live world contract"
    )]
    fn entity_foundations_round_trip_through_available_language_adapters() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54, "entities.lua", br"return {Start=function()
                local e=rustic.entity_id()
                assert(Tags.add(e,'enemy'));assert(not Tags.add(e,'enemy'))
                assert(#Tags.find_all('enemy')==1);assert(#World.find_by_tag('enemy')==1)
                assert(Groups.create('enemies'));assert(Groups.add('enemies',e));assert(#Groups.get_members('enemies')==1)
                assert(Components.has(e,'rustic.transform'));assert(#Components.get_all(e)>=2)
                assert(#Components.find_in_children(e,'rustic.transform')==0)
                assert(#Hierarchy.get_children(e)==0);Hierarchy.set_parent(e,nil);Hierarchy.detach(e)
                assert(#World.find_in_radius({0,0,0},1)==1);assert(Entity.is_valid(e))
                assert(Tags.remove(e,'enemy'));assert(not Tags.remove(e,'enemy'));Tags.add(e,'enemy')
                Entity.destroy(Tags.find_all('disposable')[1]);Entity.destroy_after(e,1)
            end}"),
            (ScriptLanguage::Luau, "entities.luau", br"return {Start=function()
                local e=rustic.entity_id();assert(Tags.add(e,'enemy'));assert(#Tags.find_all('enemy')==1)
                assert(Groups.create('enemies'));assert(Groups.add('enemies',e));assert(#Groups.get_members('enemies')==1)
                assert(Components.has(e,'rustic.transform'));Hierarchy.detach(e);assert(Entity.is_valid(e));Entity.destroy(Tags.find_all('disposable')[1]);Entity.destroy_after(e,1)
            end}"),
            (ScriptLanguage::JavaScript, "entities.js", br"globalThis.behavior.Start=function(){
                const e=rustic.entity_id();if(!Tags.add(e,'enemy')||Tags.add(e,'enemy'))throw Error('tags');
                if(Tags.find_all('enemy').length!==1||World.find_by_tag('enemy').length!==1)throw Error('find');
                Groups.create('enemies');Groups.add('enemies',e);if(Groups.get_members('enemies').length!==1)throw Error('group');
                if(!Components.has(e,'rustic.transform')||Components.get_all(e).length<2||Components.find_in_children(e,'rustic.transform').length)throw Error('components');
                Hierarchy.set_parent(e,null);Hierarchy.detach(e);if(Hierarchy.get_children(e).length)throw Error('children');
                if(World.find_in_radius([0,0,0],1).length!==1||!Entity.is_valid(e))throw Error('world');
                if(!Tags.remove(e,'enemy')||Tags.remove(e,'enemy'))throw Error('remove');Tags.add(e,'enemy');Entity.destroy(Tags.find_all('disposable')[0]);Entity.destroy_after(e,1);
            };"),
            (ScriptLanguage::Web, "entities.html", br"<!doctype html><html><body><script>globalThis.behavior.Start=function(){const e=rustic.entity_id();Tags.add(e,'enemy');Groups.create('enemies');Groups.add('enemies',e);if(!Entity.is_valid(e))throw Error('valid');Entity.destroy(Tags.find_all('disposable')[0]);Entity.destroy_after(e,1);};</script></body></html>"),
            (ScriptLanguage::Python, "entities.py", br"from rustic import *
def on_start():
    e=rustic.entity_id()
    assert Tags.add(e,'enemy') and not Tags.add(e,'enemy')
    assert len(Tags.find_all('enemy'))==1 and len(World.find_by_tag('enemy'))==1
    assert Groups.create('enemies') and Groups.add('enemies',e)
    assert len(Groups.get_members('enemies'))==1
    assert Components.has(e,'rustic.transform') and len(Components.get_all(e))>=2
    assert not Components.find_in_children(e,'rustic.transform')
    Hierarchy.set_parent(e,None);Hierarchy.detach(e)
    assert not Hierarchy.get_children(e) and Entity.is_valid(e)
    assert len(World.find_in_radius([0,0,0],1))==1
    assert Tags.remove(e,'enemy') and not Tags.remove(e,'enemy')
    Tags.add(e,'enemy');Entity.destroy(Tags.find_all('disposable')[0]);Entity.destroy_after(e,1)
run(globals())"),
            (ScriptLanguage::C, "entities.c", br#"#include "rustic.h"
void start(void){const char *e=rustic.entity_id();
if(!Tags.add(e,"enemy")||Tags.add(e,"enemy"))r_fail("tags");
if(Tags.find_all("enemy").count!=1||World.find_by_tag("enemy").count!=1)r_fail("find");
Groups.create("enemies");Groups.add("enemies",e);if(Groups.get_members("enemies").count!=1)r_fail("groups");
if(!Components.has(e,"rustic.transform")||Components.get_all(e).count<2||Components.find_in_children(e,"rustic.transform").count)r_fail("components");
Hierarchy.set_parent(e,NULL,true);Hierarchy.detach(e,true);if(Hierarchy.get_children(e).count||!Entity.is_valid(e))r_fail("hierarchy");
if(World.find_in_radius((RusticVector3){0,0,0},1).count!=1)r_fail("radius");
if(!Tags.remove(e,"enemy")||Tags.remove(e,"enemy")){r_fail("remove");}
Tags.add(e,"enemy");Entity.destroy(Tags.find_all("disposable").items[0]);Entity.destroy_after(e,1);}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::Cpp, "entities.cpp", br#"#include "rustic.hpp"
void start(){auto e=rustic.entity_id();
if(!Tags::add(e,"enemy")||Tags::add(e,"enemy"))throw std::runtime_error("tags");
if(Tags::find_all("enemy").size()!=1||World::find_by_tag("enemy").size()!=1)throw std::runtime_error("find");
Groups::create("enemies");Groups::add("enemies",e);if(Groups::get_members("enemies").size()!=1)throw std::runtime_error("groups");
if(!Components::has(e,"rustic.transform")||Components::get_all(e).size()<2||!Components::find_in_children(e,"rustic.transform").empty())throw std::runtime_error("components");
Hierarchy::set_parent(e,"");Hierarchy::detach(e);if(!Hierarchy::get_children(e).empty()||!Entity::is_valid(e))throw std::runtime_error("hierarchy");
if(World::find_in_radius({0,0,0},1).size()!=1)throw std::runtime_error("radius");
if(!Tags::remove(e,"enemy")||Tags::remove(e,"enemy")){throw std::runtime_error("remove");}
Tags::add(e,"enemy");Entity::destroy(Tags::find_all("disposable").front());Entity::destroy_after(e,1);}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::CSharp, "entities.cs", br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){var e=rustic.entity_id();
if(!Tags.add(e,"enemy")||Tags.add(e,"enemy"))throw new System.Exception("tags");
if(Tags.find_all("enemy").Length!=1||World.find_by_tag("enemy").Length!=1)throw new System.Exception("find");
Groups.create("enemies");Groups.add("enemies",e);if(Groups.get_members("enemies").Length!=1)throw new System.Exception("groups");
if(!Components.has(e,"rustic.transform")||Components.get_all(e).Length<2||Components.find_in_children(e,"rustic.transform").Length!=0)throw new System.Exception("components");
Hierarchy.set_parent(e,null);Hierarchy.detach(e);if(Hierarchy.get_children(e).Length!=0||!Entity.is_valid(e))throw new System.Exception("hierarchy");
if(World.find_in_radius(new double[]{0,0,0},1).Length!=1)throw new System.Exception("radius");
Tags.remove(e,"enemy");Tags.add(e,"enemy");Entity.destroy(Tags.find_all("disposable")[0]);Entity.destroy_after(e,1);}});"#),
            (ScriptLanguage::Java, "RusticBehavior.java", br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){String e=rustic.entity_id();
if(!Tags.add(e,"enemy")||Tags.add(e,"enemy"))throw new RuntimeException("tags");
if(Tags.find_all("enemy").length!=1||World.find_by_tag("enemy").length!=1)throw new RuntimeException("find");
Groups.create("enemies");Groups.add("enemies",e);if(Groups.get_members("enemies").length!=1)throw new RuntimeException("groups");
if(!Components.has(e,"rustic.transform")||Components.get_all(e).length<2||Components.find_in_children(e,"rustic.transform").length!=0)throw new RuntimeException("components");
Hierarchy.set_parent(e,null);Hierarchy.detach(e);if(Hierarchy.get_children(e).length!=0||!Entity.is_valid(e))throw new RuntimeException("hierarchy");
if(World.find_in_radius(new double[]{0,0,0},1).length!=1)throw new RuntimeException("radius");
Tags.remove(e,"enemy");Tags.add(e,"enemy");Entity.destroy(Tags.find_all("disposable")[0]);Entity.destroy_after(e,1);}});}}"#),
            (ScriptLanguage::Php, "entities.php", br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $rustic;$e=$rustic->entity_id();
if(!Tags::add($e,'enemy')||Tags::add($e,'enemy'))throw new Exception('tags');
if(count(Tags::find_all('enemy'))!==1||count(World::find_by_tag('enemy'))!==1)throw new Exception('find');
Groups::create('enemies');Groups::add('enemies',$e);if(count(Groups::get_members('enemies'))!==1)throw new Exception('groups');
if(!Components::has($e,'rustic.transform')||count(Components::get_all($e))<2||count(Components::find_in_children($e,'rustic.transform')))throw new Exception('components');
Hierarchy::set_parent($e,null);Hierarchy::detach($e);if(count(Hierarchy::get_children($e))||!Entity::is_valid($e))throw new Exception('hierarchy');
if(count(World::find_in_radius([0,0,0],1))!==1)throw new Exception('radius');
Tags::remove($e,'enemy');Tags::add($e,'enemy');Entity::destroy(Tags::find_all("disposable")[0]);Entity::destroy_after($e,1);}
rustic_run(['on_start'=>'on_start']);"#),
        ];
        for (language, name, source) in fixtures {
            if !matches!(
                language,
                ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
            ) && !engine_scripting::probe_language_toolchain(*language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_ENTITY_ADAPTERS").is_none(),
                    "{} toolchain is required for complete entity adapter conformance",
                    language.display_name()
                );
                eprintln!(
                    "SKIPPED entity foundations {}: missing toolchain",
                    language.display_name()
                );
                continue;
            }
            let mut world = SceneWorld::new();
            let entity = engine_world::EntitySnapshot::default();
            let id = entity.id;
            world
                .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
                .unwrap();
            let disposable = engine_world::EntitySnapshot {
                local_transform: engine_world::LocalTransform {
                    translation: glam::Vec3::new(100., 0., 0.),
                    ..engine_world::LocalTransform::IDENTITY
                },
                ..engine_world::EntitySnapshot::default()
            };
            let disposable_id = disposable.id;
            world
                .apply_commands(&[WorldCommand::Spawn(Box::new(disposable))])
                .unwrap();
            world.add_tag(disposable_id, "disposable").unwrap();
            let world = Arc::new(Mutex::new(world));
            let mut host = environment_host(&world);
            host.entity = id;
            let mut behavior =
                RuntimeBehavior::load(*language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{} load: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} start: {e}", language.display_name()));
            let mut world = world.lock().unwrap();
            assert!(
                !world.contains(disposable_id),
                "{} destroy binding",
                language.display_name()
            );
            assert!(world.find_by_tag("disposable").unwrap().is_empty());
            assert_eq!(world.find_by_tag("enemy").unwrap(), vec![id]);
            assert_eq!(world.group_members("enemies").unwrap(), vec![id]);
            world.tick_entity_lifetimes(0.5).unwrap();
            assert!(world.contains(id));
            world.tick_entity_lifetimes(0.5).unwrap();
            assert!(!world.contains(id));
            assert!(world.group_members("enemies").unwrap().is_empty());
            eprintln!("PASSED entity foundations {}", language.display_name());
        }
    }

    #[test]
    fn destroying_an_owner_cleans_callbacks_and_invokes_destroy_once() {
        let mut world = SceneWorld::new();
        let entity = engine_world::EntitySnapshot::default();
        let id = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        let world = Arc::new(Mutex::new(world));
        let mut host = environment_host(&world);
        host.entity = id;
        host.entity_bound = true;
        let owner = Owner {
            entity_bound: true,
            entity: id,
            script: host.owner_script,
            scene: host.gameplay.lock().unwrap().scene,
        };
        let gameplay = Arc::clone(&host.gameplay);
        let logs = Arc::clone(&host.logs);
        let mut behavior=RuntimeBehavior::load(ScriptLanguage::Lua54, ScriptId::new(), br"
            return {Start=function() Events.on('retained',function() end);Timer.after(100,function() end) end,
              FixedUpdate=function() error('dead owner was called') end,
              OnDestroy=function() rustic.log('info','destroyed once') end}
        ", "lifetime.lua",Box::new(host)).unwrap();
        behavior.on_start().unwrap();
        let mut scripts = SceneRuntime {
            gameplay,
            behaviors: vec![behavior],
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            logs: Arc::clone(&logs),
            dropped_logs: Arc::new(Mutex::new(0)),
            valid_scene: true,
            name: "Test".into(),
            global_behaviors: Vec::new(),
        };
        world.lock().unwrap().destroy_entity(id).unwrap();
        scripts.fixed_update(0.01);
        scripts.fixed_update(0.01);
        assert!(scripts.behaviors.is_empty());
        let context = scripts.gameplay.lock().unwrap();
        assert!(context.runtime.states(owner).is_empty());
        assert!(context.runtime.signals.tokens(owner).is_empty());
        drop(context);
        let logs = logs.lock().unwrap();
        assert_eq!(
            logs.iter()
                .filter(|(_, message)| message == "destroyed once")
                .count(),
            1
        );
        assert!(!logs.iter().any(|(level, _)| level == "error"));
    }

    #[test]
    fn documented_entity_foundation_examples_execute_in_the_live_world() {
        for (language, name, source) in [
            (
                ScriptLanguage::Lua54,
                "behavior.lua",
                include_bytes!("../../../examples/entity-foundations/behavior.lua").as_slice(),
            ),
            (
                ScriptLanguage::Python,
                "behavior.py",
                include_bytes!("../../../examples/entity-foundations/behavior.py").as_slice(),
            ),
        ] {
            if language == ScriptLanguage::Python
                && !engine_scripting::probe_language_toolchain(language).available
            {
                eprintln!("SKIPPED documented Python example: missing toolchain");
                continue;
            }
            let mut world = SceneWorld::new();
            let entity = engine_world::EntitySnapshot::default();
            let id = entity.id;
            world
                .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
                .unwrap();
            let world = Arc::new(Mutex::new(world));
            let mut host = environment_host(&world);
            host.entity = id;
            let logs = Arc::clone(&host.logs);
            let mut behavior =
                RuntimeBehavior::load(language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap();
            behavior.on_start().unwrap();
            assert!(
                logs.lock()
                    .unwrap()
                    .iter()
                    .any(|(_, message)| message == "entity foundations ready")
            );
            let mut world = world.lock().unwrap();
            assert_eq!(world.find_by_tag("enemy").unwrap(), vec![id]);
            assert_eq!(world.group_members("enemies").unwrap(), vec![id]);
            world.tick_entity_lifetimes(1.).unwrap();
            assert!(!world.contains(id));
            assert!(world.group_members("enemies").unwrap().is_empty());
        }
    }

    fn environment_host(world: &Arc<Mutex<SceneWorld>>) -> RuntimeHost {
        RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: false,
            scene_name: "Environment".into(),
            world: Arc::clone(world),
            entity: EntityId::new(),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
            scenes: Arc::default(),
            global: false,
        }
    }

    #[test]
    fn audio_device_queries_validate_ids_and_report_headless_hosts() {
        let world = Arc::new(Mutex::new(SceneWorld::new()));
        let mut host = environment_host(&world);
        for direction in ["input", "output"] {
            assert_eq!(
                host.gameplay_query(
                    serde_json::json!({"op":format!("audio_get_{direction}_devices")})
                )
                .unwrap(),
                serde_json::json!([])
            );
            for invalid in [
                serde_json::json!(42),
                serde_json::json!({"id":"device"}),
                serde_json::json!([]),
            ] {
                let error = host.gameplay_query(serde_json::json!({"op":format!("audio_set_{direction}_device"),"device":invalid})).unwrap_err();
                assert!(error.contains("ID string or null"), "{error}");
            }
            for id in [serde_json::Value::Null, serde_json::json!("missing")] {
                let error = host.gameplay_query(serde_json::json!({"op":format!("audio_set_{direction}_device"),"device":id})).unwrap_err();
                assert!(error.contains("unavailable"), "{error}");
            }
        }
    }

    #[test]
    fn environment_patches_validate_atomically_and_preserve_other_settings() {
        let world = Arc::new(Mutex::new(SceneWorld::new()));
        let mut host = environment_host(&world);
        let settings = serde_json::json!({
            "enabled":true, "sky_image":"assets/day.hdr", "sky_color":[0.1,0.2,0.3],
            "rotation_degrees":45, "exposure":2, "ambient_color":[0.4,0.5,0.6],
            "ambient_intensity":0.5, "sun_color":[1,0.9,0.8], "sun_intensity":2,
            "sun_direction":[1,2,3], "haze_color":[0.2,0.3,0.4],
            "haze_density":0.02, "haze_start":20
        });
        let expected: engine_world::SceneEnvironment =
            serde_json::from_value(settings.clone()).unwrap();
        let expected = serde_json::to_value(expected).unwrap();
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":settings}))
                .unwrap(),
            expected
        );
        let before = world.lock().unwrap().environment().clone();
        for patch in [
            serde_json::json!({"enabled":false,"ambient_intensity":-1}),
            serde_json::json!({"exposure":21}),
            serde_json::json!({"sun_direction":[0,0,0]}),
            serde_json::json!({"sky_color":[1,2]}),
            serde_json::json!({"sky_image":"../escape.hdr"}),
            serde_json::json!({"sky_image":"/outside.hdr"}),
            serde_json::json!({"sky_image":"C:\\outside.hdr"}),
            serde_json::json!({"sky_image":null}),
            serde_json::json!({"enabled":1}),
            serde_json::json!({"haze_start":"20"}),
            serde_json::json!({"sun_intensity":1e100}),
            serde_json::json!({"unknown":1}),
            serde_json::json!([]),
            serde_json::json!(null),
        ] {
            assert!(
                host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":patch}))
                    .is_err()
            );
            assert_eq!(world.lock().unwrap().environment(), &before);
        }
        let updated = host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":{"sky_image":"assets/night.hdr"}})).unwrap();
        assert_eq!(updated["sky_image"], "assets/night.hdr");
        assert_eq!(updated["sun_intensity"], expected["sun_intensity"]);
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"scene_environment"}))
                .unwrap(),
            updated
        );
        assert_eq!(
            host.gameplay_query(
                serde_json::json!({"op":"scene_environment_field","field":"sky_image"})
            )
            .unwrap(),
            "assets/night.hdr"
        );
        assert!(
            host.gameplay_query(
                serde_json::json!({"op":"scene_environment_field","field":"unknown"})
            )
            .is_err()
        );
        let captured = SceneDocument::from_world(
            engine_world::SceneId::new(),
            "Environment",
            &world.lock().unwrap(),
        )
        .unwrap();
        assert_eq!(captured.environment.sky_image, "assets/night.hdr");
        let cleared = host
            .gameplay_query(serde_json::json!({"op":"scene_environment",
            "settings":{"sky_image":"","enabled":false}}))
            .unwrap();
        assert_eq!(cleared["sky_image"], "");
        assert_eq!(cleared["enabled"], false);
        assert_eq!(cleared["sun_intensity"], expected["sun_intensity"]);
    }

    #[test]
    fn scene_environment_apis_work_through_all_available_script_adapters() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54, "sky.lua", br"return {Start=function()
                Game.scene.setEnvironment({enabled=true,sun_intensity=2})
                Game.scene.setSkyTexture('assets/night.hdr')
                assert(Game.scene.getEnvironment().sky_image=='assets/night.hdr')
                assert(Game.scene.setEnvironment({}).sun_intensity==2)
                assert(not pcall(function() Game.scene.setEnvironment({enabled=false,exposure=21}) end))
                assert(Game.scene.getEnvironment().enabled)
            end}"),
            (ScriptLanguage::JavaScript, "sky.js", br"globalThis.behavior={Start(){
                Game.scene.setEnvironment({enabled:true,sun_intensity:2});
                Game.scene.setSkyTexture('assets/night.hdr');
                if(Game.scene.getEnvironment().sky_image!=='assets/night.hdr')throw Error('stale sky');
                let rejected=false;try{Game.scene.setEnvironment({enabled:false,exposure:21});}catch(e){rejected=true;}
                if(!rejected || !Game.scene.getEnvironment().enabled)throw Error('invalid patch committed');
            }};"),
            (ScriptLanguage::Web, "sky.html", br"<!doctype html><html><head><title>Sky</title></head><body><script>globalThis.behavior={Start(){Game.scene.setEnvironment({enabled:true,sun_intensity:2});Game.scene.setSkyTexture('assets/night.hdr');if(Game.scene.getEnvironment().sun_intensity!==2)throw Error('stale lighting');}};</script></body></html>"),
            (ScriptLanguage::Python, "sky.py", br#"from rustic import Game,run
def on_start():
    Game.scene.setEnvironment({"enabled":True,"sun_intensity":2})
    Game.scene.setSkyTexture("assets/night.hdr")
    assert Game.scene.getEnvironment()["sky_image"]=="assets/night.hdr"
run(globals())"#),
            (ScriptLanguage::Luau, "sky.luau", br"return {Start=function() Game.scene.setEnvironment({enabled=true,sun_intensity=2});Game.scene.setSkyTexture('assets/night.hdr');assert(Game.scene.getEnvironment().sun_intensity==2) end}"),
            (ScriptLanguage::Cpp, "sky.cpp", br#"#include "rustic.hpp"
void start(){Game.scene.setEnvironment({{"enabled",true},{"sun_intensity",2.0}});Game.scene.setSkyTexture("assets/night.hdr");if(Game.scene.getEnvironment().at("sky_image").string()!="assets/night.hdr")throw std::runtime_error("stale sky");}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C, "sky.c", br#"#include "rustic.h"
void start(void){Game.scene.setEnvironment("enabled",(RusticValue){.type=RUSTIC_BOOL,.boolean=true});Game.scene.setEnvironment("sun_intensity",(RusticValue){.type=RUSTIC_NUMBER,.number=2});Game.scene.setSkyTexture("assets/night.hdr");if(strcmp(Game.scene.getEnvironment("sky_image").string,"assets/night.hdr"))r_fail("stale sky");}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp, "sky.cs", br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){Game.scene.setEnvironment(new {enabled=true,sun_intensity=2});Game.scene.setSkyTexture("assets/night.hdr");if(Game.scene.getEnvironment().GetProperty("sky_image").GetString()!="assets/night.hdr")throw new System.Exception("stale sky");}});"#),
            (ScriptLanguage::Java, "RusticBehavior.java", br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){Game.scene.setEnvironment(java.util.Map.of("enabled",true,"sun_intensity",2));Game.scene.setSkyTexture("assets/night.hdr");if(!((java.util.Map<?,?>)Game.scene.getEnvironment()).get("sky_image").equals("assets/night.hdr"))throw new RuntimeException("stale sky");}});}}"#),
            (ScriptLanguage::Php, "sky.php", br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $Game;$Game->scene->setEnvironment(["enabled"=>true,"sun_intensity"=>2]);$Game->scene->setSkyTexture("assets/night.hdr");if($Game->scene->getEnvironment()["sky_image"]!=="assets/night.hdr")throw new Exception("stale sky");}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(*language).available {
                eprintln!("skipped {}: toolchain unavailable", language.display_name());
                continue;
            }
            let world = Arc::new(Mutex::new(SceneWorld::new()));
            let mut behavior = RuntimeBehavior::load(
                *language,
                ScriptId::new(),
                source,
                name,
                Box::new(environment_host(&world)),
            )
            .unwrap();
            behavior.on_start().unwrap();
            let world = world.lock().unwrap();
            assert!(world.environment().enabled, "{}", language.display_name());
            assert_eq!(world.environment().sun_intensity, 2.0);
            assert_eq!(world.environment().sky_image, "assets/night.hdr");
            assert_eq!(world.environment().ambient_intensity, 0.12);
            eprintln!("validated environment API: {}", language.display_name());
        }
    }

    #[test]
    fn named_scene_object_edits_work_through_available_external_sdks() {
        let fixtures: &[(ScriptLanguage,&str,&[u8])] = &[
            (ScriptLanguage::Python,"target.py",br#"from rustic import rustic, run
def on_start(): rustic.game.Demo.Player.EditAttribute("Position",[1,2,3])
run(globals())"#),
            (ScriptLanguage::Luau,"target.luau",br#"return {Start=function() rustic.game.Demo.Player:EditAttribute("Position",{1,2,3}) end}"#),
            (ScriptLanguage::Cpp,"target.cpp",br#"#include "rustic.hpp"
void start(){rustic.game["Demo"]["Player"].EditAttribute("Position",RusticValue::Array{1.0,2.0,3.0});}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C,"target.c",br#"#include "rustic.h"
void start(void){rustic.game.EditAttribute("Demo","Player","Position",(RusticValue){.type=RUSTIC_VECTOR,.vector={1,2,3},.length=3});}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp,"target.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start")rustic.game.Demo.Player.EditAttribute("Position",new double[]{1,2,3});});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start"))rustic.game.scene("Demo").object("Player").EditAttribute("Position",new double[]{1,2,3});});}}"#),
            (ScriptLanguage::Php,"target.php",br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $rustic;$rustic->game->Demo->Player->EditAttribute("Position",[1,2,3]);}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(*language).available {
                eprintln!("skipped {}: toolchain unavailable", language.display_name());
                continue;
            }
            let target = EntitySnapshot {
                name: Some("Player".into()),
                ..Default::default()
            };
            let owner = EntitySnapshot {
                name: Some("Controller".into()),
                ..Default::default()
            };
            let mut scene = SceneDocument::new("Demo");
            scene.entities = vec![target.clone(), owner.clone()];
            let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
            let host = RuntimeHost {
                gameplay: Arc::new(Mutex::new(GameplayContext::default())),
                owner_script: ScriptId::new(),
                entity_bound: true,
                scene_name: "Demo".into(),
                world: Arc::clone(&world),
                entity: owner.id,
                input_keys: Arc::new(Mutex::new(BTreeSet::new())),
                snapshot_root: PathBuf::new(),
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::new(Mutex::new(VecDeque::new())),
                dropped_logs: Arc::new(Mutex::new(0)),
                enabled: true,
                scenes: Arc::default(),
                global: false,
            };
            let mut behavior =
                ExternalBehavior::load(ScriptId::new(), *language, source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{} load: {e}", language.display_name()));
            behavior
                .on_create()
                .unwrap_or_else(|e| panic!("{} create: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} start: {e}", language.display_name()));
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(target.id)
                    .unwrap()
                    .translation
                    .to_array(),
                [1., 2., 3.],
                "{}",
                language.display_name()
            );
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(owner.id)
                    .unwrap()
                    .translation
                    .to_array(),
                [0.; 3]
            );
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn sdk_camera_selection_changes_render_camera_and_rejects_invalid_targets() {
        let first = EntitySnapshot {
            name: Some("First".into()),
            camera: Some(engine_world::Camera {
                order: 100,
                ..Default::default()
            }),
            ..Default::default()
        };
        let second = EntitySnapshot {
            name: Some("Second".into()),
            parent: Some(first.id),
            camera: Some(engine_world::Camera {
                active: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let part = EntitySnapshot::default();
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(first.clone())),
                WorldCommand::Spawn(Box::new(second.clone())),
                WorldCommand::Spawn(Box::new(part.clone())),
            ])
            .unwrap();
        let world = Arc::new(Mutex::new(world));
        let make_host = || RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "TestScene".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: part.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
            scenes: Arc::default(),
            global: false,
        };
        let mut lua = LuaBehavior::load(
            ScriptId::new(),
            b"return {on_start=function() Game.setCurrentCamera({'Game.scene.First.Second'}) end}",
            "camera.lua",
            Box::new(make_host()),
            100_000,
        )
        .unwrap();
        lua.on_start().unwrap();
        let mut buffer = engine_world::RenderWorldBuffer::new();
        {
            let world = world.lock().unwrap();
            let render = buffer.extract(&world);
            assert_eq!(render.cameras.len(), 1);
            assert_eq!(render.cameras[0].entity, second.id);
        }
        let mut host = make_host();
        for invalid in [
            "Missing".to_owned(),
            part.id.to_string(),
            EntityId::new().to_string(),
        ] {
            assert!(host.set_current_camera(&invalid).is_err());
            assert!(
                world
                    .lock()
                    .unwrap()
                    .camera(second.id)
                    .unwrap()
                    .unwrap()
                    .active
            );
        }
        let mut js = JavaScriptBehavior::load(
            ScriptId::new(),
            b"globalThis.behavior={on_start(){Game.setCurrentCamera('First');}};",
            "camera.js",
            Box::new(make_host()),
            100_000,
        )
        .unwrap();
        js.on_start().unwrap();
        assert!(
            world
                .lock()
                .unwrap()
                .camera(first.id)
                .unwrap()
                .unwrap()
                .active
        );
        assert!(
            !world
                .lock()
                .unwrap()
                .camera(second.id)
                .unwrap()
                .unwrap()
                .active
        );
        host.set_current_camera(&second.id.to_string()).unwrap();
        host.set_current_camera(&second.id.to_string()).unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .camera(first.id)
                .unwrap()
                .unwrap()
                .order,
            100
        );
    }

    #[test]
    fn runtime_changes_skip_destroyed_originals_and_keep_survivor_edits() {
        let temp = tempfile::tempdir().unwrap();
        let mut scene = SceneDocument::new("Review");
        let root = EntitySnapshot::default();
        let child = EntitySnapshot {
            parent: Some(root.id),
            ..EntitySnapshot::default()
        };
        let survivor = EntitySnapshot::default();
        let (root_id, survivor_id) = (root.id, survivor.id);
        scene.entities = vec![root, child, survivor];
        let scene_bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        let mut host = environment_host(&runtime.active.world);
        host.gameplay_query(serde_json::json!({"op":"entity_destroy","entity":root_id}))
            .unwrap();
        {
            let mut world = runtime.active.world.lock().unwrap();
            world
                .apply_commands(&[WorldCommand::SetLocalTransform {
                    entity: survivor_id,
                    value: engine_world::LocalTransform {
                        translation: glam::Vec3::X,
                        ..engine_world::LocalTransform::IDENTITY
                    },
                }])
                .unwrap();
        }
        runtime.stop();
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, survivor_id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([1., 0., 0.])
        );
        assert_eq!(scene.to_bytes().unwrap(), scene_bytes);
    }

    #[test]
    fn runtime_changes_suppress_reparented_translations_until_parent_is_restored() {
        let temp = tempfile::tempdir().unwrap();
        let mut scene = SceneDocument::new("Review");
        let parent = EntitySnapshot {
            local_transform: engine_world::LocalTransform {
                translation: glam::Vec3::new(10., 0., 0.),
                ..engine_world::LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        let other_parent = EntitySnapshot {
            local_transform: engine_world::LocalTransform {
                translation: glam::Vec3::new(20., 0., 0.),
                ..engine_world::LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        let child = EntitySnapshot {
            parent: Some(parent.id),
            local_transform: engine_world::LocalTransform {
                translation: glam::Vec3::X,
                ..engine_world::LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        let (parent_id, other_id, child_id) = (parent.id, other_parent.id, child.id);
        scene.entities = vec![parent, other_parent, child];
        let scene_bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        for keep_world in [true, false] {
            let mut host = environment_host(&runtime.active.world);
            host.gameplay_query(serde_json::json!({"op":"hierarchy_set_parent","entity":child_id,"parent":other_id,"keep_world":keep_world})).unwrap();
            assert!(
                runtime
                    .runtime_changes("base", &scene_bytes)
                    .unwrap()
                    .changes
                    .is_empty()
            );
            host.gameplay_query(serde_json::json!({"op":"hierarchy_detach","entity":child_id,"keep_world":keep_world})).unwrap();
            assert!(
                runtime
                    .runtime_changes("base", &scene_bytes)
                    .unwrap()
                    .changes
                    .is_empty()
            );
            // A restored parent makes local translation edits meaningful again.
            host.gameplay_query(serde_json::json!({"op":"hierarchy_set_parent","entity":child_id,"parent":parent_id,"keep_world":true})).unwrap();
        }
        {
            let mut world = runtime.active.world.lock().unwrap();
            let mut value = world.local_transform(child_id).unwrap();
            value.translation = glam::Vec3::new(2., 0., 0.);
            world
                .apply_commands(&[WorldCommand::SetLocalTransform {
                    entity: child_id,
                    value,
                }])
                .unwrap();
        }
        runtime.stop();
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, child_id.to_string());
        assert_eq!(
            changes.changes[0].before,
            RuntimeValue::Vector3([1., 0., 0.])
        );
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([2., 0., 0.])
        );
    }

    #[test]
    fn hot_reload_success_and_compile_failure_retain_last_good_state() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let entity = EntitySnapshot {
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let entity_id = entity.id;
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Test".into(),
            startup_scripts: Vec::new(),
            entities: vec![entity],
            instances: Vec::new(),
        };
        let scene_bytes = scene.to_bytes().unwrap();
        let relative = std::path::PathBuf::from("scripts/test.lua");
        std::fs::write(temp.path().join(&relative), b"return { fixed_update=function(dt) local x,y,z=rustic.get_translation(); rustic.set_translation(x+1,y,z) end }").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: relative,
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(0.016);
        assert!(runtime.reload(script_id, b"return { broken =").is_err());
        runtime.fixed_update(0.016);
        runtime.reload(script_id, b"return { fixed_update=function(dt) local x,y,z=rustic.get_translation(); rustic.set_translation(x+2,y,z) end }").unwrap();
        runtime.fixed_update(0.016);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(
            runtime.live_entity(entity_id).unwrap().unwrap().translation,
            [4.0, 0.0, 0.0]
        );
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, entity_id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([4.0, 0.0, 0.0])
        );
    }

    #[test]
    fn lua_and_javascript_behaviors_run_in_the_same_scene() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let lua_id = ScriptId::new();
        let javascript_id = ScriptId::new();
        let entity = EntitySnapshot {
            scripts: vec![
                ScriptComponent::new(lua_id),
                ScriptComponent::new(javascript_id),
            ],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Mixed".into(),
            startup_scripts: Vec::new(),
            entities: vec![entity],
            instances: Vec::new(),
        };
        let scene_bytes = scene.to_bytes().unwrap();
        let lua_path = std::path::PathBuf::from("scripts/move.lua");
        let javascript_path = std::path::PathBuf::from("scripts/move.js");
        std::fs::write(
            temp.path().join(&lua_path),
            b"return { fixed_update=function() local x,y,z=rustic.get_translation(); rustic.set_translation(x+1,y,z) end }",
        )
        .unwrap();
        std::fs::write(
            temp.path().join(&javascript_path),
            b"globalThis.behavior={fixed_update(){const [x,y,z]=rustic.get_translation();rustic.set_translation(x+2,y,z);}};",
        )
        .unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![
                    ScriptManifestEntry {
                        id: lua_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: lua_path,
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                    ScriptManifestEntry {
                        id: javascript_id,
                        language: ScriptLanguage::JavaScript,
                        relative_path: javascript_path,
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                ],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(1.0 / 60.0);
        assert!(
            runtime
                .reload(javascript_id, b"globalThis.behavior={")
                .is_err()
        );
        runtime.fixed_update(1.0 / 60.0);
        runtime
            .reload(
                javascript_id,
                b"globalThis.behavior={fixed_update(){const [x,y,z]=rustic.get_translation();rustic.set_translation(x+4,y,z);}};",
            )
            .unwrap();
        runtime.fixed_update(1.0 / 60.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([11.0, 0.0, 0.0])
        );
    }

    #[test]
    fn lua_behavior_receives_play_keyboard_state() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Keyboard".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot {
                scripts: vec![ScriptComponent::new(script_id)],
                ..EntitySnapshot::default()
            }],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/controller.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/controller.lua"),
            b"return { FixedUpdate=function(dt) if rustic.key('KeyW').held then local x,y,z=rustic.get_translation(); rustic.set_translation(x,y,z-dt) end end }",
        )
        .unwrap();
        let scene_bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(1.0);
        assert!(
            runtime
                .runtime_changes("base", &scene_bytes)
                .unwrap()
                .changes
                .is_empty()
        );
        runtime.set_input_keys(vec!["KeyW".into()]);
        runtime.fixed_update(1.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([0.0, 0.0, -1.0])
        );
        runtime.set_input_keys(Vec::new());
        runtime.fixed_update(1.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([0.0, 0.0, -1.0])
        );
    }

    #[test]
    fn legacy_entry_script_is_not_required_or_run() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Entry".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        std::fs::write(
            temp.path().join("settings.json"),
            br#"{"entry_script":"scripts/main.lua"}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"rustic.log('info', 'entry ran')",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        assert!(runtime.drain_logs().is_empty());
    }

    #[test]
    fn object_behavior_runs_on_its_attached_entity_at_start() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let camera = EntitySnapshot {
            name: Some("Camera".into()),
            ..EntitySnapshot::default()
        };
        let actor = EntitySnapshot {
            name: Some("Actor".into()),
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Attached entry".into(),
            startup_scripts: Vec::new(),
            entities: vec![camera.clone(), actor.clone()],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/main.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        GameSettings::default().save(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"return { on_start=function() rustic.set_translation(7, 0, 0) end }",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        let changes = runtime
            .runtime_changes("base", &scene.to_bytes().unwrap())
            .unwrap();

        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, actor.id.to_string());
        assert_ne!(changes.changes[0].target.entity_id, camera.id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([7.0, 0.0, 0.0])
        );
    }

    #[test]
    fn missing_scripts_are_skipped_in_every_scope_without_retries() {
        for scope in ["global", "scene", "object"] {
            for missing_entry in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                std::fs::create_dir_all(temp.path().join("config")).unwrap();
                let missing = ScriptId::new();
                let working = ScriptId::new();
                let references = vec![ScriptReference::new(missing), ScriptReference::new(working)];
                let mut actor = EntitySnapshot::default();
                if scope == "object" {
                    actor.scripts =
                        vec![ScriptComponent::new(missing), ScriptComponent::new(working)];
                }
                let scene = SceneDocument {
                    environment: engine_world::SceneEnvironment::default(),
                    id: engine_core::SceneId::new(),
                    name: "Missing scripts".into(),
                    startup_scripts: if scope == "scene" {
                        references.clone()
                    } else {
                        Vec::new()
                    },
                    entities: vec![actor],
                    instances: Vec::new(),
                };
                let mut settings = GameSettings::default();
                if scope == "global" {
                    settings.global_scripts = references;
                }
                settings.save(temp.path()).unwrap();
                let mut entries = vec![ScriptManifestEntry {
                    id: working,
                    language: ScriptLanguage::Lua54,
                    relative_path: "working.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }];
                if !missing_entry {
                    entries.push(ScriptManifestEntry {
                        id: missing,
                        relative_path: "missing.lua".into(),
                        ..entries[0].clone()
                    });
                }
                save_manifest_atomic(
                    &temp.path().join("config/scripts.ron"),
                    &ScriptManifest {
                        format_version: 2,
                        scripts: entries,
                    },
                )
                .unwrap();
                std::fs::write(
                    temp.path().join("working.lua"),
                    b"return { fixed_update=function(dt) rustic.log('info', 'still running') end }",
                )
                .unwrap();
                let mut runtime =
                    RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
                assert_eq!(
                    (runtime.active.behaviors.len() + runtime.globals.len()),
                    1,
                    "{scope}: missing entry={missing_entry}"
                );
                let warnings = runtime.drain_logs();
                assert_eq!(warnings.len(), 1);
                assert_eq!(warnings[0].0, "warn");
                assert!(warnings[0].1.contains(&missing.to_string()));
                assert!(warnings[0].1.contains("skipped for this play session"));
                // Restoring the source does not trigger a retry during this session.
                std::fs::write(temp.path().join("missing.lua"), b"return {}").unwrap();
                for _ in 0..3 {
                    runtime.fixed_update(0.016);
                    runtime.frame_update(0.016);
                }
                let logs = runtime.drain_logs();
                assert_eq!(logs.len(), 3);
                assert!(
                    logs.iter()
                        .all(|(level, message)| level == "info" && message == "still running")
                );
                assert_eq!((runtime.active.behaviors.len() + runtime.globals.len()), 1);
            }
        }
    }

    #[test]
    fn linked_web_script_errors_other_than_missing_files_still_fail() {
        let temp = tempfile::tempdir().unwrap();
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let dropped_logs = Arc::new(Mutex::new(0));
        std::fs::write(temp.path().join("invalid.js"), [0xff]).unwrap();
        std::fs::create_dir(temp.path().join("directory.js")).unwrap();
        let absolute = temp.path().join("absolute.js");
        for link in [
            "../outside.js",
            absolute.to_str().unwrap(),
            "invalid.js",
            "directory.js",
        ] {
            let source = format!("<script src=\"{link}\"></script>");
            assert!(
                resolve_linked_web_scripts(
                    temp.path(),
                    Path::new("index.html"),
                    source.as_bytes(),
                    &logs,
                    &dropped_logs,
                )
                .is_err(),
                "{link} should not be treated as a missing script"
            );
        }
        assert!(logs.lock().unwrap().is_empty());
    }

    #[test]
    fn linked_web_script_executes_on_its_attached_entity() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("ui")).unwrap();
        let script_id = ScriptId::new();
        let actor = EntitySnapshot {
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Linked web script".into(),
            startup_scripts: Vec::new(),
            entities: vec![actor.clone()],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Web,
                    relative_path: "ui/index.html".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        GameSettings::default().save(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("ui/index.html"),
            br#"<!doctype html><html><body><script src="missing.js">throw new Error('do not run fallback');</script><script src="behavior.js"></script><script>globalThis.behavior.OnCreate=function(){rustic.log('info', 'inline ran');};</script></body></html>"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("ui/behavior.js"),
            b"globalThis.behavior={Start(){rustic.set_translation(9,0,0);},FixedUpdate(dt){rustic.log('info', 'linked ran');}};",
        )
        .unwrap();

        let mut runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        let logs = runtime.drain_logs();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].0, "warn");
        assert!(
            logs[0]
                .1
                .contains(&Path::new("ui").join("missing.js").display().to_string())
        );
        assert_eq!(logs[1], ("info".into(), "inline ran".into()));
        for _ in 0..3 {
            runtime.fixed_update(0.016);
        }
        assert_eq!(
            runtime.drain_logs(),
            vec![("info".into(), "linked ran".into()); 3]
        );
        let changes = runtime
            .runtime_changes("base", &scene.to_bytes().unwrap())
            .unwrap();

        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, actor.id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([9.0, 0.0, 0.0])
        );
    }

    #[test]
    fn object_script_api_errors_are_reported_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Entry API error".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot {
                scripts: vec![ScriptComponent::new(script_id)],
                ..EntitySnapshot::default()
            }],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/main.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        GameSettings::default().save(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"return { on_create=function() rustic.set_property('missing', 1.0) end }",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap())
            .expect("a callback error must not abort runtime startup");
        let logs = runtime.drain_logs();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].1.contains("on_create failed and was disabled"));
        assert!(logs[0].1.contains("not declared"));
    }

    #[test]
    fn unattached_manifest_scripts_do_not_run() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Configured entry".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        let configured_id = ScriptId::new();
        let other_id = ScriptId::new();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![
                    ScriptManifestEntry {
                        id: other_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: "scripts/other.lua".into(),
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                    ScriptManifestEntry {
                        id: configured_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: "scripts/chosen.lua".into(),
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                ],
            },
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/other.lua"),
            b"rustic.log('info', 'wrong entry')",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/chosen.lua"),
            b"rustic.log('info', 'chosen entry')",
        )
        .unwrap();
        GameSettings::default().save(temp.path()).unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();

        assert!(runtime.drain_logs().is_empty());
    }

    #[test]
    fn invalid_settings_do_not_fall_back_to_an_arbitrary_manifest_script() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        std::fs::write(temp.path().join("settings.json"), b"not json").unwrap();
        std::fs::write(temp.path().join("scripts/other.lua"), b"return {}").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: ScriptId::new(),
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/other.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Invalid settings".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };

        let error = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap())
            .err()
            .expect("invalid settings must stop runtime startup");

        assert!(error.contains("invalid"));
        assert!(error.contains("settings.json"));
    }
    #[test]
    #[allow(
        clippy::too_many_lines,
        clippy::float_cmp,
        reason = "Language fixtures verify exactly representable scene values"
    )]
    fn high_level_gameplay_bindings_advance_in_shared_scene_runtime() {
        use engine_core::gameplay::{
            Ease, Value,
            animation::{Clip, Keyframe, Marker, Track, TrackInterpolation},
        };
        let fixtures: &[(ScriptLanguage,&str,&[u8])] = &[
            (ScriptLanguage::Lua54,"actions.lua",br"return {Start=function()
                assert(Smooth.lerp(0,8,0.5,Ease.InQuad)==2)
                Animation.value({{time=0,value=0,easing=Ease.InQuad},{time=2,value=8}},function(v) rustic.log('info','keyframe') end).onFinished(function() rustic.log('info','keyframesdone') end)
                Animation.play(nil,'Grow').onMarker('Mid',function() rustic.log('info','marker') end).onFinished(function() rustic.log('info','animation') end)
                Tween.move(rustic.game.Test.Actor,Vector3(8,0,0),2,Ease.InQuad).onFinished(function() rustic.log('info','done') end)
                Timer.after(0.5,function() rustic.log('info','timer') end)
            end}"),
            (ScriptLanguage::JavaScript,"actions.js",br#"globalThis.behavior={Start(){
                if(Smooth.lerp(0,8,0.5,Ease.InQuad)!==2)throw Error("core interpolation differs");Animation.value([{time:0,value:0,easing:Ease.InQuad},{time:2,value:8}],v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(()=>rustic.log('info','done'));
                Timer.after(0.5,()=>rustic.log('info','timer'));
            }};"#),

            (ScriptLanguage::Luau,"actions.luau",br"return {Start=function()
                assert(Smooth.lerp(0,8,0.5,Ease.InQuad)==2)
                Animation.value({{time=0,value=0,easing=Ease.InQuad},{time=2,value=8}},function(v) rustic.log('info','keyframe') end).onFinished(function() rustic.log('info','keyframesdone') end)
                Animation.play(nil,'Grow').onMarker('Mid',function() rustic.log('info','marker') end).onFinished(function() rustic.log('info','animation') end)
                Tween.move(rustic.game.Test.Actor,Vector3(8,0,0),2,Ease.InQuad).onFinished(function() rustic.log('info','done') end)
                Timer.after(0.5,function() rustic.log('info','timer') end)
            end}"),
            (ScriptLanguage::Web,"actions.html",br#"<!doctype html><html><body><script>
                globalThis.behavior={Start(){if(Smooth.lerp(0,8,0.5,Ease.InQuad)!==2)throw Error("core interpolation differs");Animation.value([{time:0,value:0,easing:Ease.InQuad},{time:2,value:8}],v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(()=>rustic.log('info','done'));Timer.after(0.5,()=>rustic.log('info','timer'));}};
            </script></body></html>"#),
            (ScriptLanguage::Python,"actions.py",br#"from rustic import rustic,run,Tween,Timer,Ease,Smooth,Animation
def start():
    assert Smooth.lerp(0,8,0.5,Ease.InQuad)==2
    Animation.value([dict(time=0,value=0,easing=Ease.InQuad),dict(time=2,value=8)],lambda v: rustic.log("info","keyframe")).onFinished(lambda: rustic.log("info","keyframesdone"))
    Animation.play(None,"Grow").onMarker("Mid",lambda: rustic.log("info","marker")).onFinished(lambda: rustic.log("info","animation"))
    Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(lambda: rustic.log('info','done'))
    Timer.after(0.5,lambda: rustic.log('info','timer'))
run({'on_start':start})
"#),
            (ScriptLanguage::C,"actions.c",br#"#include "rustic.h"
static void done(void){rustic.log("info","done");}static void timer(void){rustic.log("info","timer");}
static void marker(void){rustic.log("info","marker");}static void animation(void){rustic.log("info","animation");}
static void sample(RusticValue v){(void)v;rustic.log("info","keyframe");}static void keyframesdone(void){rustic.log("info","keyframesdone");}
static void start(void){RusticKeyframe keys[]={{0,{.type=RUSTIC_NUMBER,.number=0},"InQuad"},{2,{.type=RUSTIC_NUMBER,.number=8},NULL}};Animation.value(keys,2,sample,keyframesdone);RusticGameplayHandle a=Animation.play(NULL,"Grow",NULL,animation);Animation.onMarker(a,"Mid",marker);RusticValue zero={.type=RUSTIC_NUMBER,.number=0},eight={.type=RUSTIC_NUMBER,.number=8};if(Smooth.lerp(zero,eight,0.5,Ease.InQuad).number!=2)exit(2);Tween.move(NULL,(RusticVector3){8,0,0},2,Ease.InQuad,done);Timer.after(0.5,timer);}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::Cpp,"actions.cpp",br#"#include "rustic.hpp"
int main(){RusticBehavior b;b.on_start=[](){Animation::value({{0,0,Ease::InQuad},{2,8}},[](const RusticValue&){rustic.log("info","keyframe");})->onFinished([](){rustic.log("info","keyframesdone");});Animation::play("","Grow")->onMarker("Mid",[](){rustic.log("info","marker");}).onFinished([](){rustic.log("info","animation");});if(Smooth::lerp(0,8,0.5,Ease::InQuad).number()!=2)throw std::runtime_error("core interpolation differs");Tween::move("",{8,0,0},2,Ease::InQuad)->onFinished([](){rustic.log("info","done");});Timer::after(0.5,[](){rustic.log("info","timer");});};return rustic_run(b);}"#),
            (ScriptLanguage::CSharp,"actions.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){Animation.value(new Animation.Key[]{new(0,0,Ease.InQuad),new(2,8)},v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));if(Smooth.lerp(0.0,8.0,0.5,Ease.InQuad)!=2)throw new Exception("core interpolation differs");Tween.move(null,new double[]{8,0,0},2,Ease.InQuad).onFinished(()=>rustic.log("info","done"));Timer.after(0.5,()=>rustic.log("info","timer"));}});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){Animation.value(java.util.List.of(new Animation.Key(0,0,Ease.InQuad),new Animation.Key(2,8)),v->rustic.log("info","keyframe")).onFinished(()->rustic.log("info","keyframesdone"));Animation.play(null,"Grow",null).onMarker("Mid",()->rustic.log("info","marker")).onFinished(()->rustic.log("info","animation"));if(((Number)Smooth.lerp(0,8,0.5,Ease.InQuad)).doubleValue()!=2)throw new Exception("core interpolation differs");Tween.move(null,new double[]{8,0,0},2,Ease.InQuad).onFinished(()->rustic.log("info","done"));Timer.after(0.5,()->rustic.log("info","timer"));}});}}"#),
            (ScriptLanguage::Php,"actions.php",br#"<?php
require __DIR__."/rustic.php";
function start():void{global $rustic;Animation::value([["time"=>0,"value"=>0,"easing"=>Ease::InQuad],["time"=>2,"value"=>8]],fn($v)=>$rustic->log("info","keyframe"))->onFinished(fn()=>$rustic->log("info","keyframesdone"));Animation::play(null,"Grow")->onMarker("Mid",fn()=>$rustic->log("info","marker"))->onFinished(fn()=>$rustic->log("info","animation"));if(Smooth::lerp(0,8,0.5,Ease::InQuad)!=2)throw new RuntimeException("core interpolation differs");Tween::move(null,[8,0,0],2,Ease::InQuad)->onFinished(fn()=>$rustic->log('info','done'));Timer::after(0.5,fn()=>$rustic->log('info','timer'));}
rustic_run(['on_start'=>'start']);"#),
        ];
        for (language, name, source) in fixtures {
            if !matches!(language, ScriptLanguage::Lua54 | ScriptLanguage::JavaScript)
                && !engine_scripting::probe_language_toolchain(*language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_SDKS").is_none(),
                    "{} toolchain is required",
                    language.display_name()
                );
                continue;
            }
            eprintln!("Executing gameplay facade: {}", language.display_name());
            let entity = EntitySnapshot {
                name: Some("Actor".into()),
                ..Default::default()
            };
            let mut scene = SceneDocument::new("Test");
            scene.entities.push(entity.clone());
            let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
            let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
            gameplay.lock().unwrap().clips.insert(
                (entity.id, "Grow".into()),
                Clip {
                    name: "Grow".into(),
                    duration: 2.0,
                    tracks: vec![Track {
                        target: "Scale".into(),
                        interpolation: TrackInterpolation::default(),
                        keys: vec![
                            Keyframe {
                                time: 0.0,
                                value: Value::Vector([1.; 3]),
                                easing: Ease::default(),
                            },
                            Keyframe {
                                time: 2.0,
                                value: Value::Vector([3.; 3]),
                                easing: Ease::default(),
                            },
                        ],
                    }],
                    markers: vec![Marker {
                        time: 1.0,
                        name: "Mid".into(),
                    }],
                },
            );
            let logs = Arc::new(Mutex::new(VecDeque::new()));
            let host = RuntimeHost {
                gameplay: Arc::clone(&gameplay),
                owner_script: ScriptId::new(),
                entity_bound: true,
                scene_name: "Test".into(),
                world: Arc::clone(&world),
                input_keys: Arc::new(Mutex::new(BTreeSet::new())),
                snapshot_root: PathBuf::new(),
                entity: entity.id,
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::clone(&logs),
                dropped_logs: Arc::new(Mutex::new(0)),
                enabled: true,
                scenes: Arc::default(),
                global: false,
            };
            let mut behavior =
                RuntimeBehavior::load(*language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{}: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} Start: {e}", language.display_name()));
            {
                let mut context = gameplay.lock().unwrap();
                let mut scene = world.lock().unwrap();
                context
                    .runtime
                    .tick(
                        1.0,
                        &mut crate::gameplay_world::GameplayScene(
                            &mut scene,
                            &mut engine_core::gameplay::audio::Mixer::default(),
                        ),
                    )
                    .unwrap();
                assert_eq!(
                    scene.local_transform(entity.id).unwrap().translation.x,
                    2.0,
                    "{}",
                    language.display_name()
                );
            }
            behavior.update(1.0).unwrap();
            assert!(
                logs.lock().unwrap().iter().any(|(_, m)| m == "timer"),
                "{} timer missing",
                language.display_name()
            );
            {
                let mut context = gameplay.lock().unwrap();
                let mut scene = world.lock().unwrap();
                context
                    .runtime
                    .tick(
                        1.0,
                        &mut crate::gameplay_world::GameplayScene(
                            &mut scene,
                            &mut engine_core::gameplay::audio::Mixer::default(),
                        ),
                    )
                    .unwrap();
                assert_eq!(scene.local_transform(entity.id).unwrap().translation.x, 8.0);
            }
            behavior.update(1.0).unwrap();
            assert!(
                logs.lock().unwrap().iter().any(|(_, m)| m == "done"),
                "{} completion missing",
                language.display_name()
            );
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(entity.id)
                    .unwrap()
                    .scale
                    .x,
                3.0,
                "{} animation pose",
                language.display_name()
            );
            for message in ["marker", "animation", "keyframe", "keyframesdone"] {
                assert!(
                    logs.lock().unwrap().iter().any(|(_, m)| m == message),
                    "{} missing {} callback",
                    language.display_name(),
                    message
                );
            }
            behavior.cleanup_gameplay();
            assert!(
                gameplay
                    .lock()
                    .unwrap()
                    .runtime
                    .states_for_entities()
                    .is_empty()
            );
        }
    }
    #[test]
    fn lua_paths_values_and_cross_language_signals_use_shared_core() {
        if !engine_scripting::probe_language_toolchain(ScriptLanguage::Python).available {
            return;
        }
        let entity = EntitySnapshot::default();
        let mut scene = SceneDocument::new("Test");
        scene.entities.push(entity.clone());
        let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
        let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let make_host = || RuntimeHost {
            gameplay: Arc::clone(&gameplay),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "Test".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: entity.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::clone(&logs),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
            scenes: Arc::default(),
            global: false,
        };
        let mut lua=RuntimeBehavior::load(ScriptLanguage::Lua54,ScriptId::new(),br"return {Start=function()
            Events.once('Hit',function(id,message) assert(id==rustic.entity_id());rustic.log('info',message) end)
            Path.follow(nil,Path.create({Vector3(0,0,0),Vector3(8,0,0)}),{duration=2,easing=Ease.InQuad})
            Tween.value(0,8,2,function(value) rustic.log('info','value '..value) end,Ease.InQuad)
        end}","actions.lua",Box::new(make_host())).unwrap();
        lua.on_start().unwrap();
        let mut python = RuntimeBehavior::load(
            ScriptLanguage::Python,
            ScriptId::new(),
            br"from rustic import rustic,run,Events
def start(): Events.emit('Hit',[rustic.entity_id(),'cross-language'])
run({'on_start':start})
",
            "signal.py",
            Box::new(make_host()),
        )
        .unwrap();
        python.on_start().unwrap();
        {
            let mut context = gameplay.lock().unwrap();
            let mut scene = world.lock().unwrap();
            context
                .runtime
                .tick(
                    1.0,
                    &mut crate::gameplay_world::GameplayScene(
                        &mut scene,
                        &mut engine_core::gameplay::audio::Mixer::default(),
                    ),
                )
                .unwrap();
            assert!((scene.local_transform(entity.id).unwrap().translation.x - 2.0).abs() < 1e-6);
        }
        lua.update(1.0).unwrap();
        let entries = logs.lock().unwrap();
        assert!(entries.iter().any(|(_, m)| m == "cross-language"));
        assert!(
            entries
                .iter()
                .any(|(_, m)| m == "value 2.0" || m == "value 2")
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        clippy::float_cmp,
        reason = "Complete cross-language fixtures compare exactly representable scene values"
    )]
    fn canonical_api_paths_target_live_objects_in_all_available_sdks() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54,"paths.lua",br"return {Start=function()
    game.scene.Demo.Room.Player:EditAttribute('Name','Renamed')
    assert(game.rustic:groupsCreate('selected'))
    assert(game.scene.Demo.Room.Renamed:groupsAdd('selected'))
    game.scene.Demo.Room.Renamed:tween('Position',{8,0,0},2,'InQuad')
    game.rustic:after(0.5,function() game.rustic:log('info','timer') end)
    game.rustic.files.assets['night.hdr']:setSkyTexture()
end}"),
            (ScriptLanguage::Luau,"paths.luau",br"return {Start=function()
    game.scene.Demo.Room.Player:EditAttribute('Name','Renamed')
    assert(game.rustic:groupsCreate('selected'))
    assert(game.scene.Demo.Room.Renamed:groupsAdd('selected'))
    game.scene.Demo.Room.Renamed:tween('Position',{8,0,0},2,'InQuad')
    game.rustic:after(0.5,function() game.rustic:log('info','timer') end)
    game.rustic.files.assets['night.hdr']:setSkyTexture()
end}"),
            (ScriptLanguage::JavaScript,"paths.js",br"globalThis.behavior={Start(){
    game.scene.Demo.Room.Player.EditAttribute('Name','Renamed');
    game.rustic.groupsCreate('selected');
    game.scene.Demo.Room.Renamed.groupsAdd('selected');
    game.scene.Demo.Room.Renamed.tween('Position',[8,0,0],2,'InQuad');
    game.rustic.after(0.5,()=>game.rustic.log('info','timer'));
    game.rustic.files.assets['night.hdr'].setSkyTexture();
}};"),
            (ScriptLanguage::Web,"paths.html",br"<!doctype html><html><body><script>globalThis.behavior={Start(){
    game.scene.Demo.Room.Player.EditAttribute('Name','Renamed');
    game.rustic.groupsCreate('selected');
    game.scene.Demo.Room.Renamed.groupsAdd('selected');
    game.scene.Demo.Room.Renamed.tween('Position',[8,0,0],2,'InQuad');
    game.rustic.after(0.5,()=>game.rustic.log('info','timer'));
    game.rustic.files.assets['night.hdr'].setSkyTexture();
}};</script></body></html>"),
            (ScriptLanguage::Python,"paths.py",br"from rustic import game,run
def on_start():
    game.scene.Demo.Room.Player.EditAttribute('Name','Renamed')
    assert game.rustic.groupsCreate('selected')
    assert game.scene.Demo.Room.Renamed.groupsAdd(name='selected')
    game.scene.Demo.Room.Renamed.tween(property='Position',to=[8,0,0],duration=2,easing='InQuad')
    game.rustic.after(0.5,lambda:game.rustic.log('info','timer'))
    game.rustic.files.assets['night.hdr'].setSkyTexture()
run(globals())"),
            (ScriptLanguage::Cpp,"paths.cpp",br#"#include "rustic.hpp"
void start(){game.scene["Demo"]["Room"]["Player"].EditAttribute("Name","Renamed");game.rustic.groupsCreate("selected");game.scene["Demo"]["Room"]["Renamed"].groupsAdd("selected");game.scene["Demo"]["Room"]["Renamed"].tween("Position",RusticValue::Array{8,0,0},2.0,"InQuad");game.rustic.after(0.5,[]{game.rustic.log("info","timer");});game.rustic.files["assets"]["night.hdr"].setSkyTexture();}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C,"paths.c",br#"#include "rustic.h"
void timer(void){game.rustic.log("info","timer");}
void start(void){RusticScenePath original=game.scene("Demo","Room.Player");original.EditAttribute(original,"Name",(RusticValue){.type=RUSTIC_STRING,.string="Renamed"});RusticScenePath player=game.scene("Demo","Room.Renamed");game.rustic.groupsCreate("selected");player.groupsAdd(player,"selected");player.tween(player,"Position",(RusticValue){.type=RUSTIC_VECTOR,.vector={8,0,0},.length=3},2,"InQuad",NULL);game.rustic.after(0.5,timer);RusticFilePath sky=game.rustic.files("assets/night.hdr");sky.setSkyTexture(sky);}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp,"paths.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){game.scene["Demo"]["Room"]["Player"].EditAttribute("Name","Renamed");game.rustic.groupsCreate("selected");game.scene["Demo"]["Room"]["Renamed"].groupsAdd("selected");game.scene["Demo"]["Room"]["Renamed"].tween("Position",new double[]{8,0,0},2,"InQuad");game.rustic.after(0.5,()=>game.rustic.log("info","timer"));game.rustic.files["assets"]["night.hdr"].setSkyTexture();}});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){game.scene.scene("Demo").object("Room.Player").EditAttribute("Name","Renamed");game.rustic.groupsCreate("selected");game.scene.scene("Demo").object("Room.Renamed").groupsAdd("selected");game.scene.scene("Demo").object("Room.Renamed").tween("Position",new double[]{8,0,0},2,"InQuad");game.rustic.after(0.5,()->game.rustic.log("info","timer"));game.rustic.files.object("assets/night.hdr").setSkyTexture();}});}}"#),
            (ScriptLanguage::Php,"paths.php",br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $game;$game->scene->Demo->Room->Player->EditAttribute("Name","Renamed");$game->rustic->groupsCreate("selected");$game->scene->Demo->Room->Renamed->groupsAdd(name:"selected");$game->scene->Demo->Room->Renamed->tween("Position",[8,0,0],2,"InQuad");$game->rustic->after(0.5,fn()=>$game->rustic->log("info","timer"));$game->rustic->files->assets->{"night.hdr"}->setSkyTexture();}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !matches!(language, ScriptLanguage::Lua54 | ScriptLanguage::JavaScript)
                && !engine_scripting::probe_language_toolchain(*language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_SDKS").is_none(),
                    "{} toolchain is required",
                    language.display_name()
                );
                continue;
            }
            eprintln!("Checking canonical API paths: {}", language.display_name());
            let room = EntitySnapshot {
                name: Some("Room".into()),
                ..Default::default()
            };
            let player = EntitySnapshot {
                name: Some("Player".into()),
                parent: Some(room.id),
                ..Default::default()
            };
            let controller = EntitySnapshot {
                name: Some("Controller".into()),
                ..Default::default()
            };
            let mut scene = SceneDocument::new("Demo");
            scene.entities = vec![room, player.clone(), controller.clone()];
            let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
            let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
            let logs = Arc::new(Mutex::new(VecDeque::new()));
            let host = RuntimeHost {
                gameplay: Arc::clone(&gameplay),
                owner_script: ScriptId::new(),
                entity_bound: true,
                scene_name: "Demo".into(),
                world: Arc::clone(&world),
                entity: controller.id,
                input_keys: Arc::new(Mutex::new(BTreeSet::new())),
                snapshot_root: PathBuf::new(),
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::clone(&logs),
                dropped_logs: Arc::new(Mutex::new(0)),
                enabled: true,
                scenes: Arc::default(),
                global: false,
            };
            let mut behavior =
                RuntimeBehavior::load(*language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{} load: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} Start: {e}", language.display_name()));
            {
                let mut context = gameplay.lock().unwrap();
                let mut live = world.lock().unwrap();
                context
                    .runtime
                    .tick(
                        1.0,
                        &mut crate::gameplay_world::GameplayScene(
                            &mut live,
                            &mut engine_core::gameplay::audio::Mixer::default(),
                        ),
                    )
                    .unwrap();
                assert_eq!(
                    live.local_transform(player.id).unwrap().translation.x,
                    2.0,
                    "{}",
                    language.display_name()
                );
                assert_eq!(
                    live.local_transform(controller.id)
                        .unwrap()
                        .translation
                        .to_array(),
                    [0.; 3]
                );
                assert_eq!(live.environment().sky_image, "assets/night.hdr");
                assert_eq!(live.group_members("selected").unwrap(), vec![player.id]);
            }
            behavior.update(1.0).unwrap();
            assert!(
                logs.lock()
                    .unwrap()
                    .iter()
                    .any(|(_, message)| message == "timer"),
                "{} timer",
                language.display_name()
            );
        }
    }
}
