//! Explicit, reviewable, undoable runtime-change transactions.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

/// Language-neutral value allowed in a reviewed runtime diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuntimeValue {
    Null,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Vector2([f64; 2]),
    Vector3([f64; 3]),
    Vector4([f64; 4]),
}

/// Stable property identity independent of runtime generational handles.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuntimeChangeTarget {
    pub entity_id: String,
    pub component_schema: String,
    pub property: String,
}

/// Before/after pair presented for explicit user review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeChange {
    pub target: RuntimeChangeTarget,
    pub before: RuntimeValue,
    pub after: RuntimeValue,
}

/// Diff produced by a runtime that started from `base_scene_sha256`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeChangeSet {
    pub format_version: u32,
    pub base_scene_sha256: String,
    pub changes: Vec<RuntimeChange>,
}

impl RuntimeChangeSet {
    pub const FORMAT_VERSION: u32 = 1;

    pub fn new(base_scene_sha256: impl Into<String>, changes: Vec<RuntimeChange>) -> Self {
        Self {
            format_version: Self::FORMAT_VERSION,
            base_scene_sha256: base_scene_sha256.into(),
            changes,
        }
    }
}

/// Minimal authoring boundary required by the apply transaction.
pub trait RuntimeChangeStore {
    fn source_scene_sha256(&self) -> &str;
    fn value(&self, target: &RuntimeChangeTarget) -> Option<RuntimeValue>;
    /// Replaces one property value.
    ///
    /// # Errors
    ///
    /// Returns adapter-specific validation or mutation context.
    fn set_value(
        &mut self,
        target: &RuntimeChangeTarget,
        value: RuntimeValue,
    ) -> Result<(), String>;
}

/// Simple engine-owned map store useful for headless workflows and adapters.
#[derive(Debug, Clone, Default)]
pub struct MapChangeStore {
    source_scene_sha256: String,
    values: BTreeMap<RuntimeChangeTarget, RuntimeValue>,
}

impl MapChangeStore {
    pub fn new(source_scene_sha256: impl Into<String>) -> Self {
        Self {
            source_scene_sha256: source_scene_sha256.into(),
            values: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, target: RuntimeChangeTarget, value: RuntimeValue) {
        self.values.insert(target, value);
    }

    pub fn get(&self, target: &RuntimeChangeTarget) -> Option<&RuntimeValue> {
        self.values.get(target)
    }
}

impl RuntimeChangeStore for MapChangeStore {
    fn source_scene_sha256(&self) -> &str {
        &self.source_scene_sha256
    }

    fn value(&self, target: &RuntimeChangeTarget) -> Option<RuntimeValue> {
        self.values.get(target).cloned()
    }

    fn set_value(
        &mut self,
        target: &RuntimeChangeTarget,
        value: RuntimeValue,
    ) -> Result<(), String> {
        self.values.insert(target.clone(), value);
        Ok(())
    }
}

/// One centralized authoring transaction. Construction does not mutate source data.
#[derive(Debug, Clone)]
pub struct ApplyRuntimeChangesTransaction {
    change_set: RuntimeChangeSet,
    applied: bool,
}

impl ApplyRuntimeChangesTransaction {
    /// Validates schema and duplicate targets before a review UI presents the transaction.
    ///
    /// # Errors
    ///
    /// Rejects unknown format versions and duplicate property edits.
    pub fn new(change_set: RuntimeChangeSet) -> Result<Self, RuntimeChangeError> {
        if change_set.format_version != RuntimeChangeSet::FORMAT_VERSION {
            return Err(RuntimeChangeError::UnsupportedVersion {
                found: change_set.format_version,
                supported: RuntimeChangeSet::FORMAT_VERSION,
            });
        }
        let mut targets = HashSet::new();
        for change in &change_set.changes {
            if !targets.insert(&change.target) {
                return Err(RuntimeChangeError::DuplicateTarget(change.target.clone()));
            }
        }
        Ok(Self {
            change_set,
            applied: false,
        })
    }

    pub const fn change_set(&self) -> &RuntimeChangeSet {
        &self.change_set
    }

    pub const fn is_applied(&self) -> bool {
        self.applied
    }

    /// Applies every reviewed change or leaves the store at its prior values.
    ///
    /// # Errors
    ///
    /// Refuses changed source hashes/property preconditions and rolls back a partial
    /// adapter failure before returning.
    pub fn apply(&mut self, store: &mut impl RuntimeChangeStore) -> Result<(), RuntimeChangeError> {
        if self.applied {
            return Err(RuntimeChangeError::AlreadyApplied);
        }
        if store.source_scene_sha256() != self.change_set.base_scene_sha256 {
            return Err(RuntimeChangeError::SourceSceneChanged {
                expected: self.change_set.base_scene_sha256.clone(),
                actual: store.source_scene_sha256().to_owned(),
            });
        }
        for change in &self.change_set.changes {
            let actual = store.value(&change.target);
            if actual.as_ref() != Some(&change.before) {
                return Err(RuntimeChangeError::PropertyConflict {
                    target: Box::new(change.target.clone()),
                    expected: Box::new(change.before.clone()),
                    actual: Box::new(actual),
                });
            }
        }
        for (completed, change) in self.change_set.changes.iter().enumerate() {
            if let Err(error) = store.set_value(&change.target, change.after.clone()) {
                rollback_apply(store, &self.change_set.changes[..completed])?;
                return Err(RuntimeChangeError::Store(error));
            }
        }
        self.applied = true;
        Ok(())
    }

    /// Reverts an applied transaction with the same all-or-nothing preconditions.
    ///
    /// # Errors
    ///
    /// Refuses an unapplied transaction or properties changed after apply; partial
    /// adapter failures are restored to their applied values.
    pub fn undo(&mut self, store: &mut impl RuntimeChangeStore) -> Result<(), RuntimeChangeError> {
        if !self.applied {
            return Err(RuntimeChangeError::NotApplied);
        }
        for change in &self.change_set.changes {
            let actual = store.value(&change.target);
            if actual.as_ref() != Some(&change.after) {
                return Err(RuntimeChangeError::PropertyConflict {
                    target: Box::new(change.target.clone()),
                    expected: Box::new(change.after.clone()),
                    actual: Box::new(actual),
                });
            }
        }
        let mut completed = Vec::new();
        for change in self.change_set.changes.iter().rev() {
            if let Err(error) = store.set_value(&change.target, change.before.clone()) {
                rollback_undo(store, &completed)?;
                return Err(RuntimeChangeError::Store(error));
            }
            completed.push(change);
        }
        self.applied = false;
        Ok(())
    }
}

fn rollback_apply(
    store: &mut impl RuntimeChangeStore,
    completed: &[RuntimeChange],
) -> Result<(), RuntimeChangeError> {
    for change in completed.iter().rev() {
        store
            .set_value(&change.target, change.before.clone())
            .map_err(RuntimeChangeError::Rollback)?;
    }
    Ok(())
}

fn rollback_undo(
    store: &mut impl RuntimeChangeStore,
    completed: &[&RuntimeChange],
) -> Result<(), RuntimeChangeError> {
    for change in completed.iter().rev() {
        store
            .set_value(&change.target, change.after.clone())
            .map_err(RuntimeChangeError::Rollback)?;
    }
    Ok(())
}

/// Runtime-diff validation, conflict, and adapter errors.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RuntimeChangeError {
    #[error("runtime-change format {found} is unsupported; expected {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("runtime-change set contains duplicate target {0:?}")]
    DuplicateTarget(RuntimeChangeTarget),
    #[error("source scene changed since play; expected {expected}, got {actual}")]
    SourceSceneChanged { expected: String, actual: String },
    #[error("runtime-change property conflict at {target:?}")]
    PropertyConflict {
        target: Box<RuntimeChangeTarget>,
        expected: Box<RuntimeValue>,
        actual: Box<Option<RuntimeValue>>,
    },
    #[error("runtime-change transaction is already applied")]
    AlreadyApplied,
    #[error("runtime-change transaction has not been applied")]
    NotApplied,
    #[error("authoring store rejected runtime change: {0}")]
    Store(String),
    #[error("runtime-change rollback failed: {0}")]
    Rollback(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(property: &str) -> RuntimeChangeTarget {
        RuntimeChangeTarget {
            entity_id: "entity-1".to_owned(),
            component_schema: "transform.v1".to_owned(),
            property: property.to_owned(),
        }
    }

    fn transaction() -> ApplyRuntimeChangesTransaction {
        ApplyRuntimeChangesTransaction::new(RuntimeChangeSet::new(
            "base-hash",
            vec![RuntimeChange {
                target: target("x"),
                before: RuntimeValue::Number(1.0),
                after: RuntimeValue::Number(2.0),
            }],
        ))
        .unwrap()
    }

    #[test]
    fn apply_is_explicit_and_undo_restores_exact_value() {
        let mut store = MapChangeStore::new("base-hash");
        store.insert(target("x"), RuntimeValue::Number(1.0));
        let mut transaction = transaction();
        assert_eq!(store.get(&target("x")), Some(&RuntimeValue::Number(1.0)));
        transaction.apply(&mut store).unwrap();
        assert_eq!(store.get(&target("x")), Some(&RuntimeValue::Number(2.0)));
        transaction.undo(&mut store).unwrap();
        assert_eq!(store.get(&target("x")), Some(&RuntimeValue::Number(1.0)));
    }

    #[test]
    fn source_or_property_conflict_never_partially_mutates() {
        let mut wrong_hash = MapChangeStore::new("new-authoring-hash");
        wrong_hash.insert(target("x"), RuntimeValue::Number(1.0));
        assert!(matches!(
            transaction().apply(&mut wrong_hash),
            Err(RuntimeChangeError::SourceSceneChanged { .. })
        ));
        assert_eq!(
            wrong_hash.get(&target("x")),
            Some(&RuntimeValue::Number(1.0))
        );

        let mut conflict = MapChangeStore::new("base-hash");
        conflict.insert(target("x"), RuntimeValue::Number(99.0));
        assert!(matches!(
            transaction().apply(&mut conflict),
            Err(RuntimeChangeError::PropertyConflict { .. })
        ));
        assert_eq!(
            conflict.get(&target("x")),
            Some(&RuntimeValue::Number(99.0))
        );
    }

    #[test]
    fn duplicate_targets_are_rejected() {
        let change = RuntimeChange {
            target: target("x"),
            before: RuntimeValue::Null,
            after: RuntimeValue::Bool(true),
        };
        assert!(matches!(
            ApplyRuntimeChangesTransaction::new(RuntimeChangeSet::new(
                "base",
                vec![change.clone(), change]
            )),
            Err(RuntimeChangeError::DuplicateTarget(_))
        ));
    }
}
