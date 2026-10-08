//! The single curve implementation used by every gameplay system.
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    #[default]
    Linear,
    InSine,
    OutSine,
    InOutSine,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InQuint,
    OutQuint,
    InOutQuint,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    InBack,
    OutBack,
    InOutBack,
    InElastic,
    OutElastic,
    InOutElastic,
    InBounce,
    OutBounce,
    InOutBounce,
}

impl Ease {
    /// Clamps input progress. Back/Elastic intentionally overshoot the output.
    pub fn sample(self, progress: f64) -> f64 {
        let t = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        if t <= 0.0 || t >= 1.0 {
            return t;
        }
        match self {
            Self::Linear => t,
            Self::InSine => 1.0 - (t * PI / 2.0).cos(),
            Self::OutSine => (t * PI / 2.0).sin(),
            Self::InOutSine => (1.0 - (PI * t).cos()) / 2.0,
            Self::InQuad => t * t,
            Self::OutQuad => 1.0 - (1.0 - t).powi(2),
            Self::InOutQuad => in_out(t, |x| x.powi(2)),
            Self::InCubic => t.powi(3),
            Self::OutCubic => 1.0 - (1.0 - t).powi(3),
            Self::InOutCubic => in_out(t, |x| x.powi(3)),
            Self::InQuart => t.powi(4),
            Self::OutQuart => 1.0 - (1.0 - t).powi(4),
            Self::InOutQuart => in_out(t, |x| x.powi(4)),
            Self::InQuint => t.powi(5),
            Self::OutQuint => 1.0 - (1.0 - t).powi(5),
            Self::InOutQuint => in_out(t, |x| x.powi(5)),
            Self::InExpo => 2.0_f64.powf(10.0 * t - 10.0),
            Self::OutExpo => 1.0 - 2.0_f64.powf(-10.0 * t),
            Self::InOutExpo => in_out(t, |x| 2.0_f64.powf(10.0 * x - 10.0)),
            Self::InCirc => 1.0 - (1.0 - t * t).sqrt(),
            Self::OutCirc => (1.0 - (t - 1.0).powi(2)).sqrt(),
            Self::InOutCirc => in_out(t, |x| 1.0 - (1.0 - x * x).sqrt()),
            Self::InBack => back(t),
            Self::OutBack => 1.0 - back(1.0 - t),
            Self::InOutBack => in_out(t, |x| {
                let c = 1.70158 * 1.525;
                (c + 1.0) * x.powi(3) - c * x * x
            }),
            Self::InElastic => elastic(t),
            Self::OutElastic => 1.0 - elastic(1.0 - t),
            Self::InOutElastic => {
                let c = 2.0 * PI / 4.5;
                if t < 0.5 {
                    -(2.0_f64.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c).sin()) / 2.0
                } else {
                    2.0_f64.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c).sin() / 2.0 + 1.0
                }
            }
            Self::InBounce => 1.0 - bounce(1.0 - t),
            Self::OutBounce => bounce(t),
            Self::InOutBounce => in_out(t, |x| 1.0 - bounce(1.0 - x)),
        }
    }
}
fn in_out(t: f64, f: impl Fn(f64) -> f64) -> f64 {
    if t < 0.5 {
        f(t * 2.0) / 2.0
    } else {
        1.0 - f(2.0 - t * 2.0) / 2.0
    }
}
fn back(t: f64) -> f64 {
    2.70158 * t.powi(3) - 1.70158 * t * t
}
fn elastic(t: f64) -> f64 {
    -2.0_f64.powf(10.0 * t - 10.0) * ((10.0 * t - 10.75) * 2.0 * PI / 3.0).sin()
}
fn bounce(t: f64) -> f64 {
    let (x, base) = if t < 1.0 / 2.75 {
        (t, 0.0)
    } else if t < 2.0 / 2.75 {
        (t - 1.5 / 2.75, 0.75)
    } else if t < 2.5 / 2.75 {
        (t - 2.25 / 2.75, 0.9375)
    } else {
        (t - 2.625 / 2.75, 0.984_375)
    };
    7.5625 * x * x + base
}
