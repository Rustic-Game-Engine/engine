//! Clip evaluation is separate from tweening. Source time is linear by default.
//! Importers can supply joint tracks; the renderer consumes the sampled pose.
use super::{Ease, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: f64,
    pub value: Value,
    #[serde(default)]
    pub easing: Ease,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub interpolation: TrackInterpolation,
    pub target: String,
    pub keys: Vec<Keyframe>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackInterpolation {
    #[default]
    Linear,
    Step,
    CubicSpline {
        incoming: Vec<Value>,
        outgoing: Vec<Value>,
    },
}
#[allow(
    clippy::many_single_char_names,
    reason = "Hermite basis and component equations use conventional mathematical names"
)]
fn hermite(a: Value, b: Value, out: Value, input: Value, t: f64, dt: f64) -> Result<Value, String> {
    let h00 = 2.0 * t.powi(3) - 3.0 * t * t + 1.0;
    let h10 = t.powi(3) - 2.0 * t * t + t;
    let h01 = -2.0 * t.powi(3) + 3.0 * t * t;
    let h11 = t.powi(3) - t * t;
    let sample = |a: f64, b: f64, o: f64, i: f64| a * h00 + b * h01 + o * dt * h10 + i * dt * h11;
    let result = match (a, b, out, input) {
        (Value::Number(a), Value::Number(b), Value::Number(o), Value::Number(i)) => {
            Value::Number(sample(a, b, o, i))
        }
        (Value::Vector(a), Value::Vector(b), Value::Vector(o), Value::Vector(i)) => {
            Value::Vector(std::array::from_fn(|k| sample(a[k], b[k], o[k], i[k])))
        }
        (Value::Rotation(a), Value::Rotation(b), Value::Rotation(o), Value::Rotation(i)) => {
            let q =
                glam::DQuat::from_array(std::array::from_fn(|k| sample(a[k], b[k], o[k], i[k])));
            if q.length_squared() < 1e-20 {
                return Err("cubic quaternion collapsed".into());
            }
            Value::Rotation(q.normalize().to_array())
        }
        _ => return Err("cubic tangent types differ from key values".into()),
    };
    result.validate()?;
    Ok(result)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub time: f64,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub duration: f64,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}
/// A script-facing asset handle or a small engine-created keyframe clip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClipReference {
    Named(String),
    Inline(Clip),
}
impl ClipReference {
    /// # Errors
    /// Rejects empty/oversized asset names or invalid inline clips.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Named(name) if name.is_empty() || name.len() > 256 => {
                Err("invalid clip name".into())
            }
            Self::Named(_) => Ok(()),
            Self::Inline(clip) => clip.validate(),
        }
    }
}
impl Clip {
    /// Add a marker before scheduling a clip. Running actions retain their clip snapshot.
    /// # Errors
    /// Rejects invalid names/times or exhausted marker capacity.
    pub fn add_marker(&mut self, time: f64, name: String) -> Result<(), String> {
        if !time.is_finite()
            || time < 0.0
            || time > self.duration
            || name.is_empty()
            || name.len() > 256
            || self.markers.len() >= 4096
        {
            return Err("invalid animation marker or marker capacity exceeded".into());
        }
        self.markers.push(Marker { time, name });
        Ok(())
    }
    /// # Errors
    /// Rejects invalid duration, keys, value types, or marker times.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.len() > 256
            || self.tracks.len() > 4096
            || self.markers.len() > 4096
            || self.tracks.iter().map(|t| t.keys.len()).sum::<usize>() > 1_000_000
        {
            return Err("animation clip exceeds name/track/key/marker limits".into());
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return Err("clip duration must be positive".into());
        }
        let mut targets = std::collections::BTreeSet::new();
        for track in &self.tracks {
            if track.target.is_empty() || track.target.len() > 128 || !targets.insert(&track.target)
            {
                return Err("invalid or duplicate animation track target".into());
            }
            if track.keys.is_empty() {
                return Err("empty animation track".into());
            }
            if let TrackInterpolation::CubicSpline { incoming, outgoing } = &track.interpolation {
                if incoming.len() != track.keys.len() || outgoing.len() != track.keys.len() {
                    return Err("cubic tangent/key counts differ".into());
                }
                for tangent in incoming.iter().chain(outgoing) {
                    let finite = match tangent {
                        Value::Number(v) => v.is_finite(),
                        Value::Vector(v) => v.iter().all(|v| v.is_finite()),
                        Value::Rotation(v) => v.iter().all(|v| v.is_finite()),
                    };
                    if !finite
                        || std::mem::discriminant(tangent)
                            != std::mem::discriminant(&track.keys[0].value)
                    {
                        return Err("cubic tangent is non-finite or has the wrong type".into());
                    }
                }
            }
            let mut previous = -1.0;
            for key in &track.keys {
                if !key.time.is_finite()
                    || key.time < 0.0
                    || key.time > self.duration
                    || key.time <= previous
                {
                    return Err("key times must be strictly increasing inside the clip".into());
                }
                key.value.validate()?;
                if let Some(first) = track.keys.first() {
                    first.value.distance(key.value)?;
                }
                previous = key.time;
            }
        }
        for marker in &self.markers {
            if marker.name.is_empty()
                || marker.name.len() > 256
                || !marker.time.is_finite()
                || marker.time < 0.0
                || marker.time > self.duration
            {
                return Err("marker outside clip".into());
            }
        }
        Ok(())
    }
    /// # Errors
    /// Rejects empty tracks or incompatible values. Validate the clip before sampling.
    pub fn sample(&self, time: f64) -> Result<BTreeMap<String, Value>, String> {
        if !time.is_finite() {
            return Err("animation sample time must be finite".into());
        }
        let mut pose = BTreeMap::new();
        for track in &self.tracks {
            let index = track.keys.partition_point(|k| k.time <= time);
            let value = if index == 0 {
                track.keys.first().ok_or("empty track")?.value
            } else if index >= track.keys.len() {
                track.keys.last().ok_or("empty track")?.value
            } else {
                let a = &track.keys[index - 1];
                let b = &track.keys[index];
                let t = (time - a.time) / (b.time - a.time);
                match &track.interpolation {
                    TrackInterpolation::Linear => {
                        a.value.interpolate(b.value, a.easing.sample(t))?
                    }
                    TrackInterpolation::Step => a.value,
                    TrackInterpolation::CubicSpline { incoming, outgoing } => hermite(
                        a.value,
                        b.value,
                        *outgoing.get(index - 1).ok_or("missing outgoing tangent")?,
                        *incoming.get(index).ok_or("missing incoming tangent")?,
                        t,
                        b.time - a.time,
                    )?,
                }
            };
            pose.insert(track.target.clone(), value);
        }
        Ok(pose)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationOptions {
    pub speed: f64,
    pub looping: bool,
    pub blend_in: f64,
    pub blend_in_ease: Ease,
    pub blend_out: f64,
    pub blend_out_ease: Ease,
    pub weight: f64,
    pub additive: bool,
    /// Blend a masked layer against the pose written by earlier actions this frame.
    pub layered: bool,
    /// Explicit opt-in to warping clip progression. None preserves source timing.
    pub progression_ease: Option<Ease>,
}
impl Default for AnimationOptions {
    fn default() -> Self {
        Self {
            speed: 1.0,
            looping: false,
            blend_in: 0.0,
            blend_in_ease: Ease::Linear,
            blend_out: 0.0,
            blend_out_ease: Ease::Linear,
            weight: 1.0,
            additive: false,
            layered: false,
            progression_ease: None,
        }
    }
}
pub struct ClipPlayer {
    pub(crate) options: AnimationOptions,
    pub(crate) time: f64,
    elapsed: f64,
    pub paused: bool,
    pub finished: bool,
}
#[derive(Clone, Debug)]
pub struct AnimationFrame {
    pub pose: BTreeMap<String, Value>,
    pub weight: f64,
    pub markers: Vec<String>,
    pub finished: bool,
}
impl ClipPlayer {
    /// # Errors
    /// Rejects non-positive speed or negative blend duration.
    pub fn new(options: AnimationOptions) -> Result<Self, String> {
        if !options.speed.is_finite()
            || options.speed <= 0.0
            || !options.blend_in.is_finite()
            || options.blend_in < 0.0
            || !options.blend_out.is_finite()
            || options.blend_out < 0.0
            || !options.weight.is_finite()
            || options.weight < 0.0
        {
            return Err("invalid animation speed/blend duration".into());
        }
        Ok(Self {
            options,
            time: 0.0,
            elapsed: 0.0,
            paused: false,
            finished: false,
        })
    }
    pub fn options(&self) -> AnimationOptions {
        self.options
    }
    /// # Errors
    /// Rejects non-positive or non-finite playback speed.
    pub fn set_speed(&mut self, speed: f64) -> Result<(), String> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err("invalid animation speed".into());
        }
        self.options.speed = speed;
        Ok(())
    }
    pub fn stop(&mut self) {
        self.finished = true;
    }
    /// # Errors
    /// Rejects invalid clips, invalid delta, time overflow, or excessive marker crossings.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Loop count is checked finite and bounded to 4096 before conversion"
    )]
    pub fn tick(&mut self, clip: &Clip, dt: f64) -> Result<AnimationFrame, String> {
        clip.validate()?;
        self.tick_validated(clip, dt)
    }
    /// Engine scheduler path: the immutable action clip was validated when scheduled.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Loop counts are finite, nonnegative and bounded to 4096 before conversion"
    )]
    pub(crate) fn tick_validated(
        &mut self,
        clip: &Clip,
        dt: f64,
    ) -> Result<AnimationFrame, String> {
        if !dt.is_finite() || dt < 0.0 {
            return Err("invalid animation delta".into());
        }
        let mut markers = Vec::new();
        if !self.paused && !self.finished {
            let previous = self.time;
            let advanced = dt * self.options.speed;
            if !advanced.is_finite() {
                return Err("animation time overflowed".into());
            }
            let next = previous + advanced;
            let loops = (next / clip.duration).floor();
            if !loops.is_finite() || loops > 4096.0 {
                return Err("animation marker loop budget exceeded".into());
            }
            let loops = loops as u32;
            let mut crossings = Vec::new();
            for marker in &clip.markers {
                // Markers at zero fire on the initial step and at each loop boundary.
                for cycle in 0..=if self.options.looping { loops } else { 0 } {
                    let crossing = marker.time + f64::from(cycle) * clip.duration;
                    if (crossing > previous || (crossing == 0.0 && self.elapsed == 0.0))
                        && crossing <= next
                    {
                        if crossings.len() >= 4096 {
                            return Err("animation marker delivery budget exceeded".into());
                        }
                        crossings.push((crossing, marker.name.clone()));
                    }
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            markers = crossings.into_iter().map(|(_, name)| name).collect();
            self.elapsed += dt;
            self.time = if self.options.looping {
                next.rem_euclid(clip.duration)
            } else {
                next.min(clip.duration)
            };
            self.finished = !self.options.looping && next >= clip.duration;
        }
        let time = self.options.progression_ease.map_or(self.time, |ease| {
            ease.sample(self.time / clip.duration) * clip.duration
        });
        let weight = if self.options.blend_in == 0.0 {
            1.0
        } else {
            self.options
                .blend_in_ease
                .sample(self.elapsed / self.options.blend_in)
        };
        let out_weight = if self.options.blend_out > 0.0 && !self.options.looping {
            1.0 - self.options.blend_out_ease.sample(
                1.0 - (clip.duration - self.time) / self.options.speed / self.options.blend_out,
            )
        } else {
            1.0
        };
        Ok(AnimationFrame {
            pose: clip.sample(time)?,
            weight: weight * out_weight * self.options.weight,
            markers,
            finished: self.finished,
        })
    }
}
/// Blends skeletal or procedural poses without changing either clip's clock.
/// # Errors
/// Rejects incompatible value types for a shared track.
pub fn blend(
    from: &BTreeMap<String, Value>,
    to: &BTreeMap<String, Value>,
    progress: f64,
    easing: Ease,
) -> Result<BTreeMap<String, Value>, String> {
    let weight = easing.sample(progress);
    let mut result = from.clone();
    for (joint, value) in to {
        result.insert(
            joint.clone(),
            from.get(joint)
                .map_or(Ok(*value), |a| a.interpolate(*value, weight))?,
        );
    }
    Ok(result)
}

/// Apply a relative pose against a reference clip pose, including quaternion composition.
/// # Errors
/// Rejects incompatible pose types or non-finite output.
pub fn additive(base: Value, pose: Value, reference: Value, weight: f64) -> Result<Value, String> {
    let result = match (base, pose, reference) {
        (Value::Number(b), Value::Number(p), Value::Number(r)) => {
            Value::Number(b + (p - r) * weight)
        }
        (Value::Vector(b), Value::Vector(p), Value::Vector(r)) => {
            Value::Vector(std::array::from_fn(|i| b[i] + (p[i] - r[i]) * weight))
        }
        (Value::Rotation(b), Value::Rotation(p), Value::Rotation(r)) => {
            let delta = glam::DQuat::from_array(r).normalize().inverse()
                * glam::DQuat::from_array(p).normalize();
            Value::Rotation(
                (glam::DQuat::from_array(b).normalize()
                    * glam::DQuat::IDENTITY.slerp(delta, weight))
                .normalize()
                .to_array(),
            )
        }
        _ => return Err("additive pose types differ".into()),
    };
    result.validate()?;
    Ok(result)
}
