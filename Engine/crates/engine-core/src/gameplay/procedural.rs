//! Two-bone IK uses a pole vector to choose a stable bend plane.
use glam::{DQuat, DVec3};
pub struct TwoBoneSolution {
    pub elbow: DVec3,
    pub tip: DVec3,
    pub root_delta: DQuat,
    pub middle_delta: DQuat,
}
/// # Errors
/// Rejects non-finite points, degenerate bones and invalid bend directions.
pub fn two_bone(
    root: DVec3,
    middle: DVec3,
    tip: DVec3,
    target: DVec3,
    pole: DVec3,
) -> Result<TwoBoneSolution, String> {
    if [root, middle, tip, target, pole]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err("IK values must be finite".into());
    }
    let upper = middle.distance(root);
    let lower = tip.distance(middle);
    if upper < 1e-8 || lower < 1e-8 {
        return Err("IK bones must have positive lengths".into());
    }
    let delta = target - root;
    if delta.length_squared() < 1e-16 {
        return Err("IK target coincides with root".into());
    }
    let direction = delta.normalize();
    let distance = delta
        .length()
        .clamp((upper - lower).abs() + 1e-8, upper + lower - 1e-8);
    let projected = pole - root - direction * (pole - root).dot(direction);
    let bend = if projected.length_squared() > 1e-16 {
        projected.normalize()
    } else {
        let axis = if direction.y.abs() < 0.9 {
            DVec3::Y
        } else {
            DVec3::X
        };
        direction.cross(axis).normalize()
    };
    let along = (upper * upper + distance * distance - lower * lower) / (2.0 * distance);
    let height = (upper * upper - along * along).max(0.0).sqrt();
    let elbow = root + direction * along + bend * height;
    let solved_tip = root + direction * distance;
    let root_delta =
        DQuat::from_rotation_arc((middle - root).normalize(), (elbow - root).normalize());
    let rotated_lower = root_delta * (tip - middle);
    let middle_delta =
        DQuat::from_rotation_arc(rotated_lower.normalize(), (solved_tip - elbow).normalize());
    Ok(TwoBoneSolution {
        elbow,
        tip: solved_tip,
        root_delta,
        middle_delta,
    })
}
