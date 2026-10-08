//! Interpolation math. Stateful damping returns the updated velocity explicitly.
use super::Ease;
use glam::{DQuat, DVec3};

pub fn lerp(from: f64, to: f64, progress: f64, easing: Ease) -> f64 {
    from + (to - from) * easing.sample(progress)
}
pub fn lerp_vec3(from: DVec3, to: DVec3, progress: f64, easing: Ease) -> DVec3 {
    from.lerp(to, easing.sample(progress))
}
pub fn slerp(from: DQuat, to: DQuat, progress: f64, easing: Ease) -> DQuat {
    from.normalize()
        .slerp(to.normalize(), easing.sample(progress))
        .normalize()
}
#[allow(
    clippy::float_cmp,
    reason = "An exactly degenerate interval has zero inverse progress"
)]
pub fn inverse_lerp(from: f64, to: f64, value: f64) -> f64 {
    if from == to {
        0.0
    } else {
        (value - from) / (to - from)
    }
}
pub fn remap(value: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    out_min + (out_max - out_min) * inverse_lerp(in_min, in_max, value)
}
/// Analytic critically damped spring; exact for a constant target over the step.
/// Returns (value, velocity). Non-positive dt leaves both unchanged.
pub fn smooth_damp(
    current: f64,
    target: f64,
    velocity: f64,
    smooth_time: f64,
    dt: f64,
) -> (f64, f64) {
    if !dt.is_finite() || dt <= 0.0 {
        return (current, velocity);
    }
    let omega = 2.0 / smooth_time.max(0.0001);
    let offset = current - target;
    let temp = (velocity + omega * offset) * dt;
    let decay = (-omega * dt).exp();
    (
        target + (offset + temp) * decay,
        (velocity - omega * temp) * decay,
    )
}
