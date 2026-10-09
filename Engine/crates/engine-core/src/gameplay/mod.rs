//! Language-neutral gameplay actions. No scripting VM values cross this boundary.
pub mod animation;
pub mod audio;
pub mod ease;
pub mod path;
pub mod physics;
pub mod procedural;
pub mod query;
pub mod runtime;
pub mod signals;
pub mod smooth;
pub use ease::Ease;
pub use path::{Path, PathKind, PathPoint};
pub use runtime::*;
pub use signals::*;

use crate::{EntityId, SceneId, ScriptId};
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Owner {
    /// Scene/global behaviors survive unrelated object destruction.
    pub entity_bound: bool,
    pub scene: SceneId,
    pub entity: EntityId,
    pub script: ScriptId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub entity: EntityId,
    pub property: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Vector([f64; 3]),
    Rotation([f64; 4]),
}
impl Value {
    /// # Errors
    /// Rejects non-finite values, invalid timing, incompatible types, or oversized action trees.
    pub fn validate(self) -> Result<(), String> {
        let valid = match self {
            Self::Number(v) => v.is_finite(),
            Self::Vector(v) => v.iter().all(|v| v.is_finite()),
            Self::Rotation(v) => {
                v.iter().all(|v| v.is_finite())
                    && DQuat::from_array(v).length_squared().is_finite()
                    && DQuat::from_array(v).length_squared() > 1e-12
            }
        };
        if valid {
            Ok(())
        } else {
            Err("value is non-finite or quaternion has zero length".into())
        }
    }
    /// # Errors
    /// Rejects incompatible value types. Inputs must already pass validate.
    pub fn interpolate(self, to: Self, progress: f64) -> Result<Self, String> {
        self.validate()?;
        to.validate()?;
        let result = match (self, to) {
            (Self::Number(a), Self::Number(b)) => Ok(Self::Number(a + (b - a) * progress)),
            (Self::Vector(a), Self::Vector(b)) => Ok(Self::Vector(
                DVec3::from_array(a)
                    .lerp(DVec3::from_array(b), progress)
                    .to_array(),
            )),
            (Self::Rotation(a), Self::Rotation(b)) => Ok(Self::Rotation(
                DQuat::from_array(a)
                    .normalize()
                    .slerp(DQuat::from_array(b).normalize(), progress)
                    .normalize()
                    .to_array(),
            )),
            _ => Err("interpolation value types differ".to_owned()),
        }?;
        result.validate()?;
        Ok(result)
    }
    /// # Errors
    /// Rejects incompatible value types.
    pub fn distance(self, to: Self) -> Result<f64, String> {
        self.validate()?;
        to.validate()?;
        let result = match (self, to) {
            (Self::Number(a), Self::Number(b)) => Ok((b - a).abs()),
            (Self::Vector(a), Self::Vector(b)) => {
                Ok(DVec3::from_array(a).distance(DVec3::from_array(b)))
            }
            (Self::Rotation(a), Self::Rotation(b)) => Ok(2.0
                * DQuat::from_array(a)
                    .normalize()
                    .dot(DQuat::from_array(b).normalize())
                    .abs()
                    .clamp(0.0, 1.0)
                    .acos()),
            _ => Err("movement value types differ".to_owned()),
        }?;
        if result.is_finite() {
            Ok(result)
        } else {
            Err("movement distance overflowed".into())
        }
    }
}
/// A duration in seconds or a speed in units/second (radians/second for rotations).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Timing {
    Duration { duration: f64 },
    Speed { speed: f64 },
}
impl Timing {
    /// # Errors
    /// Rejects non-finite values, invalid timing, incompatible types, or oversized action trees.
    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Duration { duration } if duration.is_finite() && duration >= 0.0 => Ok(()),
            Self::Speed { speed } if speed.is_finite() && speed > 0.0 => Ok(()),
            _ => Err(
                "duration must be finite and non-negative; speed must be finite and positive"
                    .into(),
            ),
        }
    }
    fn duration(self, distance: f64) -> f64 {
        match self {
            Self::Duration { duration } => duration,
            Self::Speed { speed } => distance / speed,
        }
    }
}
/// Property access is implemented by the scene runtime, not by each language.
pub trait GameplayWorld {
    fn contains(&self, entity: EntityId) -> bool;
    /// # Errors
    /// Rejects unknown clip tracks and unsupported backend properties.
    fn animation_target(&self, entity: EntityId, track: &str) -> Result<Target, String> {
        Ok(Target {
            entity,
            property: track.into(),
        })
    }
    /// # Errors
    /// Rejects missing entities or unsupported properties.
    fn read(&self, target: &Target) -> Result<Value, String>;
    /// # Errors
    /// Rejects missing entities, unsupported properties, invalid values, or backend failures.
    fn write(&mut self, target: &Target, value: Value) -> Result<(), String>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Named clips remain in the engine and are resolved before scheduling.
    AnimationRef {
        entity: EntityId,
        clip: animation::ClipReference,
        #[serde(default)]
        options: animation::AnimationOptions,
        #[serde(default)]
        marker_token: Option<String>,
        #[serde(default)]
        blend_source: Option<animation::ClipReference>,
        #[serde(default)]
        mask: Vec<String>,
    },
    Animation {
        entity: EntityId,
        clip: animation::Clip,
        #[serde(default)]
        options: animation::AnimationOptions,
        #[serde(default)]
        marker_token: Option<String>,
        #[serde(default)]
        blend_source: Option<animation::Clip>,
        #[serde(default)]
        mask: Vec<String>,
    },
    Value {
        from: Value,
        to: Value,
        #[serde(flatten)]
        timing: Timing,
        #[serde(default)]
        easing: Ease,
        token: String,
    },
    Tween {
        target: Target,
        #[serde(default)]
        from: Option<Value>,
        to: Value,
        #[serde(flatten)]
        timing: Timing,
        #[serde(default)]
        easing: Ease,
    },
    Move {
        entity: EntityId,
        offset: [f64; 3],
        #[serde(flatten)]
        timing: Timing,
        #[serde(default)]
        easing: Ease,
    },
    LookAt {
        entity: EntityId,
        position: [f64; 3],
        duration: f64,
        #[serde(default)]
        easing: Ease,
    },
    Shake {
        entity: EntityId,
        strength: f64,
        duration: f64,
        #[serde(default)]
        easing: Ease,
    },
    Path {
        entity: EntityId,
        path: Path,
        #[serde(flatten)]
        timing: Timing,
        #[serde(default)]
        easing: Ease,
        #[serde(default)]
        orient_to_path: bool,
    },
    Follow {
        entity: EntityId,
        target: EntityId,
        #[serde(default)]
        offset: [f64; 3],
        duration: f64,
        #[serde(default)]
        easing: Ease,
    },
    Orbit {
        entity: EntityId,
        center: [f64; 3],
        radius: f64,
        turns: f64,
        duration: f64,
        #[serde(default)]
        easing: Ease,
    },
    Wait {
        duration: f64,
    },
    Callback {
        token: String,
    },
    Sequence {
        actions: Vec<Action>,
    },
    Parallel {
        actions: Vec<Action>,
    },
}
impl Action {
    /// Resolve engine clip handles throughout a bounded action tree.
    /// # Errors
    /// Rejects invalid actions and unknown clips before changing the scheduler.
    pub fn resolve_clips(
        &mut self,
        lookup: &impl Fn(EntityId, &str) -> Result<animation::Clip, String>,
    ) -> Result<(), String> {
        self.validate()?;
        self.resolve_clips_inner(lookup)
    }
    fn resolve_clips_inner(
        &mut self,
        lookup: &impl Fn(EntityId, &str) -> Result<animation::Clip, String>,
    ) -> Result<(), String> {
        match self {
            Self::AnimationRef {
                entity,
                clip,
                options,
                marker_token,
                blend_source,
                mask,
            } => {
                let resolve = |source: &animation::ClipReference| match source {
                    animation::ClipReference::Named(name) => lookup(*entity, name),
                    animation::ClipReference::Inline(clip) => Ok(clip.clone()),
                };
                *self = Self::Animation {
                    entity: *entity,
                    clip: resolve(clip)?,
                    options: *options,
                    marker_token: marker_token.clone(),
                    blend_source: blend_source.as_ref().map(resolve).transpose()?,
                    mask: mask.clone(),
                };
            }
            Self::Sequence { actions } | Self::Parallel { actions } => {
                for action in actions {
                    action.resolve_clips_inner(lookup)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn has_clip_references(&self) -> bool {
        match self {
            Self::AnimationRef { .. } => true,
            Self::Sequence { actions } | Self::Parallel { actions } => {
                actions.iter().any(Self::has_clip_references)
            }
            _ => false,
        }
    }
    pub fn tween(target: Target, to: Value, duration: f64, easing: Ease) -> Self {
        Self::Tween {
            target,
            from: None,
            to,
            timing: Timing::Duration { duration },
            easing,
        }
    }
    /// # Errors
    /// Rejects non-finite values, invalid timing, incompatible types, or oversized action trees.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_tree(0, &mut 0)
    }
    #[allow(
        clippy::too_many_lines,
        reason = "Exhaustive validation of the closed action protocol"
    )]
    fn validate_tree(&self, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > 32 || *count > 4096 {
            return Err("action tree exceeds depth/node limit".into());
        }
        let vector = |v: &[f64]| -> Result<(), String> {
            if v.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err("non-finite action value".into())
            }
        };
        match self {
            Self::AnimationRef {
                clip,
                blend_source,
                options,
                marker_token,
                mask,
                ..
            } => {
                clip.validate()?;
                if let Some(source) = blend_source {
                    source.validate()?;
                }
                animation::ClipPlayer::new(*options)?;
                if mask.len() > 4096
                    || mask.iter().any(|s| s.len() > 128)
                    || marker_token.as_ref().is_some_and(|s| s.len() > 128)
                {
                    return Err("invalid animation mask or marker token".into());
                }
                Ok(())
            }
            Self::Animation {
                blend_source,
                mask,
                clip,
                options,
                marker_token,
                ..
            } => {
                clip.validate()?;
                if let Some(source) = blend_source {
                    source.validate()?;
                }
                if mask.len() > 4096 || mask.iter().any(|s| s.len() > 128) {
                    return Err("invalid animation mask".into());
                }
                animation::ClipPlayer::new(*options)?;
                if marker_token.as_ref().is_some_and(|s| s.len() > 128) {
                    return Err("marker token too long".into());
                }
                Ok(())
            }
            Self::Value {
                from,
                to,
                timing,
                token,
                ..
            } => {
                from.validate()?;
                to.validate()?;
                from.distance(*to)?;
                if token.len() > 128 {
                    return Err("value callback token too long".into());
                }
                timing.validate()
            }
            Self::Tween {
                target,
                from,
                to,
                timing,
                ..
            } => {
                if target.property.is_empty() || target.property.len() > 128 {
                    return Err("invalid property name".into());
                }
                to.validate()?;
                if let Some(v) = from {
                    v.validate()?;
                    v.distance(*to)?;
                }
                timing.validate()
            }
            Self::Move { offset, timing, .. } => {
                vector(offset)?;
                timing.validate()
            }
            Self::LookAt {
                position, duration, ..
            } => {
                vector(position)?;
                Timing::Duration {
                    duration: *duration,
                }
                .validate()
            }
            Self::Shake {
                strength, duration, ..
            } => {
                vector(&[*strength])?;
                if *strength < 0.0 {
                    return Err("shake strength must be non-negative".into());
                }
                Timing::Duration {
                    duration: *duration,
                }
                .validate()
            }
            Self::Path { path, timing, .. } => {
                path.validate()?;
                timing.validate()
            }
            Self::Follow {
                duration, offset, ..
            } => {
                vector(offset)?;
                Timing::Duration {
                    duration: *duration,
                }
                .validate()
            }
            Self::Orbit {
                center,
                radius,
                turns,
                duration,
                ..
            } => {
                vector(center)?;
                vector(&[*radius, *turns])?;
                Timing::Duration {
                    duration: *duration,
                }
                .validate()
            }
            Self::Wait { duration } => Timing::Duration {
                duration: *duration,
            }
            .validate(),
            Self::Callback { token } => {
                if token.len() <= 128 {
                    Ok(())
                } else {
                    Err("callback token too long".into())
                }
            }
            Self::Sequence { actions } | Self::Parallel { actions } => {
                for a in actions {
                    a.validate_tree(depth + 1, count)?;
                }
                Ok(())
            }
        }
    }
    pub(crate) fn node_count(&self) -> usize {
        match self {
            Self::Sequence { actions } | Self::Parallel { actions } => {
                1 + actions.iter().map(Self::node_count).sum::<usize>()
            }
            _ => 1,
        }
    }
    pub(crate) fn references(&self, entity: EntityId) -> bool {
        match self {
            Self::Tween { target, .. } => target.entity == entity,
            Self::AnimationRef { entity: e, .. }
            | Self::Animation { entity: e, .. }
            | Self::Move { entity: e, .. }
            | Self::LookAt { entity: e, .. }
            | Self::Shake { entity: e, .. }
            | Self::Path { entity: e, .. }
            | Self::Orbit { entity: e, .. } => *e == entity,
            Self::Follow {
                entity: e, target, ..
            } => *e == entity || *target == entity,
            Self::Sequence { actions } | Self::Parallel { actions } => {
                actions.iter().any(|a| a.references(entity))
            }
            _ => false,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Playback {
    /// Number of extra traversals. None repeats indefinitely.
    #[serde(default = "no_repeats")]
    pub repeats: Option<u32>,
    #[serde(default)]
    pub ping_pong: bool,
    #[serde(default)]
    pub looping: bool,
}
#[allow(
    clippy::unnecessary_wraps,
    reason = "Serde default must match the optional repeat field type"
)]
fn no_repeats() -> Option<u32> {
    Some(0)
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            repeats: Some(0),
            ping_pong: false,
            looping: false,
        }
    }
}
