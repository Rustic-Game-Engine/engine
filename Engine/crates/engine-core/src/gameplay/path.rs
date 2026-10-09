//! Geometry is sampled independently of traversal easing.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Validated paths bound indices to 4096 points and arc tables to 65536 samples"
)]

use super::Ease;
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PathKind {
    #[default]
    Linear,
    Bezier,
    CubicBezier,
    CatmullRom,
    Spline,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathPoint {
    pub point: [f64; 3],
    #[serde(default)]
    pub easing: Ease,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Path {
    #[serde(default)]
    pub kind: PathKind,
    pub points: Vec<PathPoint>,
}
impl Path {
    /// # Errors
    /// Rejects invalid point counts and non-finite control points.
    pub fn validate(&self) -> Result<(), String> {
        if self.points.len() < 2 || self.points.len() > 4096 {
            return Err("a path needs 2..4096 points".into());
        }
        if self.kind == PathKind::CubicBezier && self.points.len() != 4 {
            return Err("CubicBezier needs four control points".into());
        }
        if self.kind == PathKind::Bezier && self.points.len() > 32 {
            return Err("Bezier supports at most 32 control points".into());
        }
        if self
            .points
            .iter()
            .any(|p| p.point.iter().any(|v| !v.is_finite()))
        {
            return Err("path contains a non-finite point".into());
        }
        Ok(())
    }
    /// Samples normalized geometry; caller validates the path once at creation.
    pub fn sample(&self, progress: f64) -> DVec3 {
        let t = progress.clamp(0.0, 1.0);
        if self.points.is_empty() {
            return DVec3::ZERO;
        }
        if self.points.len() == 1 {
            return DVec3::from_array(self.points[0].point);
        }
        if matches!(self.kind, PathKind::Bezier | PathKind::CubicBezier) {
            let mut work: Vec<_> = self
                .points
                .iter()
                .map(|p| DVec3::from_array(p.point))
                .collect();
            for count in (1..work.len()).rev() {
                for i in 0..count {
                    work[i] = work[i].lerp(work[i + 1], t);
                }
            }
            return work[0];
        }
        let last = self.points.len() - 1;
        let segment = t * last as f64;
        let i = (segment.floor() as usize).min(last - 1);
        let local = segment - i as f64;
        let point = |index: usize| DVec3::from_array(self.points[index].point);
        let p1 = point(i);
        let p2 = point(i + 1);
        if self.kind == PathKind::Linear {
            return p1.lerp(p2, local);
        }
        let p0 = point(i.saturating_sub(1));
        let p3 = point((i + 2).min(last));
        0.5 * (2.0 * p1
            + (-p0 + p2) * local
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * local * local
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * local.powi(3))
    }
    /// Per-segment curves alter traversal progress, never geometry.
    pub fn traversal_parameter(&self, progress: f64) -> f64 {
        let t = progress.clamp(0.0, 1.0);
        if self.points.len() < 2 {
            return t;
        }
        if matches!(self.kind, PathKind::Bezier | PathKind::CubicBezier) {
            return self.points[0].easing.sample(t).clamp(0.0, 1.0);
        }
        let segments = self.points.len() - 1;
        let scaled = t * segments as f64;
        let index = (scaled.floor() as usize).min(segments - 1);
        (index as f64
            + self.points[index]
                .easing
                .sample(scaled - index as f64)
                .clamp(0.0, 1.0))
            / segments as f64
    }
    /// Approximate arc length table for speed-based traversal (256 samples/segment).
    pub fn arc_lengths(&self) -> Vec<(f64, f64)> {
        let samples = ((self.points.len() - 1) * 256).min(65536);
        let mut table = vec![(0.0, 0.0)];
        let mut previous = self.sample(0.0);
        let mut length = 0.0;
        for i in 1..=samples {
            let t = i as f64 / samples as f64;
            let point = self.sample(t);
            length += point.distance(previous);
            table.push((t, length));
            previous = point;
        }
        table
    }
}
/// Maps an eased distance fraction through a precomputed arc-length table.
pub fn distance_parameter(table: &[(f64, f64)], fraction: f64) -> f64 {
    let total = table.last().map_or(0.0, |v| v.1);
    if total <= f64::EPSILON {
        return fraction.clamp(0.0, 1.0);
    }
    let distance = fraction.clamp(0.0, 1.0) * total;
    let index = table
        .partition_point(|v| v.1 < distance)
        .clamp(1, table.len() - 1);
    let (a, b) = (table[index - 1], table[index]);
    if b.1 <= a.1 {
        b.0
    } else {
        a.0 + (b.0 - a.0) * (distance - a.1) / (b.1 - a.1)
    }
}
