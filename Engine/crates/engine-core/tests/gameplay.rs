#![allow(
    clippy::float_cmp,
    reason = "Exact endpoint and representable timing assertions define the public contract"
)]
use engine_core::gameplay::animation::*;
use engine_core::gameplay::*;
use engine_core::{EntityId, SceneId, ScriptId};
use glam::{DQuat, DVec3};
use std::collections::BTreeMap;

#[derive(Default)]
struct World(BTreeMap<EntityId, Value>);
impl GameplayWorld for World {
    fn contains(&self, id: EntityId) -> bool {
        self.0.contains_key(&id)
    }
    fn read(&self, target: &Target) -> Result<Value, String> {
        self.0
            .get(&target.entity)
            .copied()
            .ok_or("stale target".into())
    }
    fn write(&mut self, target: &Target, value: Value) -> Result<(), String> {
        if !self.contains(target.entity) {
            return Err("stale target".into());
        }
        self.0.insert(target.entity, value);
        Ok(())
    }
}
fn fixture() -> (GameplayRuntime, Owner, World) {
    let owner = Owner {
        entity_bound: true,
        scene: SceneId::new(),
        entity: EntityId::new(),
        script: ScriptId::new(),
    };
    let mut world = World::default();
    world.0.insert(owner.entity, Value::Number(0.0));
    (GameplayRuntime::default(), owner, world)
}
fn tween(owner: Owner, to: f64, duration: f64, ease: Ease) -> Action {
    Action::tween(
        Target {
            entity: owner.entity,
            property: "value".into(),
        },
        Value::Number(to),
        duration,
        ease,
    )
}
fn start(rt: &mut GameplayRuntime, owner: Owner, action: Action, playback: Playback) {
    rt.request(
        owner,
        Request::Start {
            handle: "action".into(),
            action,
            playback,
            on_finished: Some("done".into()),
        },
    )
    .unwrap();
}
fn number(world: &World, owner: Owner) -> f64 {
    let Value::Number(n) = world.0[&owner.entity] else {
        panic!()
    };
    n
}
#[test]
fn all_curves_have_exact_endpoints_and_finite_samples() {
    let curves = [
        Ease::Linear,
        Ease::InSine,
        Ease::OutSine,
        Ease::InOutSine,
        Ease::InQuad,
        Ease::OutQuad,
        Ease::InOutQuad,
        Ease::InCubic,
        Ease::OutCubic,
        Ease::InOutCubic,
        Ease::InQuart,
        Ease::OutQuart,
        Ease::InOutQuart,
        Ease::InQuint,
        Ease::OutQuint,
        Ease::InOutQuint,
        Ease::InExpo,
        Ease::OutExpo,
        Ease::InOutExpo,
        Ease::InCirc,
        Ease::OutCirc,
        Ease::InOutCirc,
        Ease::InBack,
        Ease::OutBack,
        Ease::InOutBack,
        Ease::InElastic,
        Ease::OutElastic,
        Ease::InOutElastic,
        Ease::InBounce,
        Ease::OutBounce,
        Ease::InOutBounce,
    ];
    for ease in curves {
        assert_eq!(ease.sample(0.0), 0.0);
        assert_eq!(ease.sample(1.0), 1.0);
        for i in 0..=1000 {
            assert!(ease.sample(f64::from(i) / 1000.0).is_finite());
        }
    }
    assert!(Ease::OutBack.sample(0.7) > 1.0);
}
#[test]
fn easing_pause_scale_reverse_and_completion_share_one_clock() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        tween(owner, 8.0, 2.0, Ease::InQuad),
        Playback::default(),
    );
    rt.tick(1.0, &mut world).unwrap();
    assert_eq!(number(&world, owner), 2.0);
    rt.request(
        owner,
        Request::Pause {
            handle: "action".into(),
        },
    )
    .unwrap();
    rt.tick(1.0, &mut world).unwrap();
    assert_eq!(number(&world, owner), 2.0);
    rt.request(
        owner,
        Request::Resume {
            handle: "action".into(),
        },
    )
    .unwrap();
    rt.request(
        owner,
        Request::Reverse {
            handle: "action".into(),
        },
    )
    .unwrap();
    rt.set_time_scale(0.5).unwrap();
    rt.tick(2.0, &mut world).unwrap();
    assert_eq!(number(&world, owner), 0.0);
    assert_eq!(rt.states(owner)[0].status, Status::Finished);
    assert_eq!(rt.drain_callbacks(owner)[0].token, "done");
    assert!(rt.drain_callbacks(owner).is_empty());
}
#[test]
fn sequence_consumes_leftover_time_and_captures_later_starts() {
    let (mut rt, owner, mut world) = fixture();
    let action = Action::Sequence {
        actions: vec![
            tween(owner, 10.0, 1.0, Ease::Linear),
            Action::Wait { duration: 0.5 },
            tween(owner, 20.0, 1.0, Ease::Linear),
        ],
    };
    start(&mut rt, owner, action, Playback::default());
    rt.tick(2.0, &mut world).unwrap();
    assert_eq!(number(&world, owner), 15.0);
    rt.tick(0.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 20.0);
}
#[test]
fn parallel_finishes_after_longest_child() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        Action::Sequence {
            actions: vec![
                Action::Parallel {
                    actions: vec![
                        Action::Wait { duration: 2.0 },
                        Action::Wait { duration: 0.25 },
                    ],
                },
                tween(owner, 10.0, 1.0, Ease::Linear),
            ],
        },
        Playback::default(),
    );
    rt.tick(2.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 5.0);
}
#[test]
fn repeating_timer_preserves_overshoot_and_cancel_drops_completion() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        Action::Sequence {
            actions: vec![
                Action::Wait { duration: 0.5 },
                Action::Callback {
                    token: "timer".into(),
                },
            ],
        },
        Playback {
            repeats: Some(2),
            ..Playback::default()
        },
    );
    rt.tick(1.6, &mut world).unwrap();
    let tokens: Vec<_> = rt
        .drain_callbacks(owner)
        .into_iter()
        .map(|d| d.token)
        .collect();
    assert_eq!(tokens, ["timer", "timer", "timer", "done"]);
}
#[test]
fn zero_time_infinite_loop_is_bounded() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        Action::Wait { duration: 0.0 },
        Playback {
            looping: true,
            ..Playback::default()
        },
    );
    rt.tick(1.0, &mut world).unwrap();
    assert_eq!(rt.states(owner)[0].status, Status::Failed);
}
#[test]
fn ping_pong_returns_to_original_value() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        tween(owner, 10.0, 1.0, Ease::Linear),
        Playback {
            repeats: Some(1),
            ping_pong: true,
            ..Playback::default()
        },
    );
    rt.tick(1.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 5.0);
    rt.tick(0.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 0.0);
}
#[test]
fn failed_target_and_owner_cleanup_never_call_completion() {
    let (mut rt, owner, mut world) = fixture();
    let target = EntityId::new();
    start(
        &mut rt,
        owner,
        Action::tween(
            Target {
                entity: target,
                property: "value".into(),
            },
            Value::Number(1.0),
            1.0,
            Ease::Linear,
        ),
        Playback::default(),
    );
    rt.tick(1.0, &mut world).unwrap();
    assert!(rt.states(owner).is_empty());
    assert!(rt.drain_callbacks(owner).is_empty());
    rt.cleanup(|o| o.scene == owner.scene);
    assert!(rt.states(owner).is_empty());
}
#[test]
fn destroying_target_removes_already_queued_callbacks() {
    let (mut rt, owner, mut world) = fixture();
    let target = EntityId::new();
    world.0.insert(target, Value::Number(0.0));
    start(
        &mut rt,
        owner,
        Action::tween(
            Target {
                entity: target,
                property: "value".into(),
            },
            Value::Number(1.0),
            0.1,
            Ease::Linear,
        ),
        Playback::default(),
    );
    rt.tick(1.0, &mut world).unwrap();
    rt.destroy_entity(target);
    assert!(rt.drain_callbacks(owner).is_empty());
}
#[test]
fn signals_once_disconnect_and_source_destruction() {
    let (mut rt, owner, _) = fixture();
    let source = EntityId::new();
    let signal = Signal {
        name: "hit".into(),
        source: Some(source),
    };
    rt.signals
        .connect(owner, "once".into(), signal.clone(), true)
        .unwrap();
    rt.signals
        .emit(&signal, vec![Value::Number(2.0).into()])
        .unwrap();
    rt.signals.emit(&signal, vec![]).unwrap();
    assert_eq!(rt.drain_callbacks(owner).len(), 1);
    rt.signals
        .connect(owner, "persistent".into(), signal.clone(), false)
        .unwrap();
    rt.signals.emit(&signal, vec![]).unwrap();
    rt.destroy_entity(source);
    assert!(rt.drain_callbacks(owner).is_empty());
}
#[test]
fn path_speed_uses_distance_and_easing_does_not_change_control_points() {
    let path = Path {
        kind: PathKind::Linear,
        points: vec![
            PathPoint {
                point: [0.0, 0.0, 0.0],
                easing: Ease::Linear,
            },
            PathPoint {
                point: [2.0, 0.0, 0.0],
                easing: Ease::Linear,
            },
            PathPoint {
                point: [10.0, 0.0, 0.0],
                easing: Ease::Linear,
            },
        ],
    };
    let (mut rt, owner, mut world) = fixture();
    world.0.insert(owner.entity, Value::Vector([0.0; 3]));
    start(
        &mut rt,
        owner,
        Action::Path {
            entity: owner.entity,
            path: path.clone(),
            timing: Timing::Speed { speed: 2.0 },
            easing: Ease::Linear,
            orient_to_path: false,
        },
        Playback::default(),
    );
    rt.tick(2.5, &mut world).unwrap();
    let Value::Vector(v) = world.0[&owner.entity] else {
        panic!()
    };
    assert!((v[0] - 5.0).abs() < 1e-9);
    assert_eq!(path.points[1].point, [2.0, 0.0, 0.0]);
}
#[test]
fn cubic_and_catmull_paths_reach_endpoints() {
    for kind in [
        PathKind::CubicBezier,
        PathKind::CatmullRom,
        PathKind::Spline,
    ] {
        let p = Path {
            kind,
            points: (0..4)
                .map(|i| PathPoint {
                    point: [f64::from(i), 0.0, 0.0],
                    easing: Ease::Linear,
                })
                .collect(),
        };
        p.validate().unwrap();
        assert_eq!(p.sample(0.0), DVec3::ZERO);
        assert_eq!(p.sample(1.0), DVec3::new(3.0, 0.0, 0.0));
    }
}
#[test]
fn interpolation_uses_shortest_quaternion_arc_and_damping_is_frame_invariant() {
    let a = DQuat::IDENTITY;
    let b = DQuat::from_rotation_y(1.0);
    let half = smooth::slerp(a, -b, 0.5, Ease::Linear);
    assert!(half.abs_diff_eq(DQuat::from_rotation_y(0.5), 1e-9));
    let whole = smooth::smooth_damp(0.0, 1.0, 0.0, 0.5, 1.0);
    let first = smooth::smooth_damp(0.0, 1.0, 0.0, 0.5, 0.5);
    let second = smooth::smooth_damp(first.0, 1.0, first.1, 0.5, 0.5);
    assert!((whole.0 - second.0).abs() < 1e-9);
    assert!((whole.1 - second.1).abs() < 1e-9);
}
#[test]
fn animation_blend_easing_preserves_source_timing_and_marker_crossings() {
    let clip = Clip {
        name: "Run".into(),
        duration: 2.0,
        tracks: vec![Track {
            interpolation: TrackInterpolation::Linear,
            target: "joint".into(),
            keys: vec![
                Keyframe {
                    time: 0.0,
                    value: Value::Number(0.0),
                    easing: Ease::Linear,
                },
                Keyframe {
                    time: 2.0,
                    value: Value::Number(2.0),
                    easing: Ease::Linear,
                },
            ],
        }],
        markers: vec![Marker {
            time: 0.5,
            name: "Hit".into(),
        }],
    };
    let mut player = ClipPlayer::new(AnimationOptions {
        blend_in: 2.0,
        blend_in_ease: Ease::InQuad,
        ..AnimationOptions::default()
    })
    .unwrap();
    let frame = player.tick(&clip, 1.0).unwrap();
    assert_eq!(frame.pose["joint"], Value::Number(1.0));
    assert_eq!(frame.weight, 0.25);
    assert_eq!(frame.markers, ["Hit"]);
    player.paused = true;
    assert_eq!(
        player.tick(&clip, 1.0).unwrap().pose["joint"],
        Value::Number(1.0)
    );
    player.paused = false;
    assert!(player.tick(&clip, 1.0).unwrap().finished);
}
#[test]
fn invalid_values_timing_and_deep_action_trees_are_rejected() {
    let (mut rt, owner, _) = fixture();
    assert!(rt.set_time_scale(f64::NAN).is_err());
    assert!(Timing::Speed { speed: 0.0 }.validate().is_err());
    assert!(Value::Rotation([0.0; 4]).validate().is_err());
    let mut action = tween(owner, 1.0, 1.0, Ease::Linear);
    for _ in 0..34 {
        action = Action::Sequence {
            actions: vec![action],
        };
    }
    assert!(action.validate().is_err());
}
#[test]
fn newest_operation_writes_last_regardless_of_handle_sort_order() {
    let (mut rt, owner, mut world) = fixture();
    for (handle, to) in [("z", 10.0), ("a", 20.0)] {
        rt.request(
            owner,
            Request::Start {
                handle: handle.into(),
                action: tween(owner, to, 1.0, Ease::Linear),
                playback: Playback::default(),
                on_finished: None,
            },
        )
        .unwrap();
    }
    rt.tick(0.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 12.5);
}
#[test]
fn segment_easing_changes_progress_without_changing_spline_sample() {
    let mut path = Path {
        kind: PathKind::CatmullRom,
        points: (0..3)
            .map(|i| PathPoint {
                point: [f64::from(i), 0.0, 0.0],
                easing: Ease::Linear,
            })
            .collect(),
    };
    let before = path.sample(0.25);
    path.points[0].easing = Ease::InQuad;
    assert_eq!(path.sample(0.25), before);
    assert_eq!(path.traversal_parameter(0.25), 0.125);
}
#[test]
fn value_tween_queues_samples_before_completion_and_cancel_removes_all() {
    let (mut rt, owner, mut world) = fixture();
    start(
        &mut rt,
        owner,
        Action::Value {
            from: Value::Number(0.0),
            to: Value::Number(8.0),
            timing: Timing::Duration { duration: 2.0 },
            easing: Ease::InQuad,
            token: "sample".into(),
        },
        Playback::default(),
    );
    rt.tick(1.0, &mut world).unwrap();
    assert_eq!(
        rt.drain_callbacks(owner)[0].arguments,
        [Payload::Value(Value::Number(2.0))]
    );
    rt.tick(1.0, &mut world).unwrap();
    rt.request(
        owner,
        Request::Cancel {
            handle: "action".into(),
        },
    )
    .unwrap();
    assert!(rt.drain_callbacks(owner).is_empty());
}
#[test]
fn global_timer_survives_unrelated_object_removal() {
    let (mut rt, mut owner, mut world) = fixture();
    owner.entity_bound = false;
    start(
        &mut rt,
        owner,
        Action::Wait { duration: 1.0 },
        Playback::default(),
    );
    world.0.clear();
    rt.tick(1.0, &mut world).unwrap();
    assert_eq!(rt.drain_callbacks(owner)[0].token, "done");
}

#[test]
fn shared_queries_validate_and_damp_without_language_math() {
    use engine_core::gameplay::query::{Query, QueryResult};
    let result = Query::Lerp {
        from: Value::Number(0.0),
        to: Value::Number(8.0),
        progress: 0.5,
        easing: Ease::InQuad,
    }
    .evaluate()
    .unwrap();
    assert!(matches!(result, QueryResult::Value(Value::Number(2.0))));
    assert!(
        Query::SmoothDamp {
            current: 0.0,
            target: 1.0,
            velocity: 0.0,
            smooth_time: 0.0,
            delta: 0.1
        }
        .evaluate()
        .is_err()
    );
}

#[test]
fn physics_casts_and_overlaps_return_nearest_bounds_and_exclude_ignored() {
    use engine_core::gameplay::physics::{Collider, cast, overlap};
    let id = EntityId::new();
    let colliders = [Collider {
        entity: id,
        center: [0.0, 0.0, 5.0],
        half: [1.0; 3],
    }];
    let hit = cast(&colliders, [0.0; 3], [0.0, 0.0, 2.0], 10.0, 0.0, &[])
        .unwrap()
        .unwrap();
    assert_eq!(hit.distance, 4.0);
    assert_eq!(hit.normal, [0.0, 0.0, -1.0]);
    assert!(
        cast(&colliders, [0.0; 3], [0.0, 0.0, 1.0], 10.0, 0.0, &[id])
            .unwrap()
            .is_none()
    );
    assert_eq!(
        overlap(&colliders, [0.0, 0.0, 3.5], 0.5, &[]).unwrap(),
        [id]
    );
}

#[test]
fn audio_mixer_resamples_pitch_spatial_gain_and_cleans_owners() {
    use engine_core::gameplay::audio::{AudioClip, Mixer};
    let mut mixer = Mixer::default();
    let (_, owner, _) = fixture();
    let clip = std::sync::Arc::new(AudioClip {
        channels: 1,
        sample_rate: 48000,
        samples: vec![0.5; 480],
    });
    let id = mixer
        .play(owner, clip, 1.0, 2.0, false, Some([1.0, 0.0, 0.0]))
        .unwrap();
    let samples = mixer.mix(0.005).unwrap();
    assert_eq!(samples.len(), 480);
    assert!((samples[0] - 0.25).abs() < 1e-6);
    assert!(!mixer.voices.contains_key(&id));
}

#[test]
fn sphere_cast_rounds_box_corners_and_ik_preserves_bone_lengths() {
    use engine_core::gameplay::{
        physics::{Collider, cast},
        procedural::two_bone,
    };
    let collider = Collider {
        entity: EntityId::new(),
        center: [0., 0., 5.],
        half: [1.; 3],
    };
    assert!(
        cast(&[collider], [1.9, 1.9, 0.], [0., 0., 1.], 10., 1., &[])
            .unwrap()
            .is_none()
    );
    let result = two_bone(
        DVec3::ZERO,
        DVec3::X,
        DVec3::X * 2.,
        DVec3::new(1., 1., 0.),
        DVec3::Z,
    )
    .unwrap();
    assert!((result.elbow.length() - 1.).abs() < 1e-8);
    assert!((result.tip.distance(result.elbow) - 1.).abs() < 1e-8);
}

#[test]
fn clip_actions_blend_markers_sequence_and_speed_share_runtime() {
    let (mut runtime, owner, mut world) = fixture();
    let clip = Clip {
        name: "Pulse".into(),
        duration: 1.,
        tracks: vec![Track {
            target: "value".into(),
            interpolation: TrackInterpolation::Linear,
            keys: vec![
                Keyframe {
                    time: 0.,
                    value: Value::Number(0.),
                    easing: Ease::Linear,
                },
                Keyframe {
                    time: 1.,
                    value: Value::Number(8.),
                    easing: Ease::Linear,
                },
            ],
        }],
        markers: vec![Marker {
            time: 0.5,
            name: "Hit".into(),
        }],
    };
    let action = Action::Animation {
        entity: owner.entity,
        clip,
        options: AnimationOptions {
            blend_in: 1.,
            blend_in_ease: Ease::InQuad,
            ..Default::default()
        },
        marker_token: Some("marker".into()),
        blend_source: None,
        mask: vec![],
    };
    start(&mut runtime, owner, action, Playback::default());
    runtime.tick(0.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 1.);
    assert_eq!(
        runtime.drain_callbacks(owner)[0].arguments,
        [Payload::String("Hit".into())]
    );
    runtime
        .request(
            owner,
            Request::Speed {
                handle: "action".into(),
                speed: 2.,
            },
        )
        .unwrap();
    runtime.tick(0.25, &mut world).unwrap();
    assert_eq!(number(&world, owner), 4.5);
    assert_eq!(runtime.states(owner)[0].status, Status::Finished);
}

#[test]
fn value_keyframes_share_easing_and_preserve_sequence_leftover_time() {
    use engine_core::gameplay::query::{Query, QueryResult};
    let query = Query::Keyframes {
        token: "sample".into(),
        keys: vec![
            Keyframe {
                time: 0.,
                value: Value::Number(0.),
                easing: Ease::InQuad,
            },
            Keyframe {
                time: 2.,
                value: Value::Number(8.),
                easing: Ease::Linear,
            },
            Keyframe {
                time: 3.,
                value: Value::Number(10.),
                easing: Ease::Linear,
            },
        ],
    };
    let QueryResult::Action(action) = query.evaluate().unwrap() else {
        panic!("expected keyframe action");
    };
    let (mut runtime, owner, mut world) = fixture();
    start(&mut runtime, owner, *action, Playback::default());
    runtime.tick(1., &mut world).unwrap();
    assert_eq!(
        runtime.drain_callbacks(owner)[0].arguments,
        vec![Payload::Value(Value::Number(2.))]
    );
    runtime.tick(1.5, &mut world).unwrap();
    assert_eq!(
        runtime.drain_callbacks(owner).last().unwrap().arguments,
        vec![Payload::Value(Value::Number(9.))]
    );
    let Query::Keyframes { mut keys, token } = query else {
        unreachable!()
    };
    keys[1].time = 0.;
    assert!(Query::Keyframes { keys, token }.evaluate().is_err());
}

#[test]
fn named_animation_references_resolve_inside_sequences_and_masks_skip_missing_tracks() {
    let (mut runtime, owner, mut world) = fixture();
    let clip = Clip {
        name: "Layer".into(),
        duration: 1.,
        markers: vec![],
        tracks: vec![Track {
            target: "value".into(),
            interpolation: TrackInterpolation::default(),
            keys: vec![
                Keyframe {
                    time: 0.,
                    value: Value::Number(0.),
                    easing: Ease::Linear,
                },
                Keyframe {
                    time: 1.,
                    value: Value::Number(10.),
                    easing: Ease::Linear,
                },
            ],
        }],
    };
    let mut action = Action::Sequence {
        actions: vec![Action::AnimationRef {
            entity: owner.entity,
            clip: ClipReference::Named("Layer".into()),
            options: AnimationOptions::default(),
            marker_token: None,
            blend_source: None,
            mask: vec!["value".into()],
        }],
    };
    action
        .resolve_clips(&|_, name| {
            if name == "Layer" {
                Ok(clip.clone())
            } else {
                Err("unknown clip".into())
            }
        })
        .unwrap();
    start(&mut runtime, owner, action, Playback::default());
    runtime.tick(0.5, &mut world).unwrap();
    assert_eq!(number(&world, owner), 5.);
    runtime.paused = true;
    world.0.remove(&owner.entity);
    runtime.tick(0.1, &mut world).unwrap();
    assert!(runtime.states(owner).is_empty());
}

#[test]
fn animation_loop_retains_blend_clock_and_can_finish_the_current_cycle() {
    let (mut runtime, owner, mut world) = fixture();
    let action = Action::Animation {
        entity: owner.entity,
        clip: Clip {
            name: "Walk".into(),
            duration: 1.,
            markers: vec![],
            tracks: vec![Track {
                target: "value".into(),
                interpolation: TrackInterpolation::Linear,
                keys: vec![
                    Keyframe {
                        time: 0.,
                        value: Value::Number(0.),
                        easing: Ease::Linear,
                    },
                    Keyframe {
                        time: 1.,
                        value: Value::Number(8.),
                        easing: Ease::Linear,
                    },
                ],
            }],
        },
        options: AnimationOptions {
            blend_in: 0.5,
            ..AnimationOptions::default()
        },
        marker_token: None,
        blend_source: None,
        mask: vec![],
    };
    start(
        &mut runtime,
        owner,
        action,
        Playback {
            looping: true,
            ..Playback::default()
        },
    );
    runtime.tick(0.25, &mut world).unwrap();
    assert_eq!(number(&world, owner), 1.);
    runtime.tick(1., &mut world).unwrap();
    assert_eq!(number(&world, owner), 2.);
    runtime
        .request(
            owner,
            Request::Loop {
                handle: "action".into(),
                looping: false,
            },
        )
        .unwrap();
    runtime.tick(0.75, &mut world).unwrap();
    assert_eq!(number(&world, owner), 8.);
    assert_eq!(runtime.states(owner)[0].status, Status::Finished);
}

#[test]
fn audio_long_frames_preserve_source_clock_with_bounded_output() {
    use engine_core::gameplay::audio::{AudioClip, Mixer};
    use std::sync::Arc;
    let (_, owner, _) = fixture();
    let mut mixer = Mixer::default();
    mixer
        .play(
            owner,
            Arc::new(AudioClip {
                channels: 1,
                sample_rate: 8000,
                samples: vec![0.5; 16000],
            }),
            1.,
            1.,
            false,
            None,
        )
        .unwrap();
    assert_eq!(mixer.mix_scaled(2.5, 1.).unwrap().len(), 96000);
    assert!(mixer.voices.is_empty());
    mixer
        .play(
            owner,
            Arc::new(AudioClip {
                channels: 1,
                sample_rate: 8000,
                samples: vec![0.5; 8000],
            }),
            1.,
            1.,
            false,
            None,
        )
        .unwrap();
    assert!(
        mixer
            .mix_scaled(2.5, 0.)
            .unwrap()
            .iter()
            .all(|value| *value == 0.)
    );
    assert_eq!(mixer.voices.len(), 1);
}
