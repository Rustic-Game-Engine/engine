use crate::{ActionState, EngineValue, EntityId, InputFrame, ScriptId};
use mlua::{Error as MluaError, Function, Lua, RegistryKey, Table, Value, Variadic};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Engine-owned runtime surface. Implementations live in the isolated runtime process.
#[allow(clippy::missing_errors_doc)]
pub trait GameplayHost: Send {
    fn set_current_camera(&mut self, _source: &str) -> Result<(), String> {
        Err("camera selection is unavailable".into())
    }
    fn entity_id(&self) -> EntityId;
    fn delta_time(&self) -> f64;
    fn fixed_delta_time(&self) -> f64;
    fn translation(&self) -> [f64; 3];
    /// # Errors
    /// Rejects stale entities and invalid/non-finite values.
    fn set_translation(&mut self, value: [f64; 3]) -> Result<(), String>;
    /// # Errors
    /// Reports invalid identifiers or unavailable world access.
    fn find_entity(&self, name_or_id: &str) -> Result<Option<EntityId>, String>;
    fn list_entities(&self, _path: &str) -> Result<Vec<EntityId>, String> {
        Ok(Vec::new())
    }
    fn scene_paths(&self) -> std::collections::BTreeMap<String, EntityId> {
        std::collections::BTreeMap::new()
    }
    fn add_instance(
        &mut self,
        _source: &str,
        _parent: Option<EntityId>,
    ) -> Result<EntityId, String> {
        Err("instance creation is unavailable".into())
    }
    fn clone_instance(
        &mut self,
        _source: &str,
        _parent: Option<EntityId>,
    ) -> Result<EntityId, String> {
        Err("instance cloning is unavailable".into())
    }
    fn attribute(&self, _name: &str) -> Result<Option<EngineValue>, String> {
        Ok(None)
    }
    fn edit_attribute(&mut self, _name: &str, _value: EngineValue) -> Result<(), String> {
        Err("attribute editing is unavailable".into())
    }
    fn input_action(&self, name: &str) -> ActionState;
    fn input_frame(&self) -> InputFrame {
        InputFrame::default()
    }
    /// # Errors
    /// Reports a closed or poisoned bounded log sink.
    fn log(&mut self, level: &str, message: &str) -> Result<(), String>;
    fn property(&self, name: &str) -> Option<EngineValue>;
    fn properties(&self) -> std::collections::BTreeMap<String, EngineValue> {
        std::collections::BTreeMap::new()
    }
    /// # Errors
    /// Rejects undeclared properties and type mismatches.
    fn set_property(&mut self, name: &str, value: EngineValue) -> Result<(), String>;
    fn enabled(&self) -> bool {
        true
    }
    fn set_enabled(&mut self, enabled: bool);
}

#[derive(Debug, Error)]
pub enum LuaRuntimeError {
    #[error("Lua source exceeds the {0} byte limit")]
    SourceTooLarge(usize),
    #[error("Lua source is not valid UTF-8: {0}")]
    InvalidUtf8(String),
    #[error("Lua validation failed: {0}")]
    Validation(String),
    #[error("Lua runtime failed: {0}")]
    Runtime(String),
    #[error("Lua execution budget exhausted")]
    BudgetExhausted,
}

/// Parses and compiles source without running it or discovering metadata by execution.
/// Compiles source without executing the chunk.
///
/// # Errors
/// Returns an error for oversized/non-UTF-8 source or Lua syntax/compiler failure.
pub fn validate_lua(
    source: &[u8],
    name: &str,
    maximum_bytes: usize,
) -> Result<(), LuaRuntimeError> {
    if source.len() > maximum_bytes {
        return Err(LuaRuntimeError::SourceTooLarge(maximum_bytes));
    }
    let text = std::str::from_utf8(source)
        .map_err(|error| LuaRuntimeError::InvalidUtf8(error.to_string()))?;
    let lua = sandbox_lua().map_err(|error| LuaRuntimeError::Validation(error.to_string()))?;
    lua.load(text)
        .set_name(name)
        .into_function()
        .map(|_| ())
        .map_err(|error| LuaRuntimeError::Validation(error.to_string()))
}

/// One isolated-runtime Lua instance. A failed callback disables only this instance.
pub struct LuaBehavior {
    script_id: ScriptId,
    lua: Lua,
    behavior: RegistryKey,
    host: Arc<Mutex<Box<dyn GameplayHost>>>,
    budget_remaining: Arc<AtomicU64>,
    instruction_budget: u64,
    enabled: bool,
}

impl std::fmt::Debug for LuaBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LuaBehavior")
            .field("script_id", &self.script_id)
            .field("instruction_budget", &self.instruction_budget)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl LuaBehavior {
    /// Creates a sandboxed instance with an owned host.
    ///
    /// # Errors
    /// Returns validation, sandbox setup, or module evaluation failures.
    pub fn load(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
        instruction_budget: u64,
    ) -> Result<Self, LuaRuntimeError> {
        Self::load_shared(
            script_id,
            source,
            source_name,
            Arc::new(Mutex::new(host)),
            instruction_budget,
        )
    }

    /// Creates a sandboxed instance sharing engine-owned state for compatible reload.
    ///
    /// # Errors
    /// Returns validation, sandbox setup, or module evaluation failures.
    pub fn load_shared(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Arc<Mutex<Box<dyn GameplayHost>>>,
        instruction_budget: u64,
    ) -> Result<Self, LuaRuntimeError> {
        validate_lua(source, source_name, 1024 * 1024)?;
        let text =
            std::str::from_utf8(source).map_err(|e| LuaRuntimeError::InvalidUtf8(e.to_string()))?;
        let lua = sandbox_lua().map_err(|error| runtime_error(&error))?;
        install_api(&lua, Arc::clone(&host)).map_err(|error| runtime_error(&error))?;
        let budget_remaining = Arc::new(AtomicU64::new(instruction_budget.max(1)));
        let hook_budget = Arc::clone(&budget_remaining);
        lua.set_hook(
            mlua::HookTriggers::new().every_nth_instruction(1_000),
            move |_, _| {
                let previous = hook_budget.fetch_sub(1_000, Ordering::Relaxed);
                if previous <= 1_000 {
                    Err(MluaError::RuntimeError(
                        "Rustic execution budget exhausted".into(),
                    ))
                } else {
                    Ok(mlua::VmState::Continue)
                }
            },
        )
        .map_err(|error| runtime_error(&error))?;
        let value: Value = lua
            .load(text)
            .set_name(source_name)
            .eval()
            .map_err(|error| runtime_error(&error))?;
        let table = match value {
            Value::Table(table) => table,
            Value::Nil => lua.create_table().map_err(|error| runtime_error(&error))?,
            _ => {
                return Err(LuaRuntimeError::Runtime(format!(
                    "{source_name} must return a behavior table or no value"
                )));
            }
        };
        let behavior = lua
            .create_registry_value(table)
            .map_err(|error| runtime_error(&error))?;
        Ok(Self {
            script_id,
            lua,
            behavior,
            host,
            budget_remaining,
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
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn on_create(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("OnCreate", "on_create", None)
    }
    /// # Errors
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn on_start(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("Start", "on_start", None)
    }
    /// # Errors
    /// Returns and contains an `OnEnable` callback failure.
    pub fn on_enable(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("OnEnable", "on_enable", None)
    }
    /// # Errors
    /// Returns and contains an `OnDisable` callback failure.
    pub fn on_disable(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("OnDisable", "on_disable", None)
    }
    /// # Errors
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn fixed_update(&mut self, delta: f64) -> Result<(), LuaRuntimeError> {
        self.call_compatible("FixedUpdate", "fixed_update", Some(delta))
    }
    /// # Errors
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn update(&mut self, delta: f64) -> Result<(), LuaRuntimeError> {
        self.call_compatible("Update", "update", Some(delta))
    }
    /// # Errors
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn on_destroy(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("OnDestroy", "on_destroy", None)
    }
    /// # Errors
    /// Returns and contains a callback exception or exhausted instruction budget.
    pub fn on_stop(&mut self) -> Result<(), LuaRuntimeError> {
        self.call_compatible("OnStop", "on_stop", None)
    }

    fn call_compatible(
        &mut self,
        canonical: &str,
        legacy: &str,
        delta: Option<f64>,
    ) -> Result<(), LuaRuntimeError> {
        if !self.enabled {
            return Ok(());
        }
        self.budget_remaining
            .store(self.instruction_budget, Ordering::Relaxed);
        let result = (|| {
            let table: Table = self.lua.registry_value(&self.behavior)?;
            let callback = table
                .get::<Option<Function>>(canonical)?
                .or_else(|| table.get::<Option<Function>>(legacy).ok().flatten());
            if let Some(callback) = callback {
                if let Some(delta) = delta {
                    callback.call::<()>(delta)?;
                } else {
                    callback.call::<()>(())?;
                }
            }
            Ok::<(), MluaError>(())
        })();
        if let Err(error) = result {
            self.enabled = false;
            if error.to_string().contains("execution budget exhausted") {
                return Err(LuaRuntimeError::BudgetExhausted);
            }
            return Err(runtime_error(&error));
        }
        Ok(())
    }
}

fn sandbox_lua() -> Result<Lua, MluaError> {
    let lua = Lua::new();
    let globals = lua.globals();
    for denied in [
        "io",
        "os",
        "package",
        "debug",
        "dofile",
        "loadfile",
        "require",
        "collectgarbage",
    ] {
        globals.set(denied, Value::Nil)?;
    }
    drop(globals);
    Ok(lua)
}

fn lock_host(
    host: &Arc<Mutex<Box<dyn GameplayHost>>>,
) -> mlua::Result<std::sync::MutexGuard<'_, Box<dyn GameplayHost>>> {
    host.lock()
        .map_err(|_| MluaError::RuntimeError("gameplay host lock poisoned".into()))
}

#[allow(clippy::too_many_lines)]
fn install_api(lua: &Lua, host: Arc<Mutex<Box<dyn GameplayHost>>>) -> mlua::Result<()> {
    let api = lua.create_table()?;
    let h = Arc::clone(&host);
    api.set(
        "entity_id",
        lua.create_function(move |_, ()| Ok(lock_host(&h)?.entity_id().to_string()))?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "delta_time",
        lua.create_function(move |_, ()| Ok(lock_host(&h)?.delta_time()))?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "fixed_delta_time",
        lua.create_function(move |_, ()| Ok(lock_host(&h)?.fixed_delta_time()))?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "get_translation",
        lua.create_function(move |_, ()| {
            let v = lock_host(&h)?.translation();
            Ok((v[0], v[1], v[2]))
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "set_translation",
        lua.create_function(move |_, (x, y, z): (f64, f64, f64)| {
            lock_host(&h)?
                .set_translation([x, y, z])
                .map_err(MluaError::RuntimeError)
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "find_entity",
        lua.create_function(move |_, query: String| {
            match lock_host(&h)?
                .find_entity(&query)
                .map_err(MluaError::RuntimeError)?
            {
                Some(id) => Ok((Some(id.to_string()), None::<String>)),
                None => Ok((None, Some("entity not found".into()))),
            }
        })?,
    )?;
    let h = Arc::clone(&host);
    let get_attribute = lua.create_function(move |lua, name: String| {
        let value = lock_host(&h)?
            .attribute(&name)
            .map_err(MluaError::RuntimeError)?;
        engine_to_lua(lua, value)
    })?;
    api.set("get_attribute", get_attribute.clone())?;
    api.set("GetAttribute", get_attribute)?;
    let h = Arc::clone(&host);
    let edit_attribute = lua.create_function(move |_, (name, value): (String, Value)| {
        lock_host(&h)?
            .edit_attribute(&name, lua_to_engine(value)?)
            .map_err(MluaError::RuntimeError)
    })?;
    api.set("edit_attribute", edit_attribute.clone())?;
    api.set("EditAttribute", edit_attribute)?;
    let scene = lua.create_table()?;
    let h = Arc::clone(&host);
    scene.set(
        "Find",
        lua.create_function(move |_, path: String| {
            Ok(lock_host(&h)?
                .find_entity(&path)
                .map_err(MluaError::RuntimeError)?
                .map(|id| id.to_string()))
        })?,
    )?;
    let h = Arc::clone(&host);
    scene.set(
        "List",
        lua.create_function(move |lua, path: Option<String>| {
            let ids = lock_host(&h)?
                .list_entities(path.as_deref().unwrap_or("Game.scene"))
                .map_err(MluaError::RuntimeError)?;
            let table = lua.create_table()?;
            for (index, id) in ids.into_iter().enumerate() {
                table.set(index + 1, id.to_string())?;
            }
            Ok(table)
        })?,
    )?;
    let metadata = lua.create_table()?;
    let h = Arc::clone(&host);
    metadata.set(
        "__index",
        lua.create_function(move |_, (_table, name): (Table, String)| {
            Ok(lock_host(&h)?
                .find_entity(&name)
                .map_err(MluaError::RuntimeError)?
                .map(|id| id.to_string()))
        })?,
    )?;
    scene.set_metatable(Some(metadata))?;
    let game = lua.create_table()?;
    game.set("scene", scene)?;
    let h = Arc::clone(&host);
    game.set(
        "setCurrentCamera",
        lua.create_function(move |_, value: Value| {
            let value = match value {
                Value::Table(table) => table.get::<Value>(1)?,
                value => value,
            };
            let Value::String(source) = value else {
                return Err(MluaError::RuntimeError(
                    "expected a camera path or {cameraPath}".into(),
                ));
            };
            lock_host(&h)?
                .set_current_camera(source.to_str()?.as_ref())
                .map_err(MluaError::RuntimeError)
        })?,
    )?;
    lua.globals().set("Game", game)?;
    let instance = lua.create_table()?;
    for (name, clone_only) in [("add", false), ("clone", true)] {
        let h = Arc::clone(&host);
        instance.set(
            name,
            lua.create_function(move |_, (source, parent): (String, Option<String>)| {
                let parent = parent
                    .map(|value| {
                        value.parse::<EntityId>().map_err(|_| {
                            MluaError::RuntimeError(format!("invalid parent entity id `{value}`"))
                        })
                    })
                    .transpose()?;
                let mut host = lock_host(&h)?;
                let result = if clone_only {
                    host.clone_instance(&source, parent)
                } else {
                    host.add_instance(&source, parent)
                };
                result
                    .map(|id| id.to_string())
                    .map_err(MluaError::RuntimeError)
            })?,
        )?;
    }
    lua.globals().set("instance", instance)?;
    let h = Arc::clone(&host);
    api.set(
        "input",
        lua.create_function(move |lua, name: String| {
            let state = lock_host(&h)?.input_action(&name);
            let table = lua.create_table()?;
            table.set("pressed", state.pressed)?;
            table.set("released", state.released)?;
            table.set("held", state.held)?;
            table.set("axis", state.axis)?;
            Ok(table)
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "key",
        lua.create_function(move |lua, key: String| {
            let state = lock_host(&h)?.input_frame().key(&key);
            let result = lua.create_table()?;
            result.set("pressed", state.pressed)?;
            result.set("released", state.released)?;
            result.set("held", state.held)?;
            result.set("axis", state.axis)?;
            Ok(result)
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "key_events",
        lua.create_function(move |lua, ()| {
            let events = lock_host(&h)?.input_frame().key_events;
            let result = lua.create_table()?;
            for (index, event) in events.into_iter().enumerate() {
                let item = lua.create_table()?;
                item.set("key", event.key)?;
                item.set(
                    "state",
                    match event.state {
                        crate::KeyEventState::Pressed => "pressed",
                        crate::KeyEventState::Released => "released",
                    },
                )?;
                item.set("repeat", event.repeat)?;
                result.set(index + 1, item)?;
            }
            Ok(result)
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "any_key_pressed",
        lua.create_function(move |_, ()| Ok(lock_host(&h)?.input_frame().any_key_pressed()))?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "log",
        lua.create_function(move |_, (level, message): (String, String)| {
            lock_host(&h)?
                .log(&level, &message)
                .map_err(MluaError::RuntimeError)
        })?,
    )?;
    for (name, level) in [("print", "info"), ("warn", "warn")] {
        let h = Arc::clone(&host);
        lua.globals().set(
            name,
            lua.create_function(move |lua, values: Variadic<Value>| {
                let tostring: Function = lua.globals().get("tostring")?;
                let mut parts = Vec::with_capacity(values.len());
                for value in values {
                    parts.push(tostring.call::<String>(value)?);
                }
                lock_host(&h)?
                    .log(level, &parts.join("\t"))
                    .map_err(MluaError::RuntimeError)
            })?,
        )?;
    }
    let h = Arc::clone(&host);
    api.set(
        "get_property",
        lua.create_function(move |lua, name: String| {
            engine_to_lua(lua, lock_host(&h)?.property(&name))
        })?,
    )?;
    let h = Arc::clone(&host);
    api.set(
        "set_property",
        lua.create_function(move |_, (name, value): (String, Value)| {
            let value = lua_to_engine(value)?;
            lock_host(&h)?
                .set_property(&name, value)
                .map_err(MluaError::RuntimeError)
        })?,
    )?;
    api.set(
        "set_enabled",
        lua.create_function(move |_, enabled: bool| {
            lock_host(&host)?.set_enabled(enabled);
            Ok(())
        })?,
    )?;
    lua.globals().set("rustic", api)
}

fn engine_to_lua(lua: &Lua, value: Option<EngineValue>) -> mlua::Result<Value> {
    Ok(match value {
        None | Some(EngineValue::Entity(None)) => Value::Nil,
        Some(EngineValue::Boolean(v)) => Value::Boolean(v),
        Some(EngineValue::Integer(v)) => Value::Integer(v),
        Some(EngineValue::Number(v)) => Value::Number(v),
        Some(EngineValue::String(v)) => Value::String(lua.create_string(&v)?),
        Some(EngineValue::Entity(Some(id))) => Value::String(lua.create_string(id.to_string())?),
        Some(EngineValue::Vec2(v)) => {
            let t = lua.create_table()?;
            t.set(1, v[0])?;
            t.set(2, v[1])?;
            Value::Table(t)
        }
        Some(EngineValue::Vec3(v)) => {
            let t = lua.create_table()?;
            for (i, n) in v.into_iter().enumerate() {
                t.set(i + 1, n)?;
            }
            Value::Table(t)
        }
    })
}
fn lua_to_engine(value: Value) -> mlua::Result<EngineValue> {
    match value {
        Value::Boolean(v) => Ok(EngineValue::Boolean(v)),
        Value::Integer(v) => Ok(EngineValue::Integer(v)),
        Value::Number(v) => Ok(EngineValue::Number(v)),
        Value::String(v) => Ok(EngineValue::String(v.to_str()?.to_owned())),
        _ => Err(MluaError::RuntimeError(
            "properties accept boolean, integer, number, or string values".into(),
        )),
    }
}
fn runtime_error(error: &impl ToString) -> LuaRuntimeError {
    LuaRuntimeError::Runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct Host {
        entity: EntityId,
        translation: [f64; 3],
        logs: Vec<String>,
        props: BTreeMap<String, EngineValue>,
        enabled: bool,
    }
    impl GameplayHost for Host {
        fn entity_id(&self) -> EntityId {
            self.entity
        }
        fn delta_time(&self) -> f64 {
            0.016
        }
        fn fixed_delta_time(&self) -> f64 {
            0.02
        }
        fn translation(&self) -> [f64; 3] {
            self.translation
        }
        fn set_translation(&mut self, v: [f64; 3]) -> Result<(), String> {
            if v.iter().all(|n| n.is_finite()) {
                self.translation = v;
                Ok(())
            } else {
                Err("non-finite transform".into())
            }
        }
        fn find_entity(&self, q: &str) -> Result<Option<EntityId>, String> {
            Ok((q == "self").then_some(self.entity))
        }
        fn input_action(&self, _: &str) -> ActionState {
            ActionState::default()
        }
        fn log(&mut self, _: &str, m: &str) -> Result<(), String> {
            self.logs.push(m.into());
            Ok(())
        }
        fn property(&self, n: &str) -> Option<EngineValue> {
            self.props.get(n).cloned()
        }
        fn set_property(&mut self, n: &str, v: EngineValue) -> Result<(), String> {
            let old = self
                .props
                .get(n)
                .ok_or_else(|| format!("property `{n}` is not declared"))?;
            if !old.same_type(&v) {
                return Err("property type mismatch".into());
            }
            self.props.insert(n.into(), v);
            Ok(())
        }
        fn set_enabled(&mut self, v: bool) {
            self.enabled = v;
        }
    }
    fn host() -> Box<dyn GameplayHost> {
        Box::new(Host {
            entity: EntityId::new(),
            translation: [0.0; 3],
            logs: Vec::new(),
            props: BTreeMap::new(),
            enabled: true,
        })
    }
    #[test]
    fn lifecycle_executes_api_and_sandbox_denies_host_capabilities() {
        let source=br"assert(io == nil and os == nil and package == nil and require == nil)
return { on_create=function() rustic.log('info','created') end, fixed_update=function(dt) local x,y,z=rustic.get_translation(); rustic.set_translation(x+dt,y,z) end }";
        let mut b =
            LuaBehavior::load(ScriptId::new(), source, "move.lua", host(), 100_000).unwrap();
        b.on_create().unwrap();
        b.fixed_update(0.02).unwrap();
        assert!((b.host().lock().unwrap().translation()[0] - 0.02).abs() < f64::EPSILON);
    }
    #[test]
    fn canonical_lifecycle_and_native_logging_functions_are_available() {
        let source = br#"return {
            OnCreate=function() print("created", 1); warn("careful"); rustic.set_translation(1,2,3) end,
            OnStop=function() rustic.set_translation(4,5,6) end
        }"#;
        let mut behavior =
            LuaBehavior::load(ScriptId::new(), source, "native-log.lua", host(), 100_000).unwrap();

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
    fn syntax_error_is_diagnostic_and_infinite_loop_is_contained() {
        assert!(validate_lua(b"return { broken =", "bad.lua", 1024).is_err());
        let mut b = LuaBehavior::load(
            ScriptId::new(),
            b"return { update=function() while true do end end }",
            "loop.lua",
            host(),
            10_000,
        )
        .unwrap();
        assert!(matches!(
            b.update(0.1),
            Err(LuaRuntimeError::BudgetExhausted)
        ));
        assert!(!b.enabled());
    }

    #[test]
    fn api_smoke_entry_allows_an_undeclared_optional_property() {
        let mut behavior = LuaBehavior::load(
            ScriptId::new(),
            include_bytes!("../../../examples/api-smoke-test/entry.lua"),
            "entry.lua",
            host(),
            100_000,
        )
        .unwrap();

        behavior.on_create().unwrap();
        assert!(behavior.enabled());
    }
}
