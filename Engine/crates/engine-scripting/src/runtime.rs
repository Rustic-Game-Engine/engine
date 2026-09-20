use crate::{EngineValue, EntityId, ScriptCallback, ScriptEvent, ScriptId, ScriptScope};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Opaque language-runtime instance. The core scheduler never inspects it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScriptInstanceHandle(pub u64);

/// Everything a language adapter needs to construct one isolated instance.
#[derive(Clone, Debug)]
pub struct ScriptInstanceDescriptor {
    pub asset_id: ScriptId,
    pub scope: ScriptScope,
    pub entity: Option<EntityId>,
    pub execution_order: i32,
    pub properties: BTreeMap<String, EngineValue>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Adapter {
        next: u64,
        assets: BTreeMap<ScriptInstanceHandle, ScriptId>,
        calls: Arc<Mutex<Vec<(ScriptId, ScriptCallback)>>>,
    }

    impl ScriptRuntimeAdapter for Adapter {
        fn instantiate(
            &mut self,
            descriptor: &ScriptInstanceDescriptor,
        ) -> Result<ScriptInstanceHandle, String> {
            let handle = ScriptInstanceHandle(self.next);
            self.next += 1;
            self.assets.insert(handle, descriptor.asset_id);
            Ok(handle)
        }
        fn implemented_callbacks(
            &self,
            _instance: ScriptInstanceHandle,
        ) -> BTreeSet<ScriptCallback> {
            [
                ScriptCallback::Start,
                ScriptCallback::Update,
                ScriptCallback::Enable,
                ScriptCallback::Disable,
                ScriptCallback::Destroy,
            ]
            .into_iter()
            .collect()
        }
        fn invoke(
            &mut self,
            instance: ScriptInstanceHandle,
            callback: ScriptCallback,
            _arguments: &[EngineValue],
        ) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push((self.assets[&instance], callback));
            Ok(())
        }
        fn unload(&mut self, instance: ScriptInstanceHandle) {
            self.assets.remove(&instance);
        }
    }

    fn descriptor(asset_id: ScriptId, scope: ScriptScope, order: i32) -> ScriptInstanceDescriptor {
        ScriptInstanceDescriptor {
            asset_id,
            scope,
            entity: (scope == ScriptScope::Component).then(EntityId::new),
            execution_order: order,
            properties: BTreeMap::new(),
        }
    }

    #[test]
    fn updates_are_scope_then_order_deterministic() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut scheduler = ScriptScheduler::default();
        scheduler.register_adapter(
            "test",
            Box::new(Adapter {
                next: 0,
                assets: BTreeMap::new(),
                calls: Arc::clone(&calls),
            }),
        );
        let component = ScriptId::new();
        let scene = ScriptId::new();
        let global_late = ScriptId::new();
        let global_early = ScriptId::new();
        scheduler
            .instantiate("test", descriptor(component, ScriptScope::Component, 0))
            .unwrap();
        scheduler
            .instantiate("test", descriptor(global_late, ScriptScope::Global, 10))
            .unwrap();
        scheduler
            .instantiate("test", descriptor(scene, ScriptScope::Scene, 0))
            .unwrap();
        scheduler
            .instantiate("test", descriptor(global_early, ScriptScope::Global, -10))
            .unwrap();
        calls.lock().unwrap().clear();
        scheduler.update(1.0 / 60.0).unwrap();
        let update_order = calls
            .lock()
            .unwrap()
            .iter()
            .map(|(asset, _)| *asset)
            .collect::<Vec<_>>();
        assert_eq!(update_order, [global_early, global_late, scene, component]);
    }
}

/// Narrow ABI implemented by Lua, JavaScript, and future language plugins.
#[allow(clippy::missing_errors_doc)]
pub trait ScriptRuntimeAdapter: Send {
    fn instantiate(
        &mut self,
        descriptor: &ScriptInstanceDescriptor,
    ) -> Result<ScriptInstanceHandle, String>;
    fn implemented_callbacks(&self, instance: ScriptInstanceHandle) -> BTreeSet<ScriptCallback>;
    fn invoke(
        &mut self,
        instance: ScriptInstanceHandle,
        callback: ScriptCallback,
        arguments: &[EngineValue],
    ) -> Result<(), String>;
    fn unload(&mut self, instance: ScriptInstanceHandle);
    /// Delivers a language-neutral Engine Event. Adapters may expose it as a
    /// native function, method, or message without leaking VM values.
    fn dispatch_event(
        &mut self,
        _instance: ScriptInstanceHandle,
        _event: &ScriptEvent,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExecutionKey {
    scope: u8,
    order: i32,
    entity: Option<EntityId>,
    asset: ScriptId,
    serial: u64,
}

struct Instance {
    descriptor: ScriptInstanceDescriptor,
    adapter: String,
    handle: ScriptInstanceHandle,
    callbacks: BTreeSet<ScriptCallback>,
    enabled: bool,
}

/// Deterministic, language-neutral owner of all running script instances.
///
/// Ordering is global, then scene, then component; within a scope it is explicit
/// execution order, entity ID, asset ID, and finally creation serial.
#[derive(Default)]
pub struct ScriptScheduler {
    adapters: BTreeMap<String, Box<dyn ScriptRuntimeAdapter>>,
    instances: BTreeMap<ExecutionKey, Instance>,
    events: VecDeque<ScriptEvent>,
    next_serial: u64,
}

#[allow(clippy::missing_errors_doc)]
impl ScriptScheduler {
    pub fn register_adapter(
        &mut self,
        language: impl Into<String>,
        adapter: Box<dyn ScriptRuntimeAdapter>,
    ) {
        self.adapters.insert(language.into(), adapter);
    }

    pub fn instantiate(
        &mut self,
        language: &str,
        descriptor: ScriptInstanceDescriptor,
    ) -> Result<(), String> {
        let adapter = self
            .adapters
            .get_mut(language)
            .ok_or_else(|| format!("no scripting adapter is registered for {language}"))?;
        let handle = adapter.instantiate(&descriptor)?;
        let callbacks = adapter.implemented_callbacks(handle);
        let serial = self.next_serial;
        self.next_serial = self.next_serial.saturating_add(1);
        let key = ExecutionKey {
            scope: match descriptor.scope {
                ScriptScope::Global => 0,
                ScriptScope::Scene => 1,
                ScriptScope::Component => 2,
            },
            order: descriptor.execution_order,
            entity: descriptor.entity,
            asset: descriptor.asset_id,
            serial,
        };
        self.instances.insert(
            key,
            Instance {
                descriptor,
                adapter: language.to_owned(),
                handle,
                callbacks,
                enabled: true,
            },
        );
        self.invoke_handle(language, handle, ScriptCallback::Enable, &[])?;
        self.invoke_handle(language, handle, ScriptCallback::Start, &[])
    }

    pub fn update(&mut self, delta_seconds: f64) -> Result<(), String> {
        self.invoke_all(
            ScriptCallback::Update,
            &[EngineValue::Number(delta_seconds)],
        )
    }

    pub fn fixed_update(&mut self, delta_seconds: f64) -> Result<(), String> {
        self.invoke_all(
            ScriptCallback::FixedUpdate,
            &[EngineValue::Number(delta_seconds)],
        )
    }

    pub fn collision(
        &mut self,
        entity: EntityId,
        other: EntityId,
        callback: ScriptCallback,
    ) -> Result<(), String> {
        debug_assert!(matches!(
            callback,
            ScriptCallback::CollisionEnter
                | ScriptCallback::CollisionStay
                | ScriptCallback::CollisionExit
        ));
        let targets = self
            .instances
            .values()
            .filter(|instance| instance.enabled && instance.descriptor.entity == Some(entity))
            .map(|instance| (instance.adapter.clone(), instance.handle))
            .collect::<Vec<_>>();
        for (adapter, handle) in targets {
            self.invoke_handle(
                &adapter,
                handle,
                callback,
                &[EngineValue::Entity(Some(other))],
            )?;
        }
        Ok(())
    }

    pub fn set_entity_enabled(&mut self, entity: EntityId, enabled: bool) -> Result<(), String> {
        let targets = self
            .instances
            .values_mut()
            .filter(|instance| {
                instance.descriptor.entity == Some(entity) && instance.enabled != enabled
            })
            .map(|instance| {
                instance.enabled = enabled;
                (instance.adapter.clone(), instance.handle)
            })
            .collect::<Vec<_>>();
        let callback = if enabled {
            ScriptCallback::Enable
        } else {
            ScriptCallback::Disable
        };
        for (adapter, handle) in targets {
            self.invoke_handle(&adapter, handle, callback, &[])?;
        }
        Ok(())
    }

    pub fn destroy_entity(&mut self, entity: EntityId) -> Result<(), String> {
        let keys = self
            .instances
            .iter()
            .filter(|(_, instance)| instance.descriptor.entity == Some(entity))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(instance) = self.instances.remove(&key) {
                if instance.enabled {
                    self.invoke_instance(&instance, ScriptCallback::Disable, &[])?;
                }
                self.invoke_instance(&instance, ScriptCallback::Destroy, &[])?;
                if let Some(adapter) = self.adapters.get_mut(&instance.adapter) {
                    adapter.unload(instance.handle);
                }
            }
        }
        Ok(())
    }

    pub fn emit(&mut self, event: ScriptEvent) {
        self.events.push_back(event);
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = ScriptEvent> + '_ {
        self.events.drain(..)
    }

    pub fn dispatch_events(&mut self) -> Result<usize, String> {
        let mut delivered = 0;
        while let Some(event) = self.events.pop_front() {
            let targets = self
                .instances
                .values()
                .filter(|instance| {
                    instance.enabled
                        && event
                            .target
                            .is_none_or(|target| target == instance.descriptor.asset_id)
                })
                .map(|instance| (instance.adapter.clone(), instance.handle))
                .collect::<Vec<_>>();
            for (adapter_name, handle) in targets {
                self.adapters
                    .get_mut(&adapter_name)
                    .ok_or_else(|| format!("scripting adapter {adapter_name} was unregistered"))?
                    .dispatch_event(handle, &event)?;
                delivered += 1;
            }
        }
        Ok(delivered)
    }

    fn invoke_all(
        &mut self,
        callback: ScriptCallback,
        arguments: &[EngineValue],
    ) -> Result<(), String> {
        let targets = self
            .instances
            .values()
            .filter(|instance| instance.enabled && instance.callbacks.contains(&callback))
            .map(|instance| (instance.adapter.clone(), instance.handle))
            .collect::<Vec<_>>();
        for (adapter, handle) in targets {
            self.invoke_handle(&adapter, handle, callback, arguments)?;
        }
        Ok(())
    }

    fn invoke_instance(
        &mut self,
        instance: &Instance,
        callback: ScriptCallback,
        arguments: &[EngineValue],
    ) -> Result<(), String> {
        if instance.callbacks.contains(&callback) {
            self.invoke_handle(&instance.adapter, instance.handle, callback, arguments)?;
        }
        Ok(())
    }

    fn invoke_handle(
        &mut self,
        adapter_name: &str,
        handle: ScriptInstanceHandle,
        callback: ScriptCallback,
        arguments: &[EngineValue],
    ) -> Result<(), String> {
        let implemented = self
            .instances
            .values()
            .find(|instance| instance.handle == handle && instance.adapter == adapter_name)
            .is_none_or(|instance| instance.callbacks.contains(&callback));
        if implemented {
            self.adapters
                .get_mut(adapter_name)
                .ok_or_else(|| format!("scripting adapter {adapter_name} was unregistered"))?
                .invoke(handle, callback, arguments)?;
        }
        Ok(())
    }
}
