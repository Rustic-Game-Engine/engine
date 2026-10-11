//! Backend-independent PCM mixing. The device backend consumes stereo 48 kHz frames.
use super::Owner;
use crate::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioClip {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}
impl AudioClip {
    /// # Errors
    /// Rejects invalid channel/rate counts, incomplete frames or non-finite/unbounded PCM.
    pub fn validate(&self) -> Result<(), String> {
        if self.channels == 0
            || self.channels > 8
            || self.sample_rate == 0
            || self.samples.is_empty()
            || !self
                .samples
                .len()
                .is_multiple_of(usize::from(self.channels))
            || self.samples.iter().any(|v| !v.is_finite() || v.abs() > 1.0)
        {
            return Err("invalid PCM clip".into());
        }
        Ok(())
    }
}
pub struct Voice {
    pub owner: Owner,
    pub volume: f64,
    pub pitch: f64,
    pub paused: bool,
    pub looping: bool,
    pub position: Option<[f64; 3]>,
    clip: Arc<AudioClip>,
    cursor: f64,
}
struct LoadedAudio {
    owner: Owner,
    clip: Arc<AudioClip>,
    voice: Option<EntityId>,
}
#[derive(Default)]
pub struct Mixer {
    pub voices: BTreeMap<EntityId, Voice>,
    loaded: BTreeMap<EntityId, LoadedAudio>,
    pub listener: [f64; 3],
    fraction: f64,
}
impl Mixer {
    /// Decode-independent storage for audio that can be replayed until explicitly unloaded.
    /// # Errors
    /// Rejects invalid PCM or exhausted loaded-audio capacity.
    pub fn load(&mut self, owner: Owner, clip: Arc<AudioClip>) -> Result<EntityId, String> {
        clip.validate()?;
        if self.loaded.len() >= 1024 {
            return Err("loaded audio limit exceeded".into());
        }
        let id = EntityId::new();
        self.loaded.insert(
            id,
            LoadedAudio {
                owner,
                clip,
                voice: None,
            },
        );
        Ok(id)
    }

    /// Start a loaded clip from the beginning, replacing its previous playback.
    /// # Errors
    /// Rejects unloaded/foreign handles or invalid playback options.
    pub fn play_loaded(
        &mut self,
        owner: Owner,
        id: EntityId,
        volume: f64,
        pitch: f64,
        looping: bool,
        position: Option<[f64; 3]>,
    ) -> Result<EntityId, String> {
        let loaded = self
            .loaded
            .get(&id)
            .filter(|a| a.owner == owner)
            .ok_or("audio is not loaded; call :load() before :play()")?;
        let old = loaded.voice;
        let clip = Arc::clone(&loaded.clip);
        let previous = old.and_then(|id| self.voices.remove(&id));
        let voice = match self.play(owner, clip, volume, pitch, looping, position) {
            Ok(voice) => voice,
            Err(error) => {
                if let Some(previous) = previous
                    && let Some(old) = old
                {
                    self.voices.insert(old, previous);
                }
                return Err(error);
            }
        };
        self.loaded
            .get_mut(&id)
            .ok_or("audio is no longer loaded")?
            .voice = Some(voice);
        Ok(voice)
    }

    /// Control a loaded clip's current playback, retaining PCM for replay.
    /// # Errors
    /// Rejects unloaded/foreign handles or unknown operations.
    pub fn control_loaded(
        &mut self,
        owner: Owner,
        id: EntityId,
        operation: &str,
    ) -> Result<(), String> {
        let loaded = self
            .loaded
            .get_mut(&id)
            .filter(|a| a.owner == owner)
            .ok_or("audio is not loaded; call :load() first")?;
        match operation {
            "stop" => {
                if let Some(voice) = loaded.voice.take() {
                    self.voices.remove(&voice);
                }
            }
            "pause" | "resume" => {
                if let Some(voice) = loaded.voice.and_then(|id| self.voices.get_mut(&id)) {
                    voice.paused = operation == "pause";
                }
            }
            _ => return Err("unknown loaded audio operation".into()),
        }
        Ok(())
    }

    /// Stop playback and release retained PCM. Repeated unloads are harmless.
    /// # Errors
    /// Rejects another owner's handle.
    pub fn unload(&mut self, owner: Owner, id: EntityId) -> Result<(), String> {
        if self.loaded.get(&id).is_some_and(|a| a.owner != owner) {
            return Err("audio handle belongs to another owner".into());
        }
        if let Some(loaded) = self.loaded.remove(&id)
            && let Some(voice) = loaded.voice
        {
            self.voices.remove(&voice);
        }
        Ok(())
    }
    /// # Errors
    /// Rejects invalid PCM, volume/pitch/position, or exhausted voice capacity.
    pub fn play(
        &mut self,
        owner: Owner,
        clip: Arc<AudioClip>,
        volume: f64,
        pitch: f64,
        looping: bool,
        position: Option<[f64; 3]>,
    ) -> Result<EntityId, String> {
        clip.validate()?;
        if !volume.is_finite()
            || volume < 0.0
            || volume > f64::from(f32::MAX)
            || !pitch.is_finite()
            || pitch <= 0.0
            || position.is_some_and(|v| v.iter().any(|n| !n.is_finite()))
        {
            return Err("invalid audio options".into());
        }
        if self.voices.len() >= 1024 {
            return Err("audio voice limit exceeded".into());
        }
        let id = EntityId::new();
        self.voices.insert(
            id,
            Voice {
                owner,
                volume,
                pitch,
                paused: false,
                looping,
                position,
                clip,
                cursor: 0.0,
            },
        );
        Ok(id)
    }
    pub fn cleanup(&mut self, predicate: impl Fn(Owner) -> bool) {
        self.voices.retain(|_, v| !predicate(v.owner));
        self.loaded.retain(|_, a| !predicate(a.owner));
    }
    /// # Errors
    /// Rejects invalid frame deltas or overflowing playback steps.
    pub fn mix(&mut self, delta: f64) -> Result<Vec<f32>, String> {
        self.mix_scaled(delta, 1.0)
    }
    /// Render wall-clock frames while scaling the source clock and respecting scene pause.
    /// # Errors
    /// Rejects invalid deltas/scales and overflowing source steps.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn mix_scaled(&mut self, delta: f64, scale: f64) -> Result<Vec<f32>, String> {
        if !scale.is_finite()
            || scale < 0.0
            || self
                .voices
                .values()
                .any(|v| !(f64::from(v.clip.sample_rate) / 48000.0 * v.pitch * scale).is_finite())
        {
            return Err("invalid or overflowing audio time scale".into());
        }
        if !delta.is_finite() || delta < 0.0 {
            return Err("audio frame delta must be finite and nonnegative".into());
        }
        // Keep the output queue bounded after a long frame while preserving source time.
        let skipped = (delta - 1.0).max(0.0);
        if self.voices.values().any(|voice| {
            !voice.paused
                && !(voice.cursor
                    + skipped * f64::from(voice.clip.sample_rate) * voice.pitch * scale)
                    .is_finite()
        }) {
            return Err("audio source time overflowed".into());
        }
        let frames = delta.min(1.0) * 48000.0 + self.fraction;
        let count = frames.floor() as usize;
        self.fraction = frames - count as f64;
        let mut output = vec![0.0_f64; count * 2];
        self.voices.retain(|_, voice| {
            if voice.paused || scale == 0.0 {
                return true;
            }
            let channels = usize::from(voice.clip.channels);
            let length = voice.clip.samples.len() / channels;
            voice.cursor += skipped * f64::from(voice.clip.sample_rate) * voice.pitch * scale;
            let spatial = voice.position.map_or(1.0, |p| {
                1.0 / (1.0
                    + glam::DVec3::from_array(p).distance(glam::DVec3::from_array(self.listener)))
            });
            for frame in 0..count {
                if voice.cursor >= length as f64 {
                    if voice.looping {
                        voice.cursor = voice.cursor.rem_euclid(length as f64);
                    } else {
                        return false;
                    }
                }
                let index = voice.cursor.floor() as usize;
                let next = if index + 1 < length {
                    index + 1
                } else if voice.looping {
                    0
                } else {
                    index
                };
                let t = (voice.cursor - index as f64) as f32;
                for channel in 0..2 {
                    let c = channel.min(channels - 1);
                    let a = voice.clip.samples[index * channels + c];
                    let b = voice.clip.samples[next * channels + c];
                    output[frame * 2 + channel] +=
                        f64::from(a + (b - a) * t) * voice.volume * spatial;
                }
                voice.cursor += f64::from(voice.clip.sample_rate) / 48000.0 * voice.pitch * scale;
            }
            voice.looping || voice.cursor < length as f64
        });
        Ok(output
            .into_iter()
            .map(|sample| sample.clamp(-1.0, 1.0) as f32)
            .collect())
    }
}
