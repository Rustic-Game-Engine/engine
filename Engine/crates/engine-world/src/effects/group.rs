//! Persisted composite timelines and bounded delivery into the existing audio service.
use super::{EffectKind, EffectSoundCue, PlaybackCommand, PlaybackState};
use crate::{AssetId, EntityId};
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectGroupDefaults {
    pub settings: EffectGroupSettings,
    pub sound_cues: Vec<EffectSoundCue>,
    pub parameters: BTreeMap<String, super::EffectParameter>,
    pub bindings: Vec<super::GroupParameterBinding>,
    pub trigger_bindings: Vec<super::GroupTriggerBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectGroupSettings {
    pub auto_play: bool,
    pub looping: bool,
    pub duration: f32,
    pub time_scale: f32,
}
impl Default for EffectGroupSettings {
    fn default() -> Self {
        Self {
            auto_play: true,
            looping: false,
            duration: 5.0,
            time_scale: 1.0,
        }
    }
}
impl EffectGroupSettings {
    /// # Errors
    /// Rejects non-finite, nonpositive durations and unbounded playback speeds.
    pub fn validate(&self) -> Result<(), String> {
        if !self.duration.is_finite()
            || self.duration <= 0.0
            || !self.time_scale.is_finite()
            || !(0.0..=1000.0).contains(&self.time_scale)
        {
            return Err(
                "composite duration must be finite and positive; speed must be 0..1000".into(),
            );
        }
        Ok(())
    }
}

#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectGroup {
    pub source: Option<AssetId>,
    pub sound_cues: Vec<EffectSoundCue>,
    pub auto_play: bool,
    pub looping: bool,
    pub duration: f32,
    pub time_scale: f32,
    pub parameters: BTreeMap<String, super::EffectParameter>,
    pub bindings: Vec<super::GroupParameterBinding>,
    pub trigger_bindings: Vec<super::GroupTriggerBinding>,
    /// Authored track order, independent of scene entity sorting.
    pub child_entities: Vec<EntityId>,
    pub source_baseline: Option<Box<EffectGroupDefaults>>,
}
impl Default for EffectGroup {
    fn default() -> Self {
        let settings = EffectGroupSettings::default();
        Self {
            source: None,
            sound_cues: Vec::new(),
            auto_play: settings.auto_play,
            looping: settings.looping,
            duration: settings.duration,
            time_scale: settings.time_scale,
            parameters: BTreeMap::new(),
            bindings: Vec::new(),
            trigger_bindings: Vec::new(),
            child_entities: Vec::new(),
            source_baseline: None,
        }
    }
}
impl From<&EffectGroup> for EffectGroupSettings {
    fn from(group: &EffectGroup) -> Self {
        Self {
            auto_play: group.auto_play,
            looping: group.looping,
            duration: group.duration,
            time_scale: group.time_scale,
        }
    }
}
impl EffectGroup {
    pub fn from_asset(source: Option<AssetId>, asset: &super::EffectAsset) -> Self {
        let settings = asset.group_settings.unwrap_or_else(|| {
            let looping = asset.effects.iter().any(|effect| effect.common.looping);
            let duration = asset
                .effects
                .iter()
                .map(|effect| {
                    let tail = if effect.common.looping {
                        0.0
                    } else {
                        match &effect.kind {
                            EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.lifetime[1],
                            EffectKind::Weather(w) => w.emitter.lifetime[1],
                            EffectKind::Trail(t) => t.lifetime,
                            _ => 0.0,
                        }
                    };
                    (effect.common.delay + effect.common.duration + tail)
                        / effect.common.time_scale.max(0.001)
                })
                .chain(asset.sound_cues.iter().map(|cue| cue.delay + 0.01))
                .fold(0.01_f32, f32::max);
            EffectGroupSettings {
                auto_play: asset.effects.iter().any(|effect| effect.common.auto_play),
                looping,
                duration,
                time_scale: 1.0,
            }
        });
        let mut group = Self {
            source,
            sound_cues: asset.sound_cues.clone(),
            auto_play: settings.auto_play,
            looping: settings.looping,
            duration: settings.duration,
            time_scale: settings.time_scale,
            parameters: asset.parameters.clone(),
            bindings: asset.bindings.clone(),
            trigger_bindings: asset.trigger_bindings.clone(),
            child_entities: Vec::new(),
            source_baseline: None,
        };
        group.source_baseline = Some(Box::new(group.defaults()));
        group
    }

    pub fn from_asset_with_children(
        source: Option<AssetId>,
        asset: &super::EffectAsset,
        children: &[crate::EntitySnapshot],
    ) -> Self {
        let mut group = Self::from_asset(source, asset);
        group.child_entities = children.iter().map(|child| child.id).collect();
        group
    }

    pub fn remap_entities(&mut self, mapping: &BTreeMap<EntityId, EntityId>) {
        for entity in &mut self.child_entities {
            if let Some(mapped) = mapping.get(entity) {
                *entity = *mapped;
            }
        }
    }

    /// Removes deleted tracks and remaps all surviving index-based bindings.
    pub fn remove_children(&mut self, removed: &BTreeSet<EntityId>) {
        let mut mapping = BTreeMap::new();
        let mut children = Vec::with_capacity(self.child_entities.len());
        for (old, child) in self.child_entities.iter().enumerate() {
            if !removed.contains(child) {
                if let (Ok(old), Ok(new)) = (u32::try_from(old), u32::try_from(children.len())) {
                    mapping.insert(old, new);
                }
                children.push(*child);
            }
        }
        self.child_entities = children;
        self.bindings.retain_mut(|binding| {
            if let Some(child) = mapping.get(&binding.child) {
                binding.child = *child;
                true
            } else {
                false
            }
        });
        self.trigger_bindings.retain_mut(|binding| {
            match (
                mapping.get(&binding.source_child),
                mapping.get(&binding.target_child),
            ) {
                (Some(source), Some(target)) => {
                    binding.source_child = *source;
                    binding.target_child = *target;
                    true
                }
                _ => false,
            }
        });
        if let Some(baseline) = &mut self.source_baseline {
            baseline.bindings.retain_mut(|binding| {
                if let Some(child) = mapping.get(&binding.child) {
                    binding.child = *child;
                    true
                } else {
                    false
                }
            });
            baseline.trigger_bindings.retain_mut(|binding| {
                match (
                    mapping.get(&binding.source_child),
                    mapping.get(&binding.target_child),
                ) {
                    (Some(source), Some(target)) => {
                        binding.source_child = *source;
                        binding.target_child = *target;
                        true
                    }
                    _ => false,
                }
            });
        }
    }

    fn defaults(&self) -> EffectGroupDefaults {
        EffectGroupDefaults {
            settings: EffectGroupSettings::from(self),
            sound_cues: self.sound_cues.clone(),
            parameters: self.parameters.clone(),
            bindings: self.bindings.clone(),
            trigger_bindings: self.trigger_bindings.clone(),
        }
    }

    /// Refreshes shared defaults while retaining per-instance authored overrides.
    #[allow(
        clippy::float_cmp,
        reason = "authored defaults use exact value identity to distinguish overrides"
    )]
    #[must_use]
    pub fn refresh_defaults(&self, asset: &super::EffectAsset) -> Self {
        let Some(old) = &self.source_baseline else {
            return self.clone();
        };
        let defaults = Self::from_asset(self.source, asset);
        let mut merged = self.clone();
        if self.auto_play == old.settings.auto_play {
            merged.auto_play = defaults.auto_play;
        }
        if self.looping == old.settings.looping {
            merged.looping = defaults.looping;
        }
        if self.duration == old.settings.duration {
            merged.duration = defaults.duration;
        }
        if self.time_scale == old.settings.time_scale {
            merged.time_scale = defaults.time_scale;
        }
        if self.sound_cues == old.sound_cues {
            merged.sound_cues.clone_from(&defaults.sound_cues);
        }
        if self.bindings == old.bindings {
            merged.bindings.clone_from(&defaults.bindings);
        }
        if self.trigger_bindings == old.trigger_bindings {
            merged
                .trigger_bindings
                .clone_from(&defaults.trigger_bindings);
        }
        for name in old
            .parameters
            .keys()
            .chain(defaults.parameters.keys())
            .collect::<BTreeSet<_>>()
        {
            if self.parameters.get(name) == old.parameters.get(name) {
                if let Some(value) = defaults.parameters.get(name) {
                    merged.parameters.insert(name.clone(), value.clone());
                } else {
                    merged.parameters.remove(name);
                }
            }
        }
        if merged
            .sound_cues
            .iter()
            .any(|cue| cue.delay >= merged.duration)
        {
            merged.duration = self.duration;
        }
        merged.source_baseline = Some(Box::new(defaults.defaults()));
        merged
    }

    /// # Errors
    /// Rejects invalid playback settings, cue counts, cue times and audio controls.
    pub fn validate(&self) -> Result<(), String> {
        EffectGroupSettings::from(self).validate()?;
        if self.parameters.len() > 128
            || self.bindings.len() > 256
            || self.trigger_bindings.len() > 256
            || self.child_entities.len() > 1024
        {
            return Err("composite parameter, binding or child capacity exceeded".into());
        }
        for (name, parameter) in &self.parameters {
            let finite = match parameter {
                super::EffectParameter::Scalar(value) => value.is_finite(),
                super::EffectParameter::Vector(value) => {
                    value.iter().all(|value| value.is_finite())
                }
                super::EffectParameter::Color(value) => value.iter().all(|value| value.is_finite()),
                super::EffectParameter::Boolean(_) => true,
            };
            if name.is_empty() || name.len() > 128 || !finite {
                return Err("invalid composite parameter".into());
            }
        }
        super::validate_group_triggers(&self.trigger_bindings, self.child_entities.len())?;
        if self.sound_cues.len() > 128 {
            return Err("composite supports at most 128 sound cues".into());
        }
        for cue in &self.sound_cues {
            if !cue.delay.is_finite()
                || cue.delay < 0.0
                || cue.delay >= self.duration
                || !cue.volume.is_finite()
                || !(0.0..=16.0).contains(&cue.volume)
                || !cue.pitch.is_finite()
                || !(0.01..=16.0).contains(&cue.pitch)
            {
                return Err("audio cue requires delay within the timeline, nonnegative volume and positive pitch".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectAudioCommand {
    Play {
        asset: AssetId,
        volume: f32,
        pitch: f32,
        spatial: bool,
        looping: bool,
        position: [f32; 3],
    },
    Pause,
    Resume,
    Stop,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EffectAudioEvent {
    pub group: EntityId,
    pub command: EffectAudioCommand,
}

#[derive(Debug)]
struct GroupState {
    time: f32,
    state: PlaybackState,
    start_pending: bool,
}
#[derive(Default)]
pub(crate) struct EffectGroupRuntime {
    states: BTreeMap<EntityId, GroupState>,
    events: Vec<EffectAudioEvent>,
    restarted: BTreeMap<EntityId, f32>,
    completed: BTreeSet<EntityId>,
}
const MAX_AUDIO_EVENTS: usize = 4096;
fn queue(events: &mut Vec<EffectAudioEvent>, group: EntityId, command: EffectAudioCommand) {
    if events.len() < MAX_AUDIO_EVENTS {
        events.push(EffectAudioEvent { group, command });
    }
}
impl EffectGroupRuntime {
    pub fn ensure(&mut self, entity: EntityId, group: &EffectGroup) {
        self.states.entry(entity).or_insert(GroupState {
            time: 0.0,
            state: if group.auto_play {
                PlaybackState::Playing
            } else {
                PlaybackState::Stopped
            },
            start_pending: group.auto_play,
        });
    }
    pub fn state(&self, entity: EntityId, group: &EffectGroup) -> PlaybackState {
        self.states.get(&entity).map_or(
            if group.auto_play {
                PlaybackState::Playing
            } else {
                PlaybackState::Stopped
            },
            |state| state.state,
        )
    }
    pub fn control(&mut self, entity: EntityId, group: &EffectGroup, command: PlaybackCommand) {
        self.ensure(entity, group);
        let state = self.states.get_mut(&entity).expect("ensured group exists");
        match command {
            PlaybackCommand::Pause if state.state == PlaybackState::Playing => {
                state.state = PlaybackState::Paused;
                queue(&mut self.events, entity, EffectAudioCommand::Pause);
            }
            PlaybackCommand::Resume if state.state == PlaybackState::Paused => {
                state.state = PlaybackState::Playing;
                queue(&mut self.events, entity, EffectAudioCommand::Resume);
            }
            PlaybackCommand::Play if state.state == PlaybackState::Playing => {}
            PlaybackCommand::Play | PlaybackCommand::Restart => {
                state.time = 0.0;
                state.state = PlaybackState::Playing;
                state.start_pending = true;
                queue(&mut self.events, entity, EffectAudioCommand::Stop);
            }
            PlaybackCommand::Stop | PlaybackCommand::Reset | PlaybackCommand::Dispose => {
                state.time = 0.0;
                state.state = if command == PlaybackCommand::Dispose {
                    PlaybackState::Disposed
                } else {
                    PlaybackState::Stopped
                };
                state.start_pending = false;
                queue(&mut self.events, entity, EffectAudioCommand::Stop);
            }
            _ => {}
        }
    }
    pub fn restart_at(
        &mut self,
        entity: EntityId,
        group: &EffectGroup,
        position: [f32; 3],
        elapsed: f32,
    ) {
        let mut restarted = Self::default();
        restarted.control(entity, group, PlaybackCommand::Restart);
        let mut scaled = group.clone();
        scaled.time_scale = elapsed.max(0.0);
        // A zero-offset restart must still deliver the first cue.
        if scaled.time_scale <= 0.0 {
            scaled.time_scale = f32::MIN_POSITIVE;
        }
        restarted.update(1.0, &[(entity, position, scaled)]);
        if let Some(state) = restarted.states.remove(&entity) {
            self.states.insert(entity, state);
        }
        for event in restarted.events {
            queue(&mut self.events, event.group, event.command);
        }
    }

    pub fn remove(&mut self, entity: EntityId) {
        if self.states.remove(&entity).is_some() {
            queue(&mut self.events, entity, EffectAudioCommand::Stop);
        }
    }
    pub fn take_events(&mut self) -> Vec<EffectAudioEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn take_restarted(&mut self) -> BTreeMap<EntityId, f32> {
        std::mem::take(&mut self.restarted)
    }
    pub fn take_completed(&mut self) -> BTreeSet<EntityId> {
        std::mem::take(&mut self.completed)
    }
    pub fn update(&mut self, delta: f32, groups: &[(EntityId, [f32; 3], EffectGroup)]) {
        let live: BTreeSet<_> = groups.iter().map(|(entity, _, _)| *entity).collect();
        let removed: Vec<_> = self
            .states
            .keys()
            .copied()
            .filter(|entity| !live.contains(entity))
            .collect();
        for entity in removed {
            self.remove(entity);
        }
        let delta = if delta.is_finite() {
            delta.clamp(0.0, 1.0)
        } else {
            0.0
        };
        for (entity, position, group) in groups {
            self.ensure(*entity, group);
            let state = self.states.get_mut(entity).expect("ensured group exists");
            if state.state != PlaybackState::Playing || group.time_scale <= 0.0 {
                continue;
            }
            state.time = state.time.min(group.duration);
            let mut remaining = delta * group.time_scale;
            // Limit cue work after a huge time scale while still advancing its clock.
            for _ in 0..64 {
                if state.start_pending {
                    for cue in &group.sound_cues {
                        if cue.delay <= 0.0 {
                            queue(&mut self.events, *entity, play(cue, *position));
                        }
                    }
                    state.start_pending = false;
                }
                let end = (state.time + remaining).min(group.duration);
                for cue in &group.sound_cues {
                    if cue.delay > state.time && cue.delay <= end {
                        queue(&mut self.events, *entity, play(cue, *position));
                    }
                }
                remaining -= end - state.time;
                state.time = end;
                if state.time < group.duration {
                    break;
                }
                if !group.looping {
                    state.state = PlaybackState::Completed;
                    self.completed.insert(*entity);
                    queue(&mut self.events, *entity, EffectAudioCommand::Stop);
                    break;
                }
                state.time = 0.0;
                state.start_pending = true;
                self.restarted.insert(*entity, 0.0);
                queue(&mut self.events, *entity, EffectAudioCommand::Stop);
                if remaining <= 0.0 {
                    break;
                }
            }
            if remaining > 0.0 && state.state == PlaybackState::Playing {
                state.time = (state.time + remaining) % group.duration;
                state.start_pending = false;
            }
            if self.restarted.contains_key(entity) {
                self.restarted.insert(*entity, state.time);
            }
        }
    }
}
fn play(cue: &EffectSoundCue, position: [f32; 3]) -> EffectAudioCommand {
    EffectAudioCommand::Play {
        asset: cue.asset,
        volume: cue.volume,
        pitch: cue.pitch,
        spatial: cue.spatial,
        looping: cue.looping,
        position,
    }
}
