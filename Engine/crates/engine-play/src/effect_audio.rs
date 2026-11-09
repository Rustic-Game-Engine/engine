//! Composite cues feed the existing engine-owned audio mixer and device output.
use engine_core::gameplay::{
    Owner,
    audio::{AudioClip, Mixer},
};
use engine_core::{AssetId, EntityId, SceneId, ScriptId};
use engine_world::{
    SceneWorld,
    effects::{EffectAudioCommand, EffectAudioEvent},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
struct GroupVoices {
    voices: BTreeMap<EntityId, CueVoice>,
    paused: bool,
}

struct CueVoice {
    base_pitch: f64,
    spatial_offset: Option<[f64; 3]>,
}

#[derive(Default)]
pub(crate) struct EffectAudioRuntime {
    root: PathBuf,
    sources: BTreeMap<AssetId, PathBuf>,
    clips: BTreeMap<AssetId, Arc<AudioClip>>,
    cached_samples: usize,
    groups: BTreeMap<EntityId, GroupVoices>,
    owner_script: ScriptId,
}
impl EffectAudioRuntime {
    pub(crate) fn stop(&mut self, mixer: &mut Mixer) {
        for state in self.groups.values() {
            for voice in state.voices.keys() {
                mixer.voices.remove(voice);
            }
        }
        self.groups.clear();
        self.clips.clear();
        self.cached_samples = 0;
    }
    pub(crate) fn set_root(&mut self, root: &Path) -> Result<(), String> {
        if self.root == root {
            return Ok(());
        }
        root.clone_into(&mut self.root);
        self.sources.clear();
        self.clips.clear();
        self.cached_samples = 0;
        self.groups.clear();
        self.refresh_sources()?;
        Ok(())
    }
    fn refresh_sources(&mut self) -> Result<(), String> {
        let assets = self.root.join("assets");
        if !assets.is_dir() {
            return Ok(());
        }
        self.sources = engine_assets::scan_sidecars(&assets)
            .map_err(|error| error.to_string())?
            .entries
            .into_iter()
            .map(|entry| (entry.meta.asset_id, assets.join(entry.source_relative)))
            .collect();
        Ok(())
    }
    fn clip(&mut self, asset: AssetId) -> Result<Arc<AudioClip>, String> {
        if let Some(clip) = self.clips.get(&asset) {
            return Ok(Arc::clone(clip));
        }
        if !self.sources.contains_key(&asset) {
            self.refresh_sources()?;
        }
        let source = self.sources.get(&asset).ok_or_else(|| {
            format!("effect sound asset {asset} is missing; keep its source and .rmeta together")
        })?;
        let assets = self
            .root
            .join("assets")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let resolved = source.canonicalize().map_err(|error| error.to_string())?;
        if !resolved.starts_with(&assets) {
            return Err("effect sound source escapes the assets directory".into());
        }
        if std::fs::metadata(&resolved)
            .map_err(|error| error.to_string())?
            .len()
            > u64::try_from(engine_assets::MAX_SOURCE_BYTES).unwrap_or(u64::MAX)
        {
            return Err("effect sound source exceeds the importer byte limit".into());
        }
        let request = engine_assets::ImportRequest::new(
            source
                .strip_prefix(&self.root)
                .map_err(|error| error.to_string())?,
            std::fs::read(&resolved).map_err(|error| error.to_string())?,
        );
        let engine_assets::DerivedArtifact::Audio(decoded) =
            engine_assets::ImporterRegistry::import(&request).map_err(|error| error.to_string())?
        else {
            return Err(format!("effect sound asset {asset} is not audio"));
        };
        if self.clips.len() >= 1024
            || self.cached_samples.saturating_add(decoded.samples.len()) > 16 * 1024 * 1024
        {
            return Err("effect audio clip cache limit exceeded".into());
        }
        self.cached_samples += decoded.samples.len();
        let clip = Arc::new(AudioClip {
            channels: decoded.channels,
            sample_rate: decoded.sample_rate,
            samples: decoded.samples,
        });
        clip.validate()?;
        self.clips.insert(asset, Arc::clone(&clip));
        Ok(clip)
    }
    pub(crate) fn dispatch(
        &mut self,
        scene: SceneId,
        world: &SceneWorld,
        events: Vec<EffectAudioEvent>,
        mixer: &mut Mixer,
    ) -> Result<(), String> {
        self.groups.retain(|group, state| {
            if !world.contains(*group) {
                for voice in state.voices.keys() {
                    mixer.voices.remove(voice);
                }
                return false;
            }
            state
                .voices
                .retain(|voice, _| mixer.voices.contains_key(voice));
            true
        });
        let mut errors = Vec::new();
        for event in events {
            match event.command {
                EffectAudioCommand::Play {
                    asset,
                    volume,
                    pitch,
                    spatial,
                    position,
                    looping,
                } => {
                    if !world.contains(event.group) {
                        continue;
                    }
                    let clip = match self.clip(asset) {
                        Ok(clip) => clip,
                        Err(error) => {
                            errors.push(error);
                            continue;
                        }
                    };
                    let owner = Owner {
                        entity_bound: true,
                        scene,
                        entity: event.group,
                        script: self.owner_script,
                    };
                    match mixer.play(
                        owner,
                        clip,
                        f64::from(volume),
                        f64::from(pitch),
                        looping,
                        spatial.then(|| position.map(f64::from)),
                    ) {
                        Ok(voice) => {
                            let group_position = world
                                .world_transform(event.group)
                                .map_err(|error| error.to_string())?
                                .0
                                .w_axis
                                .truncate()
                                .as_dvec3()
                                .to_array();
                            let state = self.groups.entry(event.group).or_default();
                            if let Some(value) = mixer.voices.get_mut(&voice) {
                                value.paused = state.paused;
                            }
                            state.voices.insert(
                                voice,
                                CueVoice {
                                    base_pitch: f64::from(pitch),
                                    spatial_offset: spatial.then(|| {
                                        std::array::from_fn(|axis| {
                                            f64::from(position[axis]) - group_position[axis]
                                        })
                                    }),
                                },
                            );
                        }
                        Err(error) => errors.push(error),
                    }
                }
                EffectAudioCommand::Pause | EffectAudioCommand::Resume => {
                    let state = self.groups.entry(event.group).or_default();
                    state.paused = matches!(event.command, EffectAudioCommand::Pause);
                    for id in state.voices.keys() {
                        if let Some(voice) = mixer.voices.get_mut(id) {
                            voice.paused = state.paused;
                        }
                    }
                }
                EffectAudioCommand::Stop => {
                    if let Some(state) = self.groups.remove(&event.group) {
                        for id in state.voices.into_keys() {
                            mixer.voices.remove(&id);
                        }
                    }
                }
            }
        }
        self.synchronize_voices(world, mixer)?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    fn synchronize_voices(&self, world: &SceneWorld, mixer: &mut Mixer) -> Result<(), String> {
        for (group, state) in &self.groups {
            let scale = f64::from(
                world
                    .effective_effect_time_scale(*group)
                    .map_err(|error| error.to_string())?,
            );
            let position = world
                .world_transform(*group)
                .map_err(|error| error.to_string())?
                .0
                .w_axis
                .truncate()
                .as_dvec3()
                .to_array();
            for (id, settings) in &state.voices {
                if let Some(voice) = mixer.voices.get_mut(id) {
                    voice.paused = state.paused || scale <= 0.0;
                    // A zero-speed timeline pauses its voice; Mixer requires positive pitch.
                    voice.pitch = settings.base_pitch * scale.max(f64::EPSILON);
                    if let Some(offset) = settings.spatial_offset {
                        voice.position =
                            Some(std::array::from_fn(|axis| position[axis] + offset[axis]));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Editor previews use the same cue decoder, mixer and output backend as Play.
pub struct EffectAudioPreview {
    runtime: EffectAudioRuntime,
    mixer: Mixer,
    output: crate::audio_output::AudioOutput,
    scene: SceneId,
}
impl EffectAudioPreview {
    /// Creates a bounded cue player for an isolated preview world.
    /// # Errors
    /// Returns asset-root/sidecar scan failures.
    pub fn new(project_root: &Path) -> Result<Self, String> {
        let mut runtime = EffectAudioRuntime::default();
        runtime.set_root(project_root)?;
        Ok(Self {
            runtime,
            mixer: Mixer::default(),
            output: crate::audio_output::AudioOutput::new(),
            scene: SceneId::new(),
        })
    }
    /// Drains due timeline cues, applies controls and submits actual mixed PCM.
    /// # Errors
    /// Returns missing/invalid sound or mixer errors without mutating authored data.
    pub fn step(&mut self, world: &mut SceneWorld, delta: f64) -> Result<(), String> {
        self.step_scaled(world, delta, 1.0)
    }
    /// Advances preview audio using wall-clock output frames and a separate source speed.
    /// # Errors
    /// Rejects invalid clocks and reports missing/invalid sound or mixer failures.
    pub fn step_scaled(
        &mut self,
        world: &mut SceneWorld,
        delta: f64,
        playback_speed: f64,
    ) -> Result<(), String> {
        let events = world.take_effect_audio_events();
        let result = self
            .runtime
            .dispatch(self.scene, world, events, &mut self.mixer);
        self.output
            .submit(self.mixer.mix_scaled(delta, playback_speed)?);
        result
    }
    /// Stops current preview voices; cached decoded sources remain reusable.
    pub fn stop(&mut self) {
        self.mixer.voices.clear();
        self.runtime.groups.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::EffectAudioRuntime;
    use engine_core::{AssetId, EntityId, SceneId, gameplay::audio::Mixer};
    use engine_world::{
        EntitySnapshot, SceneWorld, WorldCommand,
        effects::{EffectAudioCommand, EffectAudioEvent},
    };

    fn fixture() -> (
        tempfile::TempDir,
        EffectAudioRuntime,
        SceneWorld,
        EntityId,
        AssetId,
    ) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("assets")).unwrap();
        let sound = engine_assets::install_builtin_effect_resources(directory.path()).unwrap();
        let mut runtime = EffectAudioRuntime::default();
        runtime.set_root(directory.path()).unwrap();
        let mut world = SceneWorld::new();
        let entity = EntitySnapshot::default();
        let group = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        (directory, runtime, world, group, sound)
    }
    #[test]
    fn cues_mix_real_pcm_pause_resume_stop_and_reuse_decoded_asset() {
        let (_directory, mut runtime, world, group, sound) = fixture();
        let scene = SceneId::new();
        let mut mixer = Mixer::default();
        let play = EffectAudioEvent {
            group,
            command: EffectAudioCommand::Play {
                asset: sound,
                volume: 0.8,
                pitch: 1.0,
                spatial: false,
                position: [0.0; 3],
                looping: false,
            },
        };
        runtime
            .dispatch(scene, &world, vec![play.clone()], &mut mixer)
            .unwrap();
        assert!(
            mixer
                .mix(0.01)
                .unwrap()
                .iter()
                .any(|sample| sample.abs() > 0.001)
        );
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Pause,
                }],
                &mut mixer,
            )
            .unwrap();
        assert!(mixer.mix(0.01).unwrap().iter().all(|sample| *sample == 0.0));
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Resume,
                }],
                &mut mixer,
            )
            .unwrap();
        assert!(
            mixer
                .mix(0.01)
                .unwrap()
                .iter()
                .any(|sample| sample.abs() > 0.001)
        );
        runtime
            .dispatch(
                scene,
                &world,
                vec![
                    EffectAudioEvent {
                        group,
                        command: EffectAudioCommand::Stop,
                    },
                    play,
                ],
                &mut mixer,
            )
            .unwrap();
        assert_eq!(mixer.voices.len(), 1);
        assert_eq!(runtime.clips.len(), 1);
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Stop,
                }],
                &mut mixer,
            )
            .unwrap();
        assert!(mixer.voices.is_empty());
    }
    #[test]
    fn group_destruction_releases_voices_and_missing_sound_is_actionable() {
        let (_directory, mut runtime, mut world, group, sound) = fixture();
        let scene = SceneId::new();
        let mut mixer = Mixer::default();
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Play {
                        asset: sound,
                        volume: 1.0,
                        pitch: 1.0,
                        spatial: true,
                        position: [1.0, 0.0, 0.0],
                        looping: true,
                    },
                }],
                &mut mixer,
            )
            .unwrap();
        assert_eq!(
            mixer.voices.values().next().unwrap().position,
            Some([1.0, 0.0, 0.0])
        );
        world
            .apply_commands(&[WorldCommand::Despawn(group)])
            .unwrap();
        runtime.dispatch(scene, &world, vec![], &mut mixer).unwrap();
        assert!(mixer.voices.is_empty());
        assert!(runtime.groups.is_empty());
        let entity = EntitySnapshot::default();
        let group = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        let error = runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Play {
                        asset: AssetId::new(),
                        volume: 1.0,
                        pitch: 1.0,
                        spatial: false,
                        position: [0.0; 3],
                        looping: false,
                    },
                }],
                &mut mixer,
            )
            .unwrap_err();
        assert!(error.contains("is missing"));
        assert!(mixer.voices.is_empty());
    }

    #[test]
    fn stopping_cue_service_releases_owned_voices_and_cache_without_stopping_other_audio() {
        let (_directory, mut runtime, world, group, sound) = fixture();
        let scene = SceneId::new();
        let mut mixer = Mixer::default();
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Play {
                        asset: sound,
                        volume: 1.0,
                        pitch: 1.0,
                        spatial: false,
                        position: [0.0; 3],
                        looping: true,
                    },
                }],
                &mut mixer,
            )
            .unwrap();
        let clip = runtime.clip(sound).unwrap();
        let unrelated = mixer
            .play(
                engine_core::gameplay::Owner {
                    entity_bound: false,
                    scene,
                    entity: EntityId::new(),
                    script: engine_core::ScriptId::new(),
                },
                clip,
                0.4,
                1.0,
                true,
                None,
            )
            .unwrap();
        runtime.stop(&mut mixer);
        assert_eq!(mixer.voices.len(), 1);
        assert!(mixer.voices.contains_key(&unrelated));
        assert!(runtime.groups.is_empty());
        assert!(runtime.clips.is_empty());
        assert_eq!(runtime.cached_samples, 0);
    }
    #[test]
    fn cue_pitch_and_spatial_position_follow_group_speed_and_transform() {
        let (_directory, mut runtime, mut world, group, sound) = fixture();
        world
            .apply_commands(&[WorldCommand::SetEffectGroup {
                entity: group,
                value: Some(Box::new(engine_world::effects::EffectGroup {
                    time_scale: 2.0,
                    ..Default::default()
                })),
            }])
            .unwrap();
        let scene = SceneId::new();
        let mut mixer = Mixer::default();
        runtime
            .dispatch(
                scene,
                &world,
                vec![EffectAudioEvent {
                    group,
                    command: EffectAudioCommand::Play {
                        asset: sound,
                        volume: 1.0,
                        pitch: 1.25,
                        spatial: true,
                        looping: true,
                        position: [2.0, 0.0, 0.0],
                    },
                }],
                &mut mixer,
            )
            .unwrap();
        let voice = mixer.voices.values().next().unwrap();
        assert!((voice.pitch - 2.5).abs() < f64::EPSILON);
        world
            .apply_commands(&[
                WorldCommand::SetLocalTransform {
                    entity: group,
                    value: engine_world::LocalTransform {
                        translation: glam::Vec3::X * 3.0,
                        ..Default::default()
                    },
                },
                WorldCommand::SetEffectGroup {
                    entity: group,
                    value: Some(Box::new(engine_world::effects::EffectGroup {
                        time_scale: 0.0,
                        ..Default::default()
                    })),
                },
            ])
            .unwrap();
        world.propagate_transforms();
        runtime
            .dispatch(scene, &world, Vec::new(), &mut mixer)
            .unwrap();
        let voice = mixer.voices.values().next().unwrap();
        assert!(voice.paused);
        assert!((voice.position.unwrap()[0] - 5.0).abs() < f64::EPSILON);
        world
            .apply_commands(&[WorldCommand::SetEffectGroup {
                entity: group,
                value: Some(Box::new(engine_world::effects::EffectGroup {
                    time_scale: 0.5,
                    ..Default::default()
                })),
            }])
            .unwrap();
        runtime
            .dispatch(scene, &world, Vec::new(), &mut mixer)
            .unwrap();
        let voice = mixer.voices.values().next().unwrap();
        assert!(!voice.paused);
        assert!((voice.pitch - 0.625).abs() < f64::EPSILON);
    }
}
