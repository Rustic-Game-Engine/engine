use crate::{EngineValue, EntityId, GameplayHost, ScriptId};
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const BRIDGE: &str = r#"
(() => {
  let state = Object.create(null);
  let commands = [];
  const objectAt = source => new Proxy(Object.create(null), {get: (_, key) => {
    if (key === "__source") return source;
    if (key === "EditAttribute" || key === "edit_attribute") return (name, value) => commands.push({op:"edit_attribute", source, name, value});
    if (typeof key !== "string") return undefined;
    return objectAt(source + "." + key);
  }});
  const rustic = Object.freeze({
    game: objectAt("rustic.game"),
    query: request => JSON.parse(__rustic_query(JSON.stringify(request))),
    gameplay: request => commands.push({op:"gameplay",request}),
    gameplay_connections: () => state.gameplay_connections || [],
    gameplay_states: () => state.gameplay_states || [],
    gameplay_callbacks: () => state.gameplay_callbacks || [],
    edit_object_attribute: (source, name, value) => commands.push({op:"edit_attribute", source, name, value}),
    entity_id: () => state.entity_id,
    delta_time: () => state.delta_time,
    fixed_delta_time: () => state.fixed_delta_time,
    get_translation: () => state.translation.slice(),
    set_translation: (x, y, z) => commands.push({op:"set_translation", value:[x,y,z]}),
    get_property: name => state.properties[name],
    set_property: (name, value) => commands.push({op:"set_property", name, value}),
    get_attribute: name => state.attributes[name],
    GetAttribute: name => state.attributes[name],
    edit_attribute: (name, value) => commands.push({op:"edit_attribute", name, value}),
    EditAttribute: (name, value) => commands.push({op:"edit_attribute", name, value}),
    input: name => Object.freeze(state.actions[String(name)] || {pressed:false, released:false, held:false, axis:0}),
    key: key => Object.freeze(state.keys[String(key)] || {pressed:false, released:false, held:false, axis:0}),
    key_events: () => Object.freeze(state.key_events.map(event => Object.freeze({...event}))),
    any_key_pressed: () => state.any_key_pressed,
    log: (level, message) => commands.push({op:"log", level:String(level), message:String(message)}),
    set_enabled: enabled => commands.push({op:"set_enabled", enabled:Boolean(enabled)})
  });
  Object.defineProperty(globalThis, "rustic", {value:rustic, writable:false, configurable:false});
  const writeLog = level => (...values) => rustic.log(level, values.map(value => String(value)).join(" "));
  const print = writeLog("info");
  const warn = writeLog("warn");
  Object.defineProperty(globalThis, "print", {value:print, writable:false, configurable:false});
  Object.defineProperty(globalThis, "warn", {value:warn, writable:false, configurable:false});
  Object.defineProperty(globalThis, "console", {value:Object.freeze({
    log: print,
    info: print,
    warn,
    error: writeLog("error"),
    debug: writeLog("debug")
  }), writable:false, configurable:false});
  const scene = new Proxy(Object.create(null), {get: (_, key) => {
    if (key === "Find") return path => state.scene_paths[path];
    if (key === "List") return () => Object.keys(state.scene_paths);
    return state.scene_paths[String(key)];
  }});
  Object.defineProperty(globalThis, "Game", {value:Object.freeze({scene, setCurrentCamera: source => {
    if (typeof source !== "string") throw new TypeError("expected a camera path or entity ID");
    commands.push({op:"set_current_camera", source});
  }}), writable:false, configurable:false});
  const instance = Object.freeze({
    add: (source, parent = null) => commands.push({op:"add_instance", source:String(source), parent}),
    clone: (source, parent = null) => commands.push({op:"clone_instance", source:String(source), parent})
  });
  Object.defineProperty(globalThis, "instance", {value:instance, writable:false, configurable:false});
  Object.defineProperty(globalThis, "__rustic_set_state", {value:value => { state=value; commands=[]; }});
  Object.defineProperty(globalThis, "__rustic_take_commands", {value:() => { const value=commands; commands=[]; return value; }});
})();
"#;

#[derive(Debug, Error)]
pub enum JavaScriptRuntimeError {
    #[error("JavaScript source exceeds the {0} byte limit")]
    SourceTooLarge(usize),
    #[error("JavaScript source is not valid UTF-8: {0}")]
    InvalidUtf8(String),
    #[error("JavaScript validation failed: {0}")]
    Validation(String),
    #[error("JavaScript runtime failed: {0}")]
    Runtime(String),
}

/// Parses and byte-compiles ECMAScript source without evaluating it.
///
/// # Errors
/// Returns an error for oversized/non-UTF-8 source or a parser/compiler failure.
pub fn validate_javascript(
    source: &[u8],
    _name: &str,
    maximum_bytes: usize,
) -> Result<(), JavaScriptRuntimeError> {
    if source.len() > maximum_bytes {
        return Err(JavaScriptRuntimeError::SourceTooLarge(maximum_bytes));
    }
    let text = std::str::from_utf8(source)
        .map_err(|error| JavaScriptRuntimeError::InvalidUtf8(error.to_string()))?;
    let encoded = serde_json::to_string(text)
        .map_err(|error| JavaScriptRuntimeError::Validation(error.to_string()))?;
    let (_runtime, context, _budget) = sandbox_runtime(u64::MAX)?;
    context
        .with(|context| context.eval::<(), _>(format!("new Function({encoded})")))
        .map_err(|error| JavaScriptRuntimeError::Validation(error.to_string()))
}

/// A sandboxed ECMAScript behavior using only the engine-owned command bridge.
pub struct JavaScriptBehavior {
    script_id: ScriptId,
    _runtime: Runtime,
    context: Context,
    host: Arc<Mutex<Box<dyn GameplayHost>>>,
    budget: Arc<std::sync::atomic::AtomicU64>,
    instruction_budget: u64,
    enabled: bool,
}

impl std::fmt::Debug for JavaScriptBehavior {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JavaScriptBehavior")
            .field("script_id", &self.script_id)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl JavaScriptBehavior {
    /// Creates an isolated behavior with an owned engine host.
    ///
    /// # Errors
    /// Returns parser, compiler, bridge setup, or module evaluation failures.
    pub fn load(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
        instruction_budget: u64,
    ) -> Result<Self, JavaScriptRuntimeError> {
        Self::load_shared(
            script_id,
            source,
            source_name,
            Arc::new(Mutex::new(host)),
            instruction_budget,
        )
    }

    /// Creates a replacement behavior sharing engine-owned state for hot reload.
    ///
    /// # Errors
    /// Returns parser, compiler, bridge setup, or module evaluation failures.
    pub fn load_shared(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Arc<Mutex<Box<dyn GameplayHost>>>,
        instruction_budget: u64,
    ) -> Result<Self, JavaScriptRuntimeError> {
        validate_javascript(source, source_name, 1024 * 1024)?;
        let text = std::str::from_utf8(source)
            .map_err(|error| JavaScriptRuntimeError::InvalidUtf8(error.to_string()))?;
        let (runtime, context, budget) = sandbox_runtime(instruction_budget.max(1))?;
        let query_host = Arc::clone(&host);
        context
            .with(|ctx| {
                ctx.globals().set(
                    "__rustic_query",
                    rquickjs::Function::new(
                        ctx.clone(),
                        move |request: String| -> rquickjs::Result<String> {
                            let response = (|| -> Result<serde_json::Value, String> {
                                let request =
                                    serde_json::from_str(&request).map_err(|e| e.to_string())?;
                                query_host
                                    .lock()
                                    .map_err(|_| "gameplay lock poisoned")?
                                    .gameplay_query(request)
                            })();
                            match response {
                                Ok(value) => serde_json::to_string(&value)
                                    .map_err(|_| rquickjs::Error::Unknown),
                                Err(_) => Err(rquickjs::Error::Unknown),
                            }
                        },
                    )?,
                )
            })
            .map_err(|e| JavaScriptRuntimeError::Runtime(e.to_string()))?;
        context
            .with(|context| {
                context.eval::<(), _>(format!("{}\n{}", BRIDGE, include_str!("sdk/gameplay.js")))
            })
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        context
            .with(|context| {
                context.eval::<(), _>("globalThis.behavior = Object.create(null);")?;
                context.eval::<(), _>(text)
            })
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        let behavior_type = context
            .with(|context| context.eval::<String, _>("typeof globalThis.behavior"))
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        if behavior_type != "object" {
            return Err(JavaScriptRuntimeError::Runtime(format!(
                "{source_name} must assign an object to globalThis.behavior"
            )));
        }
        Ok(Self {
            script_id,
            _runtime: runtime,
            context,
            host,
            budget,
            instruction_budget: instruction_budget.max(1),
            enabled: true,
        })
    }

    pub const fn script_id(&self) -> ScriptId {
        self.script_id
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn host(&self) -> Arc<Mutex<Box<dyn GameplayHost>>> {
        Arc::clone(&self.host)
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn on_create(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("OnCreate", "on_create", None)
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn on_start(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("Start", "on_start", None)
    }
    /// # Errors
    /// Returns and contains an `OnEnable` callback failure.
    pub fn on_enable(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("OnEnable", "on_enable", None)
    }
    /// # Errors
    /// Returns and contains an `OnDisable` callback failure.
    pub fn on_disable(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("OnDisable", "on_disable", None)
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn fixed_update(&mut self, delta: f64) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("FixedUpdate", "fixed_update", Some(delta))
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn update(&mut self, delta: f64) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("Update", "update", Some(delta))
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn on_destroy(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("OnDestroy", "on_destroy", None)
    }

    /// # Errors
    /// Returns and contains a callback failure.
    pub fn on_stop(&mut self) -> Result<(), JavaScriptRuntimeError> {
        self.call_compatible("OnStop", "on_stop", None)
    }

    fn call_compatible(
        &mut self,
        canonical: &str,
        legacy: &str,
        delta: Option<f64>,
    ) -> Result<(), JavaScriptRuntimeError> {
        if !self.enabled {
            return Ok(());
        }
        let result = self.call_inner(canonical, legacy, delta);
        if result.is_err() {
            self.enabled = false;
        }
        result
    }

    fn call_inner(
        &mut self,
        canonical: &str,
        legacy: &str,
        delta: Option<f64>,
    ) -> Result<(), JavaScriptRuntimeError> {
        let state = {
            let mut host = self.host.lock().map_err(|_| {
                JavaScriptRuntimeError::Runtime("gameplay host lock poisoned".into())
            })?;
            let attributes = [
                "Name",
                "Position",
                "Size",
                "Color",
                "CanTouch",
                "CanCollide",
                "Anchored",
                "Parent",
            ]
            .into_iter()
            .filter_map(|name| {
                host.attribute(name)
                    .ok()
                    .flatten()
                    .map(|value| (name.to_owned(), engine_to_json(&value)))
            })
            .collect::<Map<_, _>>();
            let scene_paths = host
                .scene_paths()
                .into_iter()
                .map(|(path, id)| (path, Value::String(id.to_string())))
                .collect::<Map<_, _>>();
            json!({
                "gameplay_connections":host.gameplay_connections(),
                "gameplay_states": host.gameplay_states(),
                "gameplay_callbacks": if canonical == "Update" {host.gameplay_callbacks()} else {Vec::new()},
                "entity_id": host.entity_id().to_string(),
                "delta_time": host.delta_time(),
                "fixed_delta_time": host.fixed_delta_time(),
                "translation": host.translation(),
                "properties": host.properties().into_iter().map(|(key, value)| (key, engine_to_json(&value))).collect::<Map<_, _>>(),
                "actions": {},
                "attributes": attributes,
                "scene_paths": scene_paths,
                "keys": host.input_frame().keys.iter().chain(host.input_frame().keys_pressed.iter()).chain(host.input_frame().keys_released.iter()).map(|key| (key.clone(), json!(host.input_frame().key(key)))).collect::<Map<_, _>>(),
                "key_events": host.input_frame().key_events,
                "any_key_pressed": host.input_frame().any_key_pressed(),
            })
        };
        let state_json = serde_json::to_string(&state)
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        self.budget.store(
            self.instruction_budget,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.context
            .with(|context| context.eval::<(), _>(format!("__rustic_set_state({state_json})")))
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        let argument = delta.map_or_else(String::new, |value| value.to_string());
        let invocation = format!(
            "if (typeof __rustic_gameplay_dispatch === 'function') __rustic_gameplay_dispatch(); if (typeof behavior.{canonical} === 'function') behavior.{canonical}({argument}); else if (typeof behavior.{legacy} === 'function') behavior.{legacy}({argument})"
        );
        self.context
            .with(|context| context.eval::<(), _>(invocation))
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        let commands = self
            .context
            .with(|context| context.eval::<String, _>("JSON.stringify(__rustic_take_commands())"))
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        let commands: Vec<Command> = serde_json::from_str(&commands)
            .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
        let mut host = self
            .host
            .lock()
            .map_err(|_| JavaScriptRuntimeError::Runtime("gameplay host lock poisoned".into()))?;
        for command in commands {
            apply_command(host.as_mut(), command)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Gameplay {
        request: Box<engine_core::gameplay::Request>,
    },
    SetCurrentCamera {
        source: String,
    },
    SetTranslation {
        value: [f64; 3],
    },
    SetProperty {
        name: String,
        value: Value,
    },
    EditAttribute {
        #[serde(default)]
        source: Option<String>,
        name: String,
        value: Value,
    },
    Log {
        level: String,
        message: String,
    },
    SetEnabled {
        enabled: bool,
    },
    AddInstance {
        source: String,
        parent: Option<String>,
    },
    CloneInstance {
        source: String,
        parent: Option<String>,
    },
}

fn apply_command(
    host: &mut dyn GameplayHost,
    command: Command,
) -> Result<(), JavaScriptRuntimeError> {
    let result = match command {
        Command::Gameplay { request } => host.gameplay_request(*request),
        Command::SetTranslation { value } => host.set_translation(value),
        Command::SetProperty { name, value } => host.property(&name).map_or_else(
            || Err(format!("property `{name}` is not declared")),
            |current| {
                json_to_engine(value, &current)
                    .and_then(|converted| host.set_property(&name, converted))
            },
        ),
        Command::EditAttribute {
            source,
            name,
            value,
        } => source
            .as_ref()
            .map_or_else(
                || host.attribute(&name),
                |source| host.object_attribute(source, &name),
            )
            .and_then(|current| {
                let current = current.ok_or_else(|| format!("unknown attribute `{name}`"))?;
                json_to_engine(value, &current).and_then(|converted| match &source {
                    Some(source) => host.edit_object_attribute(source, &name, converted),
                    None => host.edit_attribute(&name, converted),
                })
            }),
        Command::Log { level, message } => host.log(&level, &message),
        Command::SetEnabled { enabled } => {
            host.set_enabled(enabled);
            Ok(())
        }
        Command::AddInstance { source, parent } => parse_parent(parent)
            .and_then(|parent| host.add_instance(&source, parent))
            .map(|_| ()),
        Command::SetCurrentCamera { source } => host.set_current_camera(&source),
        Command::CloneInstance { source, parent } => parse_parent(parent)
            .and_then(|parent| host.clone_instance(&source, parent))
            .map(|_| ()),
    };
    result.map_err(JavaScriptRuntimeError::Runtime)
}

fn parse_parent(parent: Option<String>) -> Result<Option<EntityId>, String> {
    parent
        .map(|value| {
            value
                .parse::<EntityId>()
                .map_err(|_| format!("invalid parent entity id `{value}`"))
        })
        .transpose()
}

fn engine_to_json(value: &EngineValue) -> Value {
    match value {
        EngineValue::Boolean(value) => Value::Bool(*value),
        EngineValue::Integer(value) => Value::Number((*value).into()),
        EngineValue::Number(value) => json!(value),
        EngineValue::String(value) => Value::String(value.clone()),
        EngineValue::Vec2(value) => json!(value),
        EngineValue::Vec3(value) => json!(value),
        EngineValue::Entity(value) => value.map_or(Value::Null, |id| Value::String(id.to_string())),
    }
}

fn json_to_engine(value: Value, current: &EngineValue) -> Result<EngineValue, String> {
    match current {
        EngineValue::Boolean(_) => value.as_bool().map(EngineValue::Boolean),
        EngineValue::Integer(_) => value.as_i64().map(EngineValue::Integer),
        EngineValue::Number(_) => value.as_f64().map(EngineValue::Number),
        EngineValue::String(_) => value.as_str().map(|v| EngineValue::String(v.into())),
        EngineValue::Vec2(_) => serde_json::from_value(value).ok().map(EngineValue::Vec2),
        EngineValue::Vec3(_) => serde_json::from_value(value).ok().map(EngineValue::Vec3),
        EngineValue::Entity(_) => {
            if value.is_null() {
                Some(EngineValue::Entity(None))
            } else {
                value
                    .as_str()
                    .and_then(|text| text.parse().ok())
                    .map(|id| EngineValue::Entity(Some(id)))
            }
        }
    }
    .ok_or_else(|| "public property type mismatch".into())
}

fn sandbox_runtime(
    instruction_budget: u64,
) -> Result<(Runtime, Context, Arc<std::sync::atomic::AtomicU64>), JavaScriptRuntimeError> {
    use std::sync::atomic::Ordering;
    let runtime =
        Runtime::new().map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
    runtime.set_memory_limit(32 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let budget = Arc::new(std::sync::atomic::AtomicU64::new(instruction_budget));
    let interrupt_budget = Arc::clone(&budget);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupt_budget
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_err()
    })));
    let context = Context::full(&runtime)
        .map_err(|error| JavaScriptRuntimeError::Runtime(error.to_string()))?;
    Ok((runtime, context, budget))
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
mod tests {
    use super::*;
    use crate::ActionState;
    use engine_core::EntityId;
    use std::collections::BTreeMap;

    struct Host {
        entity: EntityId,
        translation: [f64; 3],
        properties: BTreeMap<String, EngineValue>,
        logs: Vec<String>,
    }

    impl GameplayHost for Host {
        fn entity_id(&self) -> EntityId {
            self.entity
        }
        fn delta_time(&self) -> f64 {
            1.0 / 60.0
        }
        fn fixed_delta_time(&self) -> f64 {
            1.0 / 60.0
        }
        fn translation(&self) -> [f64; 3] {
            self.translation
        }
        fn set_translation(&mut self, value: [f64; 3]) -> Result<(), String> {
            self.translation = value;
            Ok(())
        }
        fn find_entity(&self, _query: &str) -> Result<Option<EntityId>, String> {
            Ok(None)
        }
        fn input_action(&self, _name: &str) -> ActionState {
            ActionState::default()
        }
        fn log(&mut self, _level: &str, message: &str) -> Result<(), String> {
            self.logs.push(message.into());
            Ok(())
        }
        fn property(&self, name: &str) -> Option<EngineValue> {
            self.properties.get(name).cloned()
        }
        fn properties(&self) -> BTreeMap<String, EngineValue> {
            self.properties.clone()
        }
        fn set_property(&mut self, name: &str, value: EngineValue) -> Result<(), String> {
            let current = self
                .properties
                .get(name)
                .ok_or_else(|| format!("property `{name}` is not declared"))?;
            if !current.same_type(&value) {
                return Err("property type mismatch".into());
            }
            self.properties.insert(name.into(), value);
            Ok(())
        }
        fn set_enabled(&mut self, _enabled: bool) {}
    }

    fn host() -> Box<dyn GameplayHost> {
        Box::new(Host {
            entity: EntityId::new(),
            translation: [0.0; 3],
            properties: BTreeMap::from([("speed".into(), EngineValue::Number(2.0))]),
            logs: Vec::new(),
        })
    }

    #[test]
    fn validation_does_not_execute_source() {
        validate_javascript(b"throw new Error('must not run')", "test.js", 1024).unwrap();
    }

    #[test]
    fn lifecycle_uses_shared_api_and_budget_contains_loops() {
        let source = br"globalThis.behavior = { fixed_update(dt) { const [x,y,z]=rustic.get_translation(); rustic.set_translation(x+rustic.get_property('speed'),y,z); } };";
        let mut behavior =
            JavaScriptBehavior::load(ScriptId::new(), source, "move.js", host(), 10_000).unwrap();
        behavior.fixed_update(0.016).unwrap();
        let translation = behavior.host().lock().unwrap().translation();
        assert!((translation[0] - 2.0).abs() < f64::EPSILON);

        let looping = br"globalThis.behavior = { update() { while (true) {} } };";
        let mut behavior =
            JavaScriptBehavior::load(ScriptId::new(), looping, "loop.js", host(), 100).unwrap();
        assert!(behavior.update(0.016).is_err());
    }

    #[test]
    fn canonical_lifecycle_and_native_logging_functions_are_available() {
        let source = br#"globalThis.behavior = {
            OnCreate() { print("created", 1); warn("careful"); console.debug("detail"); rustic.set_translation(1,2,3); },
            OnStop() { rustic.set_translation(4,5,6); }
        };"#;
        let mut behavior =
            JavaScriptBehavior::load(ScriptId::new(), source, "native-log.js", host(), 100_000)
                .unwrap();

        behavior.on_create().unwrap();
        assert_eq!(
            behavior.host().lock().unwrap().translation(),
            [1.0, 2.0, 3.0]
        );
        behavior.on_stop().unwrap();
        assert_eq!(
            behavior.host().lock().unwrap().translation(),
            [4.0, 5.0, 6.0]
        );
    }

    #[test]
    fn api_smoke_entry_allows_an_undeclared_optional_property() {
        let mut behavior = JavaScriptBehavior::load(
            ScriptId::new(),
            include_bytes!("../../../examples/api-smoke-test/entry.js"),
            "entry.js",
            host(),
            100_000,
        )
        .unwrap();

        behavior.on_create().unwrap();
        assert!(behavior.enabled);
    }
}
