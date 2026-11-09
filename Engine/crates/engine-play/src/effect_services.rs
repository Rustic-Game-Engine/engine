//! Effect requests use the same live-world boundary as other gameplay services.
use engine_world::effects::{EffectComponent, EffectKind, PlaybackCommand};
use engine_world::{EntityId, SceneWorld, WorldCommand};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Query {
    #[serde(rename = "effect_types")]
    Types,
    #[serde(rename = "effect_wind")]
    Wind {
        position: [f32; 3],
        time: f32,
        layers: u32,
    },
    #[serde(rename = "effect_spawn")]
    Spawn {
        asset: engine_world::AssetId,
        #[serde(default)]
        parent: Option<EntityId>,
        position: [f32; 3],
        lifetime: f64,
    },
    #[serde(rename = "effect_instantiate")]
    Instantiate {
        asset: engine_world::AssetId,
        #[serde(default)]
        parent: Option<EntityId>,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default)]
        lifetime: Option<f64>,
    },
    #[serde(rename = "effect_create")]
    Create { entity: EntityId, kind: String },
    #[serde(rename = "effect_get")]
    Get { entity: EntityId },
    #[serde(rename = "effect_configure")]
    Configure { entity: EntityId, settings: Value },
    #[serde(rename = "effect_group_get")]
    GroupGet { entity: EntityId },
    #[serde(rename = "effect_group_configure")]
    GroupConfigure { entity: EntityId, settings: Value },
    #[serde(rename = "effect_control")]
    Control { entity: EntityId, action: String },
    #[serde(rename = "effect_refresh_probe")]
    RefreshProbe { entity: EntityId },
    #[serde(rename = "effect_state")]
    State { entity: EntityId },
}

pub(crate) fn query(world: &mut SceneWorld, value: Value) -> Result<Value, String> {
    let query: Query =
        serde_json::from_value(value).map_err(|e| format!("invalid effect request: {e}"))?;
    match query {
        Query::Spawn {
            asset,
            parent,
            position,
            lifetime,
        } => self::query(
            world,
            json!({"op":"effect_instantiate","asset":asset,"parent":parent,"position":position,"lifetime":lifetime}),
        ),
        Query::Instantiate {
            asset,
            parent,
            position,
            lifetime,
        } => {
            if lifetime.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                return Err("effect spawn lifetime must be finite and positive".into());
            }
            if !position.iter().all(|v| v.is_finite()) {
                return Err("effect position must be finite".into());
            }
            let transform = engine_world::LocalTransform {
                translation: glam::Vec3::from_array(position),
                ..engine_world::LocalTransform::IDENTITY
            };
            let id = world
                .instantiate_effect_asset(asset, parent, transform)
                .map_err(|e| e.to_string())?;
            if let Some(lifetime) = lifetime {
                world
                    .destroy_after(id, lifetime)
                    .map_err(|e| e.to_string())?;
            }
            Ok(json!(id))
        }
        Query::Wind {
            position,
            time,
            layers,
        } => {
            if !position.iter().all(|v| v.is_finite()) || !time.is_finite() {
                return Err("wind sample position and time must be finite".into());
            }
            world.propagate_transforms();
            Ok(json!(
                world
                    .sample_effect_wind(glam::Vec3::from_array(position), time, layers)
                    .to_array()
            ))
        }
        Query::Types => Ok(json!(
            EffectKind::all_defaults()
                .iter()
                .map(kind_key)
                .collect::<Vec<_>>()
        )),
        Query::Get { entity } => {
            serde_json::to_value(world.effect(entity).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
        Query::GroupGet { entity } => serde_json::to_value(
            world
                .effect_group(entity)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string()),
        Query::GroupConfigure { entity, settings } => configure_group(world, entity, settings),
        Query::Create { entity, kind } => {
            if world.effect(entity).map_err(|e| e.to_string())?.is_some() {
                return Err(
                    "entity already has an effect; configure it or dispose it first".into(),
                );
            }
            let kind = EffectKind::all_defaults()
                .into_iter()
                .find(|k| kind_key(k) == kind)
                .ok_or_else(|| {
                    format!("unknown effect kind `{kind}`; query effect_types for supported names")
                })?;
            save(world, entity, EffectComponent::new(kind))
        }
        Query::Configure { entity, settings } => {
            let effect = world
                .effect(entity)
                .map_err(|e| e.to_string())?
                .ok_or("entity has no effect")?;
            let mut value = serde_json::to_value(effect).map_err(|e| e.to_string())?;
            patch(&mut value, &settings, "effect")?;
            let effect = serde_json::from_value(value)
                .map_err(|e| format!("invalid effect settings: {e}"))?;
            save(world, entity, effect)
        }
        Query::Control { entity, action } => control(world, entity, &action),
        Query::RefreshProbe { entity } => world
            .request_reflection_probe_refresh(entity)
            .map(|revision| json!(revision))
            .map_err(|e| e.to_string()),
        Query::State { entity } => Ok(world
            .effect_state(entity)
            .map_err(|e| e.to_string())?
            .map_or(Value::Null, |state| json!(format!("{state:?}")))),
    }
}

fn control(world: &mut SceneWorld, entity: EntityId, action: &str) -> Result<Value, String> {
    let command = match action {
        "play" => PlaybackCommand::Play,
        "pause" => PlaybackCommand::Pause,
        "resume" => PlaybackCommand::Resume,
        "stop" => PlaybackCommand::Stop,
        "restart" => PlaybackCommand::Restart,
        "reset" => PlaybackCommand::Reset,
        "dispose" => {
            world
                .control_effect(entity, PlaybackCommand::Dispose)
                .map_err(|e| e.to_string())?;
            let mut entities = vec![entity];
            entities.extend(world.descendants(entity).map_err(|e| e.to_string())?);
            let commands = entities
                .into_iter()
                .filter(|id| world.effect(*id).ok().flatten().is_some())
                .map(|entity| WorldCommand::SetEffect {
                    entity,
                    value: None,
                })
                .collect::<Vec<_>>();
            world.apply_commands(&commands).map_err(|e| e.to_string())?;
            return Ok(Value::Null);
        }
        _ => return Err(format!("unknown effect action `{action}`")),
    };
    world
        .control_effect(entity, command)
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

fn kind_key(kind: &EffectKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_object().and_then(|o| o.keys().next().cloned()))
        .unwrap_or_default()
}

pub(crate) fn references(effect: &EffectComponent) -> Vec<engine_world::AssetId> {
    let mut references = effect
        .source
        .as_ref()
        .map(|s| s.asset)
        .into_iter()
        .collect::<Vec<_>>();
    match &effect.kind {
        EffectKind::Particle(p) | EffectKind::MeshParticle(p) => references.extend(p.sub_emitter),
        EffectKind::Weather(w) => references.extend(w.emitter.sub_emitter),
        EffectKind::CollisionVolume(c) => references.extend(c.impact_effect),
        _ => {}
    }
    references
}

fn save(
    world: &mut SceneWorld,
    entity: EntityId,
    effect: EffectComponent,
) -> Result<Value, String> {
    effect.validate()?;
    for reference in references(&effect) {
        if !world.has_effect_asset(reference) {
            return Err(format!(
                "effect asset `{reference}` is not loaded; add a valid .effect source and stable .rmeta sidecar before starting the scene"
            ));
        }
    }
    let value = serde_json::to_value(&effect).map_err(|e| e.to_string())?;
    world
        .apply_commands(&[WorldCommand::SetEffect {
            entity,
            value: Some(Box::new(effect)),
        }])
        .map_err(|e| e.to_string())?;
    Ok(value)
}

/// Merge only known fields, rejecting typos before mutating the world.
fn configure_group(
    world: &mut SceneWorld,
    entity: EntityId,
    settings: Value,
) -> Result<Value, String> {
    if settings.as_object().is_some_and(|fields| {
        ["source", "source_baseline", "child_entities"]
            .iter()
            .any(|name| fields.contains_key(*name))
    }) {
        return Err("composite source and child identities are read-only; instantiate or reassign the asset through the scene authoring API".into());
    }
    let group = world
        .effect_group(entity)
        .map_err(|error| error.to_string())?
        .ok_or("entity has no composite effect timeline; instantiate an effect asset first")?;
    let mut value = serde_json::to_value(group).map_err(|error| error.to_string())?;
    patch(&mut value, &settings, "group")?;
    let group: engine_world::effects::EffectGroup = serde_json::from_value(value)
        .map_err(|error| format!("invalid composite settings: {error}"))?;
    group.validate()?;
    if let Some(source) = group.source
        && !world.has_effect_asset(source)
    {
        return Err(format!("effect asset `{source}` is not loaded"));
    }
    let output = serde_json::to_value(&group).map_err(|error| error.to_string())?;
    world
        .apply_commands(&[WorldCommand::SetEffectGroup {
            entity,
            value: Some(Box::new(group)),
        }])
        .map_err(|error| error.to_string())?;
    Ok(output)
}

fn patch(target: &mut Value, settings: &Value, path: &str) -> Result<(), String> {
    let fields = settings
        .as_object()
        .ok_or("effect settings must be an object")?;
    let slots = target
        .as_object_mut()
        .ok_or_else(|| format!("`{path}` is not an object"))?;
    for (name, value) in fields {
        if path.ends_with(".parameters") {
            if value.is_null() {
                slots.remove(name);
            } else {
                slots.insert(name.clone(), value.clone());
            }
            continue;
        }
        let slot = slots
            .get_mut(name)
            .ok_or_else(|| format!("unknown effect property `{path}.{name}`"))?;
        if value.is_object() && slot.is_object() {
            patch(slot, value, &format!("{path}.{name}"))?;
        } else {
            *slot = value.clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_queries_configure_live_simulation_and_dispose_without_destroying_entity() {
        let mut world = SceneWorld::new();
        let entity = engine_world::EntitySnapshot::default();
        let id = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        assert_eq!(
            query(&mut world, json!({"op":"effect_types"}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            26
        );
        query(
            &mut world,
            json!({"op":"effect_create","entity":id,"kind":"Particle"}),
        )
        .unwrap();
        query(&mut world,json!({"op":"effect_configure","entity":id,"settings":{"kind":{"Particle":{"rate":60}}}})).unwrap();
        let original = world.effect(id).unwrap();
        for settings in [
            json!({"common":{"duration":0}}),
            json!({"kind":{"Particle":{"raet":3}}}),
        ] {
            assert!(
                query(
                    &mut world,
                    json!({"op":"effect_configure","entity":id,"settings":settings})
                )
                .is_err()
            );
            assert_eq!(world.effect(id).unwrap(), original);
        }
        world.propagate_transforms();
        query(
            &mut world,
            json!({"op":"effect_control","entity":id,"action":"play"}),
        )
        .unwrap();
        world.update_effects(0.1);
        assert!(!world.effect_frame().particles.is_empty());
        query(
            &mut world,
            json!({"op":"effect_control","entity":id,"action":"pause"}),
        )
        .unwrap();
        assert_eq!(
            query(&mut world, json!({"op":"effect_state","entity":id})).unwrap(),
            "Paused"
        );
        query(
            &mut world,
            json!({"op":"effect_control","entity":id,"action":"dispose"}),
        )
        .unwrap();
        world.update_effects(0.1);
        assert!(world.effect_frame().particles.is_empty());
        assert!(world.effect(id).unwrap().is_none());
        assert!(world.contains(id));
    }

    #[test]
    fn temporary_asset_instances_keep_independent_configuration_and_cleanup() {
        let mut world = SceneWorld::new();
        let asset = engine_world::AssetId::new();
        world
            .register_effect_asset(
                asset,
                engine_world::effects::EffectAsset::new("Sparks", vec![EffectComponent::default()]),
            )
            .unwrap();
        assert!(
            query(
                &mut world,
                json!({"op":"effect_spawn","asset":asset,"position":[0,0,0]})
            )
            .is_err()
        );
        assert_eq!(world.entity_ids().count(), 0);
        let first: EntityId = serde_json::from_value(
            query(
                &mut world,
                json!({"op":"effect_spawn","asset":asset,"position":[1,2,3],"lifetime":0.5}),
            )
            .unwrap(),
        )
        .unwrap();
        let second: EntityId = serde_json::from_value(
            query(&mut world, json!({"op":"effect_instantiate","asset":asset})).unwrap(),
        )
        .unwrap();
        assert_ne!(first, second);
        query(
            &mut world,
            json!({"op":"effect_control","entity":second,"action":"pause"}),
        )
        .unwrap();
        assert_eq!(
            query(&mut world, json!({"op":"effect_state","entity":second})).unwrap(),
            "Paused"
        );
        world.tick_entity_lifetimes(0.5).unwrap();
        assert!(!world.contains(first));
        assert!(world.contains(second));
        query(
            &mut world,
            json!({"op":"effect_control","entity":second,"action":"dispose"}),
        )
        .unwrap();
        assert!(world.contains(second));
        assert!(
            world
                .descendants(second)
                .unwrap()
                .iter()
                .all(|id| world.effect(*id).unwrap().is_none())
        );
    }

    #[test]
    fn nested_patch_rejects_unknown_properties_without_touching_original() {
        let original = json!({"common":{"duration":2.0},"kind":{"Particle":{"rate":10.0}}});
        let mut value = original.clone();
        patch(
            &mut value,
            &json!({"kind":{"Particle":{"rate":20.0}}}),
            "effect",
        )
        .unwrap();
        assert_eq!(value["kind"]["Particle"]["rate"], 20.0);
        assert_eq!(original["kind"]["Particle"]["rate"], 10.0);
        assert!(
            patch(&mut value, &json!({"common":{"duraton":1}}), "effect")
                .unwrap_err()
                .contains("duraton")
        );
    }

    #[test]
    fn group_queries_preserve_identity_and_validate_parameter_updates_before_commit() {
        let mut world = SceneWorld::new();
        let snapshot = engine_world::EntitySnapshot {
            effect_group: Some(engine_world::effects::EffectGroup::default()),
            ..Default::default()
        };
        let entity = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .unwrap();
        let updated = query(&mut world, json!({"op":"effect_group_configure","entity":entity,"settings":{"duration":2.0,"parameters":{"strength":{"Scalar":3.0}}}})).unwrap();
        assert_eq!(updated["duration"], 2.0);
        assert_eq!(updated["parameters"]["strength"]["Scalar"], 3.0);
        assert_eq!(
            query(&mut world, json!({"op":"effect_group_get","entity":entity})).unwrap(),
            updated
        );
        for settings in [
            json!({"duration":-2.0}),
            json!({"parameters":{"strength":{"Scalar":"bad"}}}),
            json!({"child_entities":[]}),
            json!({"source":null}),
            json!({"source_baseline":null}),
        ] {
            assert!(
                query(
                    &mut world,
                    json!({"op":"effect_group_configure","entity":entity,"settings":settings})
                )
                .is_err()
            );
            assert_eq!(
                query(&mut world, json!({"op":"effect_group_get","entity":entity})).unwrap(),
                updated
            );
        }
        let cleared = query(&mut world, json!({"op":"effect_group_configure","entity":entity,"settings":{"parameters":{"strength":null}}})).unwrap();
        assert_eq!(cleared["parameters"], json!({}));
    }

    #[test]
    fn script_parameter_maps_accept_new_keys_and_drive_the_existing_simulation() {
        let mut world = SceneWorld::new();
        let snapshot = engine_world::EntitySnapshot {
            effect: Some(EffectComponent::default()),
            ..Default::default()
        };
        let entity = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .unwrap();
        query(&mut world, json!({"op":"effect_configure","entity":entity,"settings":{"common":{"parameters":{"emission_rate":{"Scalar":100.0}}}}})).unwrap();
        world.propagate_transforms();
        world.update_effects(0.1);
        assert_eq!(world.effect_frame().particles.len(), 10);
        query(&mut world, json!({"op":"effect_configure","entity":entity,"settings":{"common":{"parameters":{"emission_rate":null}}}})).unwrap();
        assert!(
            world
                .effect(entity)
                .unwrap()
                .unwrap()
                .common
                .parameters
                .is_empty()
        );
    }
}
