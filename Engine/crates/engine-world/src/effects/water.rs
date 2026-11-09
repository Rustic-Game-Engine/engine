//! Water-surface interface shared by procedural water and imported mesh surfaces.
use super::RingEffect;
use glam::{Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterSample {
    pub displacement: f32,
    pub normal: Vec3,
    pub foam: f32,
}
impl Default for WaterSample {
    fn default() -> Self {
        Self {
            displacement: 0.0,
            normal: Vec3::Y,
            foam: 0.0,
        }
    }
}
/// A compatible water surface samples disturbances in its world XZ plane.
pub trait WaterSurface {
    fn sample(&self, world_position: Vec3) -> WaterSample;
}
#[derive(Clone, Debug)]
pub struct WaterDisturbance {
    pub source: Vec3,
    pub elapsed: f32,
    pub duration: f32,
    pub settings: RingEffect,
}
impl WaterDisturbance {
    pub fn sample(&self, world_position: Vec3) -> WaterSample {
        let p = &self.settings;
        let elapsed = self.elapsed.max(0.0);
        if elapsed > self.duration {
            return WaterSample::default();
        }
        let speed = p.expansion_speed.map_or(
            (p.final_radius - p.initial_radius) / self.duration.max(0.001),
            |speed| speed * (p.final_radius - p.initial_radius).signum(),
        );
        let relative = world_position - self.source;
        let radial = Vec2::new(relative.x, relative.z);
        let distance = radial.length();
        let radius = (p.initial_radius + speed * elapsed).clamp(
            p.initial_radius.min(p.final_radius),
            p.initial_radius.max(p.final_radius),
        );
        let band = ((distance - radius) / p.thickness.max(0.01)).abs();
        let envelope = (-band * band * 2.0).exp() * (-elapsed * p.damping.max(0.0)).exp();
        let k = std::f32::consts::TAU / p.wavelength.max(0.01);
        let phase = k * (distance - radius);
        let mut height = phase.sin() * p.amplitude * envelope;
        let velocity = Vec2::new(p.source_velocity[0], p.source_velocity[2]);
        let direction = velocity.normalize_or_zero();
        let along = -radial.dot(direction);
        let sideways = radial.perp_dot(direction).abs();
        let wake_width = (along * p.wake_angle.tan().abs()).max(p.thickness);
        let wake = if velocity.length_squared() > 0.001 && along > 0.0 {
            (-((sideways - wake_width) / p.thickness.max(0.01)).powi(2) * 2.0).exp()
                * (-along / p.final_radius.max(0.1)).exp()
                * (-elapsed * p.damping).exp()
        } else {
            0.0
        };
        height += wake * p.amplitude;
        let slope = phase.cos() * k * p.amplitude * envelope * p.normal_strength;
        let normal = Vec3::new(
            -radial.x / distance.max(0.001) * slope,
            1.0,
            -radial.y / distance.max(0.001) * slope,
        )
        .normalize();
        let foam =
            ((envelope + wake) * p.foam_intensity * (-elapsed / p.foam_lifetime.max(0.001)).exp())
                .clamp(0.0, 1.0);
        WaterSample {
            displacement: height,
            normal,
            foam,
        }
    }
}
/// Bounded disturbance collection suitable for persistent water components.
#[derive(Clone, Debug)]
pub struct RippleWaterSurface {
    pub disturbances: std::collections::VecDeque<WaterDisturbance>,
    pub max_disturbances: usize,
}
impl Default for RippleWaterSurface {
    fn default() -> Self {
        Self {
            disturbances: std::collections::VecDeque::default(),
            max_disturbances: 32,
        }
    }
}
impl RippleWaterSurface {
    pub fn disturb(&mut self, disturbance: WaterDisturbance) {
        let limit = self.max_disturbances.clamp(1, 256);
        while self.disturbances.len() >= limit {
            self.disturbances.pop_front();
        }
        self.disturbances.push_back(disturbance);
    }
    pub fn update(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        for d in &mut self.disturbances {
            d.elapsed += dt;
        }
        self.disturbances.retain(|d| d.elapsed <= d.duration);
    }
}
impl WaterSurface for RippleWaterSurface {
    fn sample(&self, position: Vec3) -> WaterSample {
        let mut result = WaterSample::default();
        let mut slope = Vec3::ZERO;
        for d in &self.disturbances {
            let s = d.sample(position);
            result.displacement += s.displacement;
            slope += Vec3::new(
                s.normal.x / s.normal.y.max(0.001),
                0.0,
                s.normal.z / s.normal.y.max(0.001),
            );
            result.foam = 1.0 - (1.0 - result.foam) * (1.0 - s.foam);
        }
        result.normal = Vec3::new(slope.x, 1.0, slope.z).normalize();
        result
    }
}
/// Produces a reusable displaced water grid with per-vertex foam coverage.
pub fn water_surface_mesh(
    surface: &impl WaterSurface,
    center: Vec3,
    size: f32,
    subdivisions: u16,
) -> (crate::PrimitiveMesh, Vec<f32>) {
    let n = subdivisions.clamp(2, 128);
    let mut mesh = crate::PrimitiveMesh::default();
    let mut foam = Vec::with_capacity(usize::from(n + 1).pow(2));
    for z in 0..=n {
        for x in 0..=n {
            let uv = [f32::from(x) / f32::from(n), f32::from(z) / f32::from(n)];
            let mut pos = center + Vec3::new((uv[0] - 0.5) * size, 0.0, (uv[1] - 0.5) * size);
            let s = surface.sample(pos);
            pos.y += s.displacement;
            mesh.positions.push(pos.to_array());
            mesh.normals.push(s.normal.to_array());
            mesh.tex_coords.push(uv);
            foam.push(s.foam);
            if x < n && z < n {
                let a = u32::from(z) * u32::from(n + 1) + u32::from(x);
                let b = a + u32::from(n + 1);
                mesh.indices
                    .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    (mesh, foam)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disturbance_displaces_and_expires() {
        let mut water = RippleWaterSurface::default();
        water.disturb(WaterDisturbance {
            source: Vec3::ZERO,
            elapsed: 0.2,
            duration: 2.0,
            settings: RingEffect::default(),
        });
        let s = water.sample(Vec3::new(0.55, 0.0, 0.0));
        assert!(s.displacement.abs() > 0.0001);
        assert!((s.normal.length() - 1.0).abs() < 1e-5);
        water.update(3.0);
        assert_eq!(water.sample(Vec3::ZERO), WaterSample::default());
    }
    #[test]
    fn water_grid_has_displaced_normals_and_bounded_storage() {
        let mut water = RippleWaterSurface {
            max_disturbances: 2,
            ..Default::default()
        };
        for _ in 0..5 {
            water.disturb(WaterDisturbance {
                source: Vec3::ZERO,
                elapsed: 0.2,
                duration: 2.0,
                settings: RingEffect::default(),
            });
        }
        assert_eq!(water.disturbances.len(), 2);
        let (mesh, foam) = water_surface_mesh(&water, Vec3::ZERO, 4.0, 16);
        assert_eq!(mesh.positions.len(), 289);
        assert_eq!(foam.len(), 289);
        assert!(mesh.positions.iter().any(|p| p[1].abs() > 0.001));
        assert_eq!(mesh.indices.len(), 16 * 16 * 6);
    }
}
