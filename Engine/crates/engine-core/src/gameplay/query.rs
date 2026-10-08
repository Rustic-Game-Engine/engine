//! Synchronous, language-neutral queries. Long-running work remains in the scheduler.
use super::{Ease, Value, smooth};
use glam::DQuat;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Query {
    Keyframes {
        keys: Vec<super::animation::Keyframe>,
        token: String,
    },
    Lerp {
        from: Value,
        to: Value,
        progress: f64,
        #[serde(default)]
        easing: Ease,
    },
    Slerp {
        from: [f64; 4],
        to: [f64; 4],
        progress: f64,
        #[serde(default)]
        easing: Ease,
    },
    InverseLerp {
        from: f64,
        to: f64,
        value: f64,
    },
    Remap {
        value: f64,
        in_min: f64,
        in_max: f64,
        out_min: f64,
        out_max: f64,
    },
    SmoothDamp {
        current: f64,
        target: f64,
        velocity: f64,
        smooth_time: f64,
        delta: f64,
    },
    Ease {
        progress: f64,
        #[serde(default)]
        easing: Ease,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueryResult {
    Action(Box<super::Action>),
    Value(Value),
    Damping { value: f64, velocity: f64 },
}
impl Query {
    /// # Errors
    /// Rejects non-finite values, incompatible types, invalid intervals or invalid keyframes.
    pub fn evaluate(&self) -> Result<QueryResult, String> {
        let finite = |values: &[f64]| {
            if values.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err("query values must be finite".to_owned())
            }
        };
        let value = match *self {
            Self::Keyframes {
                ref keys,
                ref token,
            } => return keyframe_action(keys, token),
            Self::Lerp {
                from,
                to,
                progress,
                easing,
            } => {
                finite(&[progress])?;
                from.validate()?;
                to.validate()?;
                from.distance(to)?;
                from.interpolate(to, easing.sample(progress))?
            }
            Self::Slerp {
                from,
                to,
                progress,
                easing,
            } => {
                finite(&[progress])?;
                Value::Rotation(from).validate()?;
                Value::Rotation(to).validate()?;
                Value::Rotation(
                    smooth::slerp(
                        DQuat::from_array(from),
                        DQuat::from_array(to),
                        progress,
                        easing,
                    )
                    .to_array(),
                )
            }
            Self::InverseLerp { from, to, value } => {
                finite(&[from, to, value])?;
                Value::Number(smooth::inverse_lerp(from, to, value))
            }
            Self::Remap {
                value,
                in_min,
                in_max,
                out_min,
                out_max,
            } => {
                finite(&[value, in_min, in_max, out_min, out_max])?;
                Value::Number(smooth::remap(value, in_min, in_max, out_min, out_max))
            }
            Self::SmoothDamp {
                current,
                target,
                velocity,
                smooth_time,
                delta,
            } => {
                finite(&[current, target, velocity, smooth_time, delta])?;
                if smooth_time <= 0.0 || delta < 0.0 {
                    return Err("smooth time must be positive and delta non-negative".into());
                }
                let (value, velocity) =
                    smooth::smooth_damp(current, target, velocity, smooth_time, delta);
                finite(&[value, velocity])?;
                return Ok(QueryResult::Damping { value, velocity });
            }
            Self::Ease { progress, easing } => {
                finite(&[progress])?;
                Value::Number(easing.sample(progress))
            }
        };
        value.validate()?;
        Ok(QueryResult::Value(value))
    }
}

fn keyframe_action(
    keys: &[super::animation::Keyframe],
    token: &str,
) -> Result<QueryResult, String> {
    if keys.len() < 2 || keys.first().is_none_or(|key| key.time != 0.0) {
        return Err("value animation needs at least two keys beginning at zero".into());
    }
    let clip = super::animation::Clip {
        name: "value".into(),
        duration: keys.last().ok_or("missing key")?.time,
        tracks: vec![super::animation::Track {
            target: "value".into(),
            interpolation: super::animation::TrackInterpolation::default(),
            keys: keys.to_vec(),
        }],
        markers: Vec::new(),
    };
    clip.validate()?;
    let action = super::Action::Sequence {
        actions: keys
            .windows(2)
            .map(|pair| super::Action::Value {
                from: pair[0].value,
                to: pair[1].value,
                timing: super::Timing::Duration {
                    duration: pair[1].time - pair[0].time,
                },
                easing: pair[0].easing,
                token: token.to_owned(),
            })
            .collect(),
    };
    action.validate()?;
    Ok(QueryResult::Action(Box::new(action)))
}
