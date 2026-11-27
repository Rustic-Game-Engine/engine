use super::EffectComponent;
use serde_json::Value;

/// Applies changed asset defaults while preserving authored instance overrides.
/// Struct fields merge independently; arrays (including curve keys) are atomic
/// overrides. The instance retains its stable source identity.
///
/// # Errors
/// Returns a serialization or validation diagnostic if any component is invalid.
pub fn merge_instance_defaults(
    instance: &EffectComponent,
    old_default: &EffectComponent,
    new_default: &EffectComponent,
) -> Result<EffectComponent, String> {
    fn merge(instance: Value, old: &Value, new: &Value) -> Value {
        if &instance == old {
            return new.clone();
        }
        match (instance, old, new) {
            (Value::Object(instance), Value::Object(old), Value::Object(new)) => {
                let mut result = new.clone();
                for (key, value) in instance {
                    match (old.get(&key), new.get(&key)) {
                        (Some(old), Some(new)) => {
                            result.insert(key, merge(value, old, new));
                        }
                        (Some(old), None) if &value == old => {}
                        _ => {
                            result.insert(key, value);
                        }
                    }
                }
                Value::Object(result)
            }
            (instance, _, _) => instance,
        }
    }
    let mut merged = instance.clone();
    merged.common = serde_json::from_value(merge(
        serde_json::to_value(&instance.common).map_err(|error| error.to_string())?,
        &serde_json::to_value(&old_default.common).map_err(|error| error.to_string())?,
        &serde_json::to_value(&new_default.common).map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    merged.kind = if instance.kind == old_default.kind {
        new_default.kind.clone()
    } else if std::mem::discriminant(&instance.kind) != std::mem::discriminant(&old_default.kind)
        || std::mem::discriminant(&old_default.kind) != std::mem::discriminant(&new_default.kind)
    {
        // An enum variant is an atomic override: combining variant object keys
        // would make the externally tagged serialized enum invalid.
        instance.kind.clone()
    } else {
        serde_json::from_value(merge(
            serde_json::to_value(&instance.kind).map_err(|error| error.to_string())?,
            &serde_json::to_value(&old_default.kind).map_err(|error| error.to_string())?,
            &serde_json::to_value(&new_default.kind).map_err(|error| error.to_string())?,
        ))
        .map_err(|error| error.to_string())?
    };
    merged.source.clone_from(&instance.source);
    merged.validate()?;
    Ok(merged)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // These tests require exact transfer, without arithmetic.
mod tests {
    use super::*;
    use crate::effects::{EffectKind, ParticleEmitter};

    #[test]
    fn changed_defaults_update_unmodified_fields_and_preserve_overrides() {
        let old = EffectComponent::default();
        let mut instance = old.clone();
        instance.common.duration = 7.0;
        let EffectKind::Particle(ref mut particle) = instance.kind else {
            unreachable!()
        };
        particle.rate = 17.0;
        let mut new = old.clone();
        new.common.duration = 12.0;
        new.common.render_distance = 100.0;
        new.kind = EffectKind::Particle(ParticleEmitter {
            rate: 80.0,
            drag: 2.0,
            ..Default::default()
        });
        let merged = merge_instance_defaults(&instance, &old, &new).unwrap();
        assert_eq!(merged.common.duration, 7.0);
        assert_eq!(merged.common.render_distance, 100.0);
        let EffectKind::Particle(particle) = merged.kind else {
            unreachable!()
        };
        assert_eq!(particle.rate, 17.0);
        assert_eq!(particle.drag, 2.0);
        assert_eq!(merge_instance_defaults(&old, &old, &new).unwrap(), new);
    }
    #[test]
    fn changed_effect_variants_merge_atomically_and_preserve_common_overrides() {
        use crate::effects::BeamRenderer;
        let old = EffectComponent::default();
        let mut new = old.clone();
        new.kind = EffectKind::Beam(BeamRenderer::default());
        new.common.render_distance = 100.0;
        assert_eq!(
            merge_instance_defaults(&old, &old, &new).unwrap().kind,
            new.kind
        );
        let mut overridden = old.clone();
        overridden.common.duration = 9.0;
        if let EffectKind::Particle(p) = &mut overridden.kind {
            p.rate = 17.0;
        }
        let merged = merge_instance_defaults(&overridden, &old, &new).unwrap();
        assert_eq!(merged.kind, overridden.kind);
        assert_eq!(merged.common.duration, 9.0);
        assert_eq!(merged.common.render_distance, 100.0);
        let mut different_override = old.clone();
        different_override.kind = EffectKind::Beam(BeamRenderer {
            width: 0.9,
            ..BeamRenderer::default()
        });
        assert_eq!(
            merge_instance_defaults(&different_override, &old, &new)
                .unwrap()
                .kind,
            different_override.kind
        );
        assert_eq!(
            merge_instance_defaults(&different_override, &old, &old)
                .unwrap()
                .kind,
            different_override.kind
        );
    }
}
