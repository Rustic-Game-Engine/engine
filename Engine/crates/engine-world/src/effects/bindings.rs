//! Typed composite parameter bindings affect extracted clones, never authored children.
use super::{
    EffectComponent, EffectEvent, EffectEventKind, EffectGroup, EffectParameter, PlaybackCommand,
};
use crate::EntityId;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompositeTriggerEvent {
    Birth,
    Collision,
    Death,
    #[default]
    Completed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CompositeTriggerAction {
    #[default]
    Play,
    Pause,
    Resume,
    Stop,
    Restart,
    Reset,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupTriggerBinding {
    pub source_child: u32,
    pub event: CompositeTriggerEvent,
    pub target_child: u32,
    pub action: CompositeTriggerAction,
}

/// Validates composite child event links before scene mutation.
/// # Errors
/// Rejects over-budget event tracks and references to absent children.
pub fn validate_group_triggers(
    bindings: &[GroupTriggerBinding],
    child_count: usize,
) -> Result<(), String> {
    if bindings.len() > 256 {
        return Err("composite trigger bindings are limited to256".into());
    }
    for binding in bindings {
        if usize::try_from(binding.source_child).map_or(true, |index| index >= child_count)
            || usize::try_from(binding.target_child).map_or(true, |index| index >= child_count)
        {
            return Err("composite trigger source/target child does not exist".into());
        }
    }
    Ok(())
}

/// Converts current child events to a bounded, deduplicated set of next-tick controls.
/// Controls do not recursively simulate an effect in this call.
pub fn group_trigger_commands(
    group: &EffectGroup,
    events: &[EffectEvent],
) -> Vec<(EntityId, PlaybackCommand)> {
    let mut unique = BTreeSet::new();
    for binding in &group.trigger_bindings {
        let Some(source) = usize::try_from(binding.source_child)
            .ok()
            .and_then(|index| group.child_entities.get(index))
        else {
            continue;
        };
        let Some(target) = usize::try_from(binding.target_child)
            .ok()
            .and_then(|index| group.child_entities.get(index))
        else {
            continue;
        };
        let kind = match binding.event {
            CompositeTriggerEvent::Birth => EffectEventKind::Birth,
            CompositeTriggerEvent::Collision => EffectEventKind::Collision,
            CompositeTriggerEvent::Death => EffectEventKind::Death,
            CompositeTriggerEvent::Completed => EffectEventKind::Completed,
        };
        if events
            .iter()
            .any(|event| event.entity == *source && event.kind == kind)
        {
            unique.insert((*target, binding.action));
            if unique.len() >= 64 {
                break;
            }
        }
    }
    unique
        .into_iter()
        .map(|(entity, action)| {
            (
                entity,
                match action {
                    CompositeTriggerAction::Play => PlaybackCommand::Play,
                    CompositeTriggerAction::Pause => PlaybackCommand::Pause,
                    CompositeTriggerAction::Resume => PlaybackCommand::Resume,
                    CompositeTriggerAction::Stop => PlaybackCommand::Stop,
                    CompositeTriggerAction::Restart => PlaybackCommand::Restart,
                    CompositeTriggerAction::Reset => PlaybackCommand::Reset,
                },
            )
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupParameterBinding {
    pub parameter: String,
    pub child: u32,
    pub property_path: String,
    pub scale: f32,
    pub offset: f32,
}
impl Default for GroupParameterBinding {
    fn default() -> Self {
        Self {
            parameter: "power".into(),
            child: 0,
            property_path: "kind.Particle.rate".into(),
            scale: 1.0,
            offset: 0.0,
        }
    }
}

fn validate_binding(
    parameters: &BTreeMap<String, EffectParameter>,
    binding: &GroupParameterBinding,
) -> Result<(), String> {
    if binding.parameter.is_empty()
        || binding.parameter.len() > 64
        || !parameters.contains_key(&binding.parameter)
    {
        return Err(format!(
            "composite binding parameter `{}` is missing or invalid",
            binding.parameter
        ));
    }
    if binding.property_path.len() > 256
        || binding.property_path.split('.').count() > 32
        || !(binding.property_path.starts_with("kind.")
            || binding.property_path.starts_with("common."))
        || binding.property_path.split('.').any(str::is_empty)
        || !binding.scale.is_finite()
        || !binding.offset.is_finite()
    {
        return Err("composite binding requires a finite scale/offset and an authored kind/common property path".into());
    }
    Ok(())
}

fn slot<'a>(mut value: &'a mut Value, path: &str) -> Result<&'a mut Value, String> {
    for segment in path.split('.') {
        value = match value {
            Value::Object(fields) => fields.get_mut(segment),
            Value::Array(values) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| values.get_mut(index)),
            _ => None,
        }
        .ok_or_else(|| format!("unknown composite child property `{path}`"))?;
    }
    Ok(value)
}

fn numeric(value: f32, target: &Value) -> Result<Value, String> {
    if !value.is_finite() {
        return Err("composite binding produced a non-finite value".into());
    }
    let Value::Number(number) = target else {
        return Err("numeric composite parameter requires a numeric child property".into());
    };
    let number = if number.is_f64() {
        Number::from_f64(f64::from(value)).ok_or("composite value is not finite")?
    } else {
        if value.fract().abs() > 0.0 {
            return Err("integer child property requires an exact integer composite value".into());
        }
        let number = format!("{value:.0}")
            .parse::<Number>()
            .map_err(|error| error.to_string())?;
        if number.is_f64() {
            return Err("composite integer value is outside the supported integer range".into());
        }
        number
    };
    Ok(Value::Number(number))
}

fn bound_value(
    parameter: &EffectParameter,
    target: &Value,
    scale: f32,
    offset: f32,
) -> Result<Value, String> {
    match parameter {
        EffectParameter::Scalar(value) => numeric(value.mul_add(scale, offset), target),
        EffectParameter::Boolean(value) => {
            if !target.is_boolean() || scale.to_bits() != 1.0_f32.to_bits() || offset.abs() > 0.0 {
                return Err(
                    "boolean composite bindings require a boolean property, scale1 and offset0"
                        .into(),
                );
            }
            Ok(Value::Bool(*value))
        }
        EffectParameter::Vector(values) => bind_vector(values, target, scale, offset),
        EffectParameter::Color(values) => bind_vector(values, target, scale, offset),
    }
}
fn bind_vector(values: &[f32], target: &Value, scale: f32, offset: f32) -> Result<Value, String> {
    let elements = target
        .as_array()
        .filter(|elements| elements.len() == values.len())
        .ok_or("composite vector/color dimensions must match the child property")?;
    values
        .iter()
        .zip(elements)
        .map(|(value, target)| numeric(value.mul_add(scale, offset), target))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}
/// Checks complete bindings against authored child configurations before instantiation.
///
/// # Errors
/// Rejects missing parameters/children, unknown paths, incompatible types and invalid values.
pub fn validate_group_bindings(
    parameters: &BTreeMap<String, EffectParameter>,
    bindings: &[GroupParameterBinding],
    effects: &[EffectComponent],
) -> Result<(), String> {
    if parameters.len() > 256 || bindings.len() > 256 {
        return Err("composite parameters and bindings are each limited to256".into());
    }
    let mut parameter_validation = EffectComponent::default();
    parameter_validation
        .common
        .parameters
        .clone_from(parameters);
    parameter_validation.validate()?;
    let mut candidates = BTreeMap::new();
    for binding in bindings {
        validate_binding(parameters, binding)?;
        let child = usize::try_from(binding.child).map_err(|error| error.to_string())?;
        let effect = effects
            .get(child)
            .ok_or("composite binding child does not exist")?;
        if let std::collections::btree_map::Entry::Vacant(entry) = candidates.entry(child) {
            entry.insert(serde_json::to_value(effect).map_err(|error| error.to_string())?);
        }
        let value = candidates
            .get_mut(&child)
            .ok_or("composite child cache unavailable")?;
        let target = slot(value, &binding.property_path)?;
        let parameter = parameters
            .get(&binding.parameter)
            .ok_or("composite parameter does not exist")?;
        *target = bound_value(parameter, target, binding.scale, binding.offset)?;
    }
    for value in candidates.into_values() {
        let candidate: EffectComponent =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        candidate.validate()?;
    }
    Ok(())
}

/// Resolves matching child bindings into a simulation/render clone.
///
/// # Errors
/// Rejects invalid bindings atomically; the supplied clone is unchanged on failure.
pub fn apply_group_bindings(
    group: &EffectGroup,
    entity: EntityId,
    effect: &mut EffectComponent,
) -> Result<(), String> {
    let mut value = None;
    for binding in &group.bindings {
        validate_binding(&group.parameters, binding)?;
        let child = usize::try_from(binding.child).map_err(|error| error.to_string())?;
        let target = group
            .child_entities
            .get(child)
            .ok_or("composite binding child identity is missing")?;
        if *target == entity {
            if value.is_none() {
                value = Some(serde_json::to_value(&*effect).map_err(|error| error.to_string())?);
            }
            let value = value.as_mut().ok_or("composite child value unavailable")?;
            let target = slot(value, &binding.property_path)?;
            let parameter = group
                .parameters
                .get(&binding.parameter)
                .ok_or("composite binding parameter is missing")?;
            *target = bound_value(parameter, target, binding.scale, binding.offset)?;
        }
    }
    if let Some(value) = value {
        let candidate: EffectComponent =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        candidate.validate()?;
        *effect = candidate;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{EffectKind, ParticleEmitter};

    #[test]
    fn shared_group_parameter_drives_two_children_without_mutating_authored_data() {
        let first = EntityId::new();
        let second = EntityId::new();
        let authored = EffectComponent::default();
        let mut group = EffectGroup {
            child_entities: vec![first, second],
            ..Default::default()
        };
        group
            .parameters
            .insert("power".into(), EffectParameter::Scalar(3.0));
        group.bindings = vec![
            GroupParameterBinding {
                parameter: "power".into(),
                child: 0,
                property_path: "kind.Particle.rate".into(),
                scale: 20.0,
                offset: 0.0,
            },
            GroupParameterBinding {
                parameter: "power".into(),
                child: 1,
                property_path: "kind.Particle.max_particles".into(),
                scale: 100.0,
                offset: 0.0,
            },
        ];
        validate_group_bindings(
            &group.parameters,
            &group.bindings,
            &[authored.clone(), authored.clone()],
        )
        .unwrap();
        let mut first_effect = authored.clone();
        let mut second_effect = authored.clone();
        apply_group_bindings(&group, first, &mut first_effect).unwrap();
        apply_group_bindings(&group, second, &mut second_effect).unwrap();
        let EffectKind::Particle(first) = first_effect.kind else {
            panic!("particle")
        };
        let EffectKind::Particle(second) = second_effect.kind else {
            panic!("particle")
        };
        assert!((first.rate - 60.0).abs() < f32::EPSILON);
        assert_eq!(second.max_particles, 300);
        assert_eq!(
            authored,
            EffectComponent::new(EffectKind::Particle(ParticleEmitter::default()))
        );
    }

    #[test]
    fn invalid_binding_types_paths_and_ranges_are_rejected_atomically() {
        let entity = EntityId::new();
        let mut group = EffectGroup {
            child_entities: vec![entity],
            ..Default::default()
        };
        group
            .parameters
            .insert("power".into(), EffectParameter::Scalar(3.5));
        let authored = EffectComponent::default();
        for property_path in [
            "kind.Particle.unknown",
            "source.asset",
            "kind.Particle.max_particles",
            "common.enabled",
        ] {
            group.bindings = vec![GroupParameterBinding {
                property_path: property_path.into(),
                ..GroupParameterBinding::default()
            }];
            let mut effect = authored.clone();
            assert!(apply_group_bindings(&group, entity, &mut effect).is_err());
            assert_eq!(effect, authored);
        }
    }

    #[test]
    fn event_bindings_are_validated_and_deduplicate_particle_bursts() {
        let source = EntityId::new();
        let target = EntityId::new();
        let mut group = EffectGroup {
            child_entities: vec![source, target],
            ..Default::default()
        };
        group.trigger_bindings = vec![GroupTriggerBinding {
            source_child: 0,
            event: CompositeTriggerEvent::Birth,
            target_child: 1,
            action: CompositeTriggerAction::Restart,
        }];
        validate_group_triggers(&group.trigger_bindings, 2).unwrap();
        let event = EffectEvent {
            entity: source,
            kind: EffectEventKind::Birth,
            position: glam::Vec3::ZERO,
            velocity: glam::Vec3::ZERO,
            child_effect: None,
        };
        let commands = group_trigger_commands(&group, &vec![event; 100]);
        assert_eq!(commands, vec![(target, PlaybackCommand::Restart)]);
        group.trigger_bindings[0].target_child = 3;
        assert!(validate_group_triggers(&group.trigger_bindings, 2).is_err());
    }
}
