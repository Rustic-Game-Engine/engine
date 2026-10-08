use super::{Owner, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Payload {
    Value(Value),
    String(String),
    Boolean(bool),
}
impl From<Value> for Payload {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}
impl Payload {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Value(v) => v.validate(),
            Self::String(s) if s.len() > 4096 => Err("signal string exceeds 4096 bytes".into()),
            _ => Ok(()),
        }
    }
}
use crate::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub name: String,
    #[serde(default)]
    pub source: Option<EntityId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delivery {
    pub owner: Owner,
    pub token: String,
    pub arguments: Vec<Payload>,
    #[serde(skip)]
    pub source: Option<EntityId>,
}
#[derive(Clone)]
struct Subscription {
    serial: u64,
    owner: Owner,
    signal: Signal,
    token: String,
    once: bool,
}
/// Snapshot delivery: reentrant emits are consumed on the next frame.
#[derive(Default)]
pub struct Signals {
    subscriptions: BTreeMap<(Owner, String), Subscription>,
    pending: VecDeque<Delivery>,
    next_serial: u64,
}
impl Signals {
    /// # Errors
    /// Rejects invalid names/tokens, duplicate connections, or subscription capacity exhaustion.
    pub fn connect(
        &mut self,
        owner: Owner,
        token: String,
        signal: Signal,
        once: bool,
    ) -> Result<(), String> {
        if signal.name.is_empty() || signal.name.len() > 128 || token.len() > 128 {
            return Err("invalid signal name/token".into());
        }
        if self.subscriptions.len() >= 4096 {
            return Err("signal subscription limit reached".into());
        }
        let key = (owner, token.clone());
        if self.subscriptions.contains_key(&key) {
            return Err("subscription token already connected".into());
        }
        self.next_serial = self
            .next_serial
            .checked_add(1)
            .ok_or("signal serial exhausted")?;
        self.subscriptions.insert(
            key,
            Subscription {
                serial: self.next_serial,
                owner,
                signal,
                token,
                once,
            },
        );
        Ok(())
    }
    pub fn disconnect(&mut self, owner: Owner, token: &str) {
        self.subscriptions.remove(&(owner, token.into()));
        self.pending
            .retain(|d| d.owner != owner || d.token != token);
    }
    /// # Errors
    /// Rejects invalid or oversized payloads and pending-delivery capacity exhaustion.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Emitted arguments form an owned message boundary"
    )]
    pub fn emit(&mut self, signal: &Signal, arguments: Vec<Payload>) -> Result<(), String> {
        if arguments.len() > 64 {
            return Err("signal argument limit reached".into());
        }
        let payload_bytes: usize = arguments
            .iter()
            .map(|v| match v {
                Payload::String(s) => s.len(),
                _ => 32,
            })
            .sum();
        if payload_bytes > 16_384 {
            return Err("signal payload exceeds 16 KiB".into());
        }
        for value in &arguments {
            value.validate()?;
        }
        let mut targets: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|(_, s)| s.signal == *signal)
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect();
        targets.sort_by_key(|(_, s)| s.serial);
        if self.pending.len() + targets.len() > 4096 {
            return Err("signal delivery limit reached".into());
        }
        for (key, s) in targets {
            self.pending.push_back(Delivery {
                owner: s.owner,
                token: s.token,
                arguments: arguments.clone(),
                source: signal.source,
            });
            if s.once {
                self.subscriptions.remove(&key);
            }
        }
        Ok(())
    }
    pub fn cleanup(&mut self, predicate: impl Fn(Owner) -> bool) {
        self.subscriptions.retain(|_, s| !predicate(s.owner));
        self.pending.retain(|d| !predicate(d.owner));
    }
    pub fn destroy_entity(&mut self, entity: EntityId) {
        self.subscriptions.retain(|_, s| {
            (!s.owner.entity_bound || s.owner.entity != entity) && s.signal.source != Some(entity)
        });
        self.pending.retain(|d| {
            (!d.owner.entity_bound || d.owner.entity != entity) && d.source != Some(entity)
        });
    }
    pub fn entities(&self) -> Vec<EntityId> {
        let mut result = Vec::new();
        for s in self.subscriptions.values() {
            if s.owner.entity_bound {
                result.push(s.owner.entity);
            }
            if let Some(source) = s.signal.source {
                result.push(source);
            }
        }
        for d in &self.pending {
            if d.owner.entity_bound {
                result.push(d.owner.entity);
            }
            if let Some(source) = d.source {
                result.push(source);
            }
        }
        result
    }
    pub fn tokens(&self, owner: Owner) -> Vec<String> {
        let mut tokens = self
            .subscriptions
            .values()
            .filter(|s| s.owner == owner)
            .map(|s| s.token.clone())
            .chain(
                self.pending
                    .iter()
                    .filter(|d| d.owner == owner)
                    .map(|d| d.token.clone()),
            )
            .collect::<Vec<_>>();
        tokens.sort();
        tokens.dedup();
        tokens
    }
    pub fn drain(&mut self, owner: Owner) -> Vec<Delivery> {
        let mut result = Vec::new();
        self.pending.retain(|d| {
            if d.owner == owner {
                result.push(d.clone());
                false
            } else {
                true
            }
        });
        result
    }
}
