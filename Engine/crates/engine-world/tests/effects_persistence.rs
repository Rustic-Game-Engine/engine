use engine_world::effects::*;
use engine_world::*;

#[test]
fn every_effect_survives_scene_reload_and_world_capture() {
    let mut document = SceneDocument::new("Effects compatibility");
    for kind in EffectKind::all_defaults() {
        document.entities.push(EntitySnapshot {
            name: Some(kind.name().into()),
            effect: Some(EffectComponent::new(kind)),
            ..Default::default()
        });
    }
    document.entities.sort_by_key(|entity| entity.id);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("effects.scene");
    save_scene_atomic(&path, &document).unwrap();
    let loaded = load_scene(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!loaded.has_errors(), "{:?}", loaded.diagnostics);
    assert_eq!(loaded.document, document);
    let world = loaded.document.create_world().unwrap();
    for entity in document.entities {
        assert_eq!(world.snapshot(entity.id).unwrap().effect, entity.effect);
    }
}

#[test]
fn effect_edits_delete_and_restore_use_existing_undo() {
    let mut world = SceneWorld::new();
    let entity = EntitySnapshot::default();
    let id = entity.id;
    world.apply_commands(&[WorldCommand::Spawn(Box::new(entity))]).unwrap();
    let mut undo = UndoStack::new();
    let effect = EffectComponent::new(EffectKind::Beam(Default::default()));
    undo.execute(&mut world, SceneEdit::Effect { entity: id, before: None, after: Some(effect.clone()) }).unwrap();
    assert_eq!(world.effect(id).unwrap(), Some(effect.clone()));
    undo.undo(&mut world).unwrap();
    assert!(world.effect(id).unwrap().is_none());
    undo.redo(&mut world).unwrap();
    let delete = SceneEdit::capture_delete(&world, id).unwrap();
    undo.execute(&mut world, delete).unwrap();
    assert!(world.effect(id).is_err());
    undo.undo(&mut world).unwrap();
    assert_eq!(world.effect(id).unwrap(), Some(effect));
}

#[test]
fn additive_instances_remap_effect_attachments_and_targets() {
    let target = EntitySnapshot::default();
    let effect = EntitySnapshot {
        effect: Some(EffectComponent {
            source: None,
            common: EffectCommon { attachment_target: Some(target.id), ..Default::default() },
            kind: EffectKind::Beam(BeamRenderer { start_target: Some(target.id), end_target: Some(target.id), ..Default::default() }),
        }),
        ..Default::default()
    };
    let mut document = SceneDocument::new("Reusable effect");
    document.entities = vec![target.clone(), effect.clone()];
    let mut world = SceneWorld::new();
    let instance = world.instantiate_scene(&document, SceneInstanceId::new(), None).unwrap();
    let remapped = world.effect(instance.entity_map[&effect.id]).unwrap().unwrap();
    let target_id = instance.entity_map[&target.id];
    assert_eq!(remapped.common.attachment_target, Some(target_id));
    let EffectKind::Beam(beam) = remapped.kind else { panic!("wrong effect type") };
    assert_eq!(beam.start_target, Some(target_id));
    assert_eq!(beam.end_target, Some(target_id));
}

#[test]
fn invalid_effect_cannot_partially_commit_a_transaction() {
    let mut world = SceneWorld::new();
    let entity = EntitySnapshot::default();
    let id = entity.id;
    world.apply_commands(&[WorldCommand::Spawn(Box::new(entity))]).unwrap();
    let mut effect = EffectComponent::default();
    effect.common.duration = f32::NAN;
    assert!(world.apply_commands(&[
        WorldCommand::SetName { entity: id, value: Some("must not commit".into()) },
        WorldCommand::SetEffect { entity: id, value: Some(Box::new(effect)) },
    ]).is_err());
    assert!(world.snapshot(id).unwrap().name.is_none());
}

#[test]
fn play_before_first_tick_and_pause_use_live_world_runtime() {
    let mut entity = EntitySnapshot::default();
    let id = entity.id;
    let mut effect = EffectComponent::default();
    effect.common.auto_play = false;
    entity.effect = Some(effect);
    let mut world = SceneWorld::new();
    world.apply_commands(&[WorldCommand::Spawn(Box::new(entity))]).unwrap();
    world.propagate_transforms();
    world.control_effect(id, PlaybackCommand::Play).unwrap();
    world.update_effects(0.1);
    assert_eq!(world.effect_state(id).unwrap(), Some(PlaybackState::Playing));
    assert!(!world.effect_frame().particles.is_empty());
    world.control_effect(id, PlaybackCommand::Pause).unwrap();
    let count = world.effect_frame().particles.len();
    world.update_effects(0.5);
    assert_eq!(world.effect_state(id).unwrap(), Some(PlaybackState::Paused));
    assert_eq!(world.effect_frame().particles.len(), count);
}

#[test]
fn registered_subemitters_spawn_and_cleanup_with_bounded_generations() {
    let child_id = AssetId::new();
    let child = EffectComponent {
        source: None,
        common: EffectCommon { duration: 0.01, looping: false, ..Default::default() },
        kind: EffectKind::Particle(ParticleEmitter {
            rate: 0.0, lifetime: [0.01, 0.01],
            bursts: vec![Burst { count: 1, ..Default::default() }],
            sub_emitter: Some(child_id), max_generations: 2,
            ..Default::default()
        }),
    };
    let mut world = SceneWorld::new();
    world.register_effect_asset(child_id, EffectAsset::new("recursive", vec![child.clone()])).unwrap();
    world.apply_commands(&[WorldCommand::Spawn(Box::new(EntitySnapshot {
        effect: Some(child), ..Default::default()
    }))]).unwrap();
    world.propagate_transforms();
    for _ in 0..20 {
        world.update_effects(0.02);
        assert!(world.entity_count() < 32);
    }
    assert_eq!(world.entity_count(), 1);
}

#[test]
fn composite_asset_instances_keep_source_identity_and_control_as_a_group() {
    let asset = presets().pop().unwrap();
    let asset_id = AssetId::new();
    let mut world = SceneWorld::new();
    world.register_effect_asset(asset_id, asset.clone()).unwrap();
    let root = world.instantiate_effect_asset(asset_id, None, LocalTransform::IDENTITY).unwrap();
    assert_eq!(world.children(root).unwrap().len(), asset.effects.len());
    for child in world.children(root).unwrap() {
        let effect = world.effect(*child).unwrap().unwrap();
        assert_eq!(effect.source.unwrap().asset, asset_id);
    }
    world.control_effect(root, PlaybackCommand::Pause).unwrap();
    assert_eq!(world.effect_state(root).unwrap(), Some(PlaybackState::Paused));
    world.control_effect(root, PlaybackCommand::Restart).unwrap();
    world.update_effects(0.01);
    assert_eq!(world.effect_state(root).unwrap(), Some(PlaybackState::Playing));
}

#[test]
fn loading_changed_assets_refreshes_defaults_without_losing_saved_overrides() {
    let asset_id = AssetId::new();
    let mut asset = EffectAsset::new("shared", vec![EffectComponent::default()]);
    let mut world = SceneWorld::new();
    world.register_effect_asset(asset_id, asset.clone()).unwrap();
    let root = world.instantiate_effect_asset(asset_id, None, LocalTransform::IDENTITY).unwrap();
    let entity = world.children(root).unwrap()[0];
    let mut effect = world.effect(entity).unwrap().unwrap();
    effect.common.duration = 12.0;
    world.apply_commands(&[WorldCommand::SetEffect { entity, value: Some(Box::new(effect)) }]).unwrap();
    let document = SceneDocument::from_world(SceneId::new(), "saved overrides", &world).unwrap();
    let mut reopened = load_scene(&document.to_bytes().unwrap()).unwrap().document.create_world().unwrap();
    asset.effects[0].common.duration = 9.0;
    asset.effects[0].common.render_distance = 50.0;
    reopened.register_effect_asset(asset_id, asset).unwrap();
    let refreshed = reopened.effect(entity).unwrap().unwrap();
    assert_eq!(refreshed.common.duration, 12.0);
    assert_eq!(refreshed.common.render_distance, 50.0);
    let baseline = refreshed.source.unwrap().baseline.unwrap();
    assert_eq!(baseline.common.duration, 9.0);
    assert_eq!(baseline.common.render_distance, 50.0);
}

#[test]
fn composite_completion_emits_once_and_replays_after_restart() {
    let mut effect = EffectComponent::new(EffectKind::ScreenFlash(Default::default()));
    effect.common.looping = false;
    effect.common.duration = 0.01;
    let asset_id = AssetId::new();
    let mut world = SceneWorld::new();
    world.register_effect_asset(asset_id, EffectAsset::new("finite composite", vec![effect])).unwrap();
    let root = world.instantiate_effect_asset(asset_id, None, LocalTransform::IDENTITY).unwrap();
    world.update_effects(0.02);
    assert_eq!(world.effect_frame().events.iter().filter(|event| event.entity == root && event.kind == EffectEventKind::Completed).count(), 1);
    world.update_effects(0.02);
    assert!(!world.effect_frame().events.iter().any(|event| event.entity == root));
    world.control_effect(root, PlaybackCommand::Restart).unwrap();
    world.update_effects(0.02);
    assert_eq!(world.effect_frame().events.iter().filter(|event| event.entity == root && event.kind == EffectEventKind::Completed).count(), 1);
}
