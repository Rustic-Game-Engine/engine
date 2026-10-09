//! Deterministic queries against the runtime's world-space collision bounds.
use crate::EntityId;
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collider {
    pub entity: EntityId,
    pub center: [f64; 3],
    pub half: [f64; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub entity: EntityId,
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub distance: f64,
}
/// # Errors
/// Rejects invalid extents, origins, directions and distances.
pub fn cast(
    colliders: &[Collider],
    origin: [f64; 3],
    direction: [f64; 3],
    distance: f64,
    radius: f64,
    ignore: &[EntityId],
) -> Result<Option<Hit>, String> {
    let origin = DVec3::from_array(origin);
    let direction = DVec3::from_array(direction);
    if !origin.is_finite()
        || !direction.is_finite()
        || direction.length_squared() < 1e-20
        || !distance.is_finite()
        || distance < 0.0
        || !radius.is_finite()
        || radius < 0.0
    {
        return Err("invalid cast origin, direction, distance or radius".into());
    }
    let direction = direction
        .try_normalize()
        .ok_or("cast direction cannot be normalized")?;
    let mut best: Option<Hit> = None;
    for c in colliders {
        if ignore.contains(&c.entity) {
            continue;
        }
        let local = origin - DVec3::from_array(c.center);
        let half = DVec3::from_array(c.half) + DVec3::splat(radius);
        let mut near = 0.0_f64;
        let mut far = distance;
        let mut normal = -direction;
        for axis in 0..3 {
            if direction[axis].abs() < 1e-12 {
                if local[axis].abs() > half[axis] {
                    far = -1.0;
                    break;
                }
                continue;
            }
            let a = (-half[axis] - local[axis]) / direction[axis];
            let b = (half[axis] - local[axis]) / direction[axis];
            if a.min(b) > near {
                near = a.min(b);
                normal = DVec3::ZERO;
                normal[axis] = -direction[axis].signum();
            }
            far = far.min(a.max(b));
        }
        if radius > 0.0 && near <= far {
            if let Some((time, n)) = sphere_box(
                local,
                direction,
                DVec3::from_array(c.half),
                radius,
                distance,
            ) {
                near = time;
                normal = n;
            } else {
                continue;
            }
        }
        if near <= far && near <= distance && best.as_ref().is_none_or(|h| near < h.distance) {
            best = Some(Hit {
                entity: c.entity,
                point: (origin + direction * near - normal * radius).to_array(),
                normal: normal.to_array(),
                distance: near,
            });
        }
    }
    Ok(best)
}
/// # Errors
/// Rejects invalid centers, radii and collider bounds.
pub fn overlap(
    colliders: &[Collider],
    center: [f64; 3],
    radius: f64,
    ignore: &[EntityId],
) -> Result<Vec<EntityId>, String> {
    let center = DVec3::from_array(center);
    if !center.is_finite() || !radius.is_finite() || radius < 0.0 {
        return Err("invalid overlap sphere".into());
    }
    Ok(colliders
        .iter()
        .filter(|c| {
            !ignore.contains(&c.entity) && {
                let delta =
                    (center - DVec3::from_array(c.center)).abs() - DVec3::from_array(c.half);
                delta.max(DVec3::ZERO).length_squared() <= radius * radius
            }
        })
        .map(|c| c.entity)
        .collect())
}

// A sphere swept against a box has rounded corners. Solve each piece of the
// squared point-to-box distance exactly rather than enlarging to a bigger box.
fn sphere_box(
    origin: DVec3,
    direction: DVec3,
    half: DVec3,
    radius: f64,
    distance: f64,
) -> Option<(f64, DVec3)> {
    let mut cuts = vec![0.0, distance];
    for axis in 0..3 {
        if direction[axis].abs() > 1e-12 {
            for bound in [-half[axis], half[axis]] {
                let t = (bound - origin[axis]) / direction[axis];
                if t > 0.0 && t < distance {
                    cuts.push(t);
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let contact = |time: f64| {
        let point = origin + direction * time;
        let delta = point - point.clamp(-half, half);
        (time, delta.try_normalize().unwrap_or(-direction))
    };
    for interval in cuts.windows(2) {
        let start = interval[0];
        let end = interval[1];
        let point = origin + direction * start;
        let delta = point - point.clamp(-half, half);
        if delta.length_squared() <= radius * radius {
            return Some(contact(start));
        }
        let middle = origin + direction * start.midpoint(end);
        let mut a = 0.0;
        let mut b = 0.0;
        let mut c = -radius * radius;
        for axis in 0..3 {
            let bound = if middle[axis] < -half[axis] {
                -half[axis]
            } else if middle[axis] > half[axis] {
                half[axis]
            } else {
                continue;
            };
            let offset = origin[axis] - bound;
            a += direction[axis] * direction[axis];
            b += 2.0 * direction[axis] * offset;
            c += offset * offset;
        }
        let discriminant = b * b - 4.0 * a * c;
        if a > 1e-24 && discriminant >= 0.0 {
            let t = (-b - discriminant.sqrt()) / (2.0 * a);
            if t >= start - 1e-10 && t <= end + 1e-10 {
                return Some(contact(t.clamp(start, end)));
            }
        }
    }
    let point = origin + direction * distance;
    if (point - point.clamp(-half, half)).length_squared() <= radius * radius {
        Some(contact(distance))
    } else {
        None
    }
}
