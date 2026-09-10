use crate::{AssetId, EntityId};
use bevy_ecs::prelude::Component;
pub use engine_scripting::{EngineValue, ScriptId};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable authored identity stored alongside an ECS entity.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct StableEntity(pub EntityId);

/// Optional human-readable entity name.
#[derive(Component, Clone, Debug, Eq, PartialEq)]
pub struct Name(pub String);

/// Local translation, rotation, and scale relative to an optional parent.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LocalTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl LocalTransform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Converts the transform to a column-major affine matrix.
    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// Rejects values that cannot be serialized or rendered reliably.
    pub fn is_finite(self) -> bool {
        self.translation.is_finite()
            && self.rotation.is_finite()
            && self.rotation.length_squared() > f32::EPSILON
            && self.scale.is_finite()
            && self.scale.abs().min_element() > f32::EPSILON
    }
}

impl Default for LocalTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Cached world-space transform derived from the hierarchy.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WorldTransform(pub Mat4);

impl Default for WorldTransform {
    fn default() -> Self {
        Self(Mat4::IDENTITY)
    }
}

/// Stable parent link. Children are a derived cache.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct Parent(pub EntityId);

/// Deterministically ordered derived child list.
#[derive(Component, Clone, Debug, Default, Eq, PartialEq)]
pub struct Children(pub Vec<EntityId>);

/// Common editable properties shared by placed scene objects.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartAttributes {
    pub color: [f32; 4],
    pub can_touch: bool,
    pub can_collide: bool,
    pub anchored: bool,
}

impl Default for PartAttributes {
    fn default() -> Self {
        Self {
            color: [0.55, 0.62, 0.72, 1.0],
            can_touch: true,
            can_collide: true,
            anchored: false,
        }
    }
}

/// Static mesh asset attached to an entity.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mesh {
    pub asset: AssetId,
}

/// Material asset attached to a mesh entity.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct Material {
    pub asset: AssetId,
}

/// Versioned gameplay behavior attachment. Multiple entries are ordered and persisted.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptComponents(pub Vec<ScriptComponent>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptComponent {
    pub format_version: u32,
    pub script_id: ScriptId,
    pub enabled: bool,
    pub properties: BTreeMap<String, EngineValue>,
}

impl ScriptComponent {
    pub fn new(script_id: ScriptId) -> Self {
        Self {
            format_version: 1,
            script_id,
            enabled: true,
            properties: BTreeMap::new(),
        }
    }

    /// Validates the persisted component bounds and schema version.
    ///
    /// # Errors
    /// Returns an error for an incompatible version or invalid property bounds.
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 {
            return Err(format!(
                "unsupported script component version {}",
                self.format_version
            ));
        }
        if self.properties.len() > 256 {
            return Err("script component exceeds 256 public properties".into());
        }
        if self
            .properties
            .keys()
            .any(|name| name.is_empty() || name.len() > 128)
        {
            return Err("script property names must contain 1..=128 bytes".into());
        }
        Ok(())
    }
}

/// Backend-independent camera projection.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CameraProjection {
    Perspective {
        vertical_fov_radians: f32,
        near: f32,
        far: f32,
    },
    Orthographic {
        vertical_size: f32,
        near: f32,
        far: f32,
    },
}

impl Default for CameraProjection {
    fn default() -> Self {
        Self::Perspective {
            vertical_fov_radians: 60.0_f32.to_radians(),
            near: 0.1,
            far: 1_000.0,
        }
    }
}

/// Camera component extracted for rendering.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub projection: CameraProjection,
    pub active: bool,
    pub order: i32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: CameraProjection::default(),
            active: true,
            order: 0,
        }
    }
}

/// Supported baseline light shapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

/// Backend-independent light component.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub color: Vec3,
    pub intensity: f32,
    pub range: f32,
    pub spot_outer_angle_radians: f32,
    pub casts_shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            kind: LightKind::Directional,
            color: Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
            spot_outer_angle_radians: 45.0_f32.to_radians(),
            casts_shadows: true,
        }
    }
}
