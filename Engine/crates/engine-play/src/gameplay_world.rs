//! Scene property adapter used by the shared gameplay scheduler.
use engine_core::gameplay::{GameplayWorld, Target, Value};
use engine_world::{CameraProjection, EntityId, SceneWorld, WorldCommand};

pub(crate) struct GameplayScene<'a>(
    pub &'a mut SceneWorld,
    pub &'a mut engine_core::gameplay::audio::Mixer,
);
impl GameplayWorld for GameplayScene<'_> {
    fn animation_target(&self, entity: EntityId, track: &str) -> Result<Target, String> {
        if let Some((path, property)) = track.rsplit_once('/') {
            let root = self.0.entity_path(entity).map_err(|e| e.to_string())?;
            let mut found = self
                .0
                .find_path(&format!("{root}.{}", path.replace('/', ".")))
                .map_err(|e| e.to_string())?;
            if found.is_none() && path.starts_with("node_") {
                for id in self.0.entity_ids() {
                    if self
                        .0
                        .snapshot(id)
                        .map_err(|e| e.to_string())?
                        .name
                        .as_deref()
                        != Some(path)
                    {
                        continue;
                    }
                    let mut parent = self.0.parent(id).map_err(|e| e.to_string())?;
                    while let Some(p) = parent {
                        if p == entity {
                            found = Some(id);
                            break;
                        }
                        parent = self.0.parent(p).map_err(|e| e.to_string())?;
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
            let id = found.ok_or_else(|| format!("animation target {track} was not found"))?;
            Ok(Target {
                entity: id,
                property: property.into(),
            })
        } else {
            Ok(Target {
                entity,
                property: track.into(),
            })
        }
    }
    fn contains(&self, id: EntityId) -> bool {
        self.0.contains(id) || self.1.voices.contains_key(&id)
    }
    fn read(&self, target: &Target) -> Result<Value, String> {
        if let Some(voice) = self.1.voices.get(&target.entity) {
            return match target.property.to_ascii_lowercase().as_str() {
                "volume" => Ok(Value::Number(voice.volume)),
                "pitch" => Ok(Value::Number(voice.pitch)),
                _ => Err("unsupported audio property".into()),
            };
        }
        let transform = self
            .0
            .local_transform(target.entity)
            .map_err(|e| e.to_string())?;
        match target.property.to_ascii_lowercase().as_str() {
            "position" => Ok(Value::Vector(transform.translation.as_dvec3().to_array())),
            "scale" | "size" => Ok(Value::Vector(transform.scale.as_dvec3().to_array())),
            "rotation" => Ok(Value::Rotation(transform.rotation.as_dquat().to_array())),
            "opacity" => Ok(Value::Number(f64::from(
                self.0
                    .part_attributes(target.entity)
                    .map_err(|e| e.to_string())?
                    .color[3],
            ))),
            "color" => {
                let c = self
                    .0
                    .part_attributes(target.entity)
                    .map_err(|e| e.to_string())?
                    .color;
                Ok(Value::Vector([
                    f64::from(c[0]),
                    f64::from(c[1]),
                    f64::from(c[2]),
                ]))
            }
            "fov" => {
                let camera = self
                    .0
                    .camera(target.entity)
                    .map_err(|e| e.to_string())?
                    .ok_or("target is not a camera")?;
                match camera.projection {
                    CameraProjection::Perspective {
                        vertical_fov_radians,
                        ..
                    } => Ok(Value::Number(f64::from(vertical_fov_radians))),
                    CameraProjection::Orthographic { .. } => {
                        Err("FOV requires a perspective camera".into())
                    }
                }
            }
            property => Err(format!("gameplay property `{property}` is unsupported")),
        }
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Scene storage is f32 and conversions are checked for finite range first"
    )]
    fn write(&mut self, target: &Target, value: Value) -> Result<(), String> {
        value.validate()?;
        if let Some(voice) = self.1.voices.get_mut(&target.entity) {
            return match (target.property.to_ascii_lowercase().as_str(), value) {
                ("volume", Value::Number(v)) if v >= 0.0 && v <= f64::from(f32::MAX) => {
                    voice.volume = v;
                    Ok(())
                }
                ("pitch", Value::Number(v)) if v > 0.0 => {
                    voice.pitch = v;
                    Ok(())
                }
                _ => Err("invalid audio property/value".into()),
            };
        }

        let name = target.property.to_ascii_lowercase();
        // Scene/render storage uses f32; reject conversion overflow.
        let finite = match value {
            Value::Number(v) => (v as f32).is_finite(),
            Value::Vector(v) => v.iter().all(|v| (*v as f32).is_finite()),
            Value::Rotation(v) => v.iter().all(|v| (*v as f32).is_finite()),
        };
        if !finite {
            return Err("gameplay value exceeds scene storage range".into());
        }
        let command = match (name.as_str(), value) {
            ("position" | "scale" | "size", Value::Vector(v)) => {
                let mut t = self
                    .0
                    .local_transform(target.entity)
                    .map_err(|e| e.to_string())?;
                let vector = glam::DVec3::from_array(v).as_vec3();
                if name == "position" {
                    t.translation = vector;
                } else {
                    t.scale = vector;
                }
                WorldCommand::SetLocalTransform {
                    entity: target.entity,
                    value: t,
                }
            }
            ("rotation", Value::Rotation(v)) => {
                let mut t = self
                    .0
                    .local_transform(target.entity)
                    .map_err(|e| e.to_string())?;
                t.rotation = glam::DQuat::from_array(v).normalize().as_quat();
                WorldCommand::SetLocalTransform {
                    entity: target.entity,
                    value: t,
                }
            }
            ("opacity", Value::Number(v)) => {
                let mut a = self
                    .0
                    .part_attributes(target.entity)
                    .map_err(|e| e.to_string())?;
                a.color[3] = v.clamp(0.0, 1.0) as f32;
                WorldCommand::SetPartAttributes {
                    entity: target.entity,
                    value: a,
                }
            }
            ("color", Value::Vector(v)) => {
                let mut a = self
                    .0
                    .part_attributes(target.entity)
                    .map_err(|e| e.to_string())?;
                a.color[..3].copy_from_slice(&v.map(|v| v.clamp(0.0, 1.0) as f32));
                WorldCommand::SetPartAttributes {
                    entity: target.entity,
                    value: a,
                }
            }
            ("fov", Value::Number(v)) => {
                if v <= 0.0 || v >= std::f64::consts::PI {
                    return Err("FOV must be in (0, pi) radians".into());
                }
                let mut camera = self
                    .0
                    .camera(target.entity)
                    .map_err(|e| e.to_string())?
                    .ok_or("target is not a camera")?;
                if let CameraProjection::Perspective {
                    ref mut vertical_fov_radians,
                    ..
                } = camera.projection
                {
                    *vertical_fov_radians = v as f32;
                } else {
                    return Err("FOV requires a perspective camera".into());
                }
                WorldCommand::SetCamera {
                    entity: target.entity,
                    value: Some(camera),
                }
            }
            _ => {
                return Err(format!(
                    "unsupported gameplay property `{name}` or value type"
                ));
            }
        };
        self.0.apply_commands(&[command]).map_err(|e| e.to_string())
    }
}
