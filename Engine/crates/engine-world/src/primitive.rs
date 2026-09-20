use bevy_ecs::prelude::Component;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use thiserror::Error;

const MIN_SEGMENTS: u16 = 3;

/// Editable parametric primitives shared by the 2D and 3D editor.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Primitive {
    Cube {
        size: f32,
    },
    RectangularPrism {
        size: [f32; 3],
    },
    Plane {
        size: [f32; 2],
    },
    Sphere {
        radius: f32,
        segments: u16,
        rings: u16,
    },
    Cylinder {
        radius: f32,
        height: f32,
        segments: u16,
    },
    Capsule {
        radius: f32,
        height: f32,
        segments: u16,
        rings: u16,
    },
    Cone {
        radius: f32,
        height: f32,
        segments: u16,
    },
    Torus {
        major_radius: f32,
        minor_radius: f32,
        segments: u16,
        sides: u16,
    },
    Rectangle2d {
        size: [f32; 2],
    },
    Circle2d {
        radius: f32,
        segments: u16,
    },
    Polygon2d {
        points: Vec<[f32; 2]>,
    },
}

/// Validation failures for editable primitive dimensions.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum PrimitiveError {
    #[error("primitive dimensions must be finite and greater than zero")]
    InvalidDimension,
    #[error("primitive requires at least {MIN_SEGMENTS} segments")]
    TooFewSegments,
    #[error("polygon requires at least three finite points")]
    InvalidPolygon,
}

/// CPU-side, renderer-neutral triangle mesh generated from a primitive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tex_coords: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Primitive {
    /// Validates dimensions before an editor transaction is committed.
    ///
    /// # Errors
    ///
    /// Returns [`PrimitiveError`] for invalid dimensions, tessellation, or polygon data.
    pub fn validate(&self) -> Result<(), PrimitiveError> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        let segments = |value: u16| {
            (value >= MIN_SEGMENTS)
                .then_some(())
                .ok_or(PrimitiveError::TooFewSegments)
        };
        match self {
            Self::Cube { size } => positive(*size)
                .then_some(())
                .ok_or(PrimitiveError::InvalidDimension),
            Self::RectangularPrism { size } => size
                .iter()
                .all(|value| positive(*value))
                .then_some(())
                .ok_or(PrimitiveError::InvalidDimension),
            Self::Plane { size } | Self::Rectangle2d { size } => size
                .iter()
                .all(|value| positive(*value))
                .then_some(())
                .ok_or(PrimitiveError::InvalidDimension),
            Self::Sphere {
                radius,
                segments: columns,
                rings,
            } => {
                positive(*radius)
                    .then_some(())
                    .ok_or(PrimitiveError::InvalidDimension)?;
                segments(*columns)?;
                segments(*rings)
            }
            Self::Cylinder {
                radius,
                height,
                segments: count,
            }
            | Self::Cone {
                radius,
                height,
                segments: count,
            } => {
                if !positive(*radius) || !positive(*height) {
                    return Err(PrimitiveError::InvalidDimension);
                }
                segments(*count)
            }
            Self::Capsule {
                radius,
                height,
                segments: columns,
                rings,
            } => {
                if !positive(*radius) || !positive(*height) {
                    return Err(PrimitiveError::InvalidDimension);
                }
                segments(*columns)?;
                segments(*rings)
            }
            Self::Torus {
                major_radius,
                minor_radius,
                segments: columns,
                sides,
            } => {
                if !positive(*major_radius)
                    || !positive(*minor_radius)
                    || minor_radius >= major_radius
                {
                    return Err(PrimitiveError::InvalidDimension);
                }
                segments(*columns)?;
                segments(*sides)
            }
            Self::Circle2d {
                radius,
                segments: count,
            } => {
                positive(*radius)
                    .then_some(())
                    .ok_or(PrimitiveError::InvalidDimension)?;
                segments(*count)
            }
            Self::Polygon2d { points } => (points.len() >= 3
                && points.iter().flatten().all(|value| value.is_finite()))
            .then_some(())
            .ok_or(PrimitiveError::InvalidPolygon),
        }
    }

    /// Generates deterministic triangle data. Polygon input is treated as a convex fan.
    ///
    /// # Errors
    ///
    /// Returns [`PrimitiveError`] when the primitive parameters are invalid.
    pub fn mesh(&self) -> Result<PrimitiveMesh, PrimitiveError> {
        self.validate()?;
        Ok(match self {
            Self::Cube { size } => box_mesh(Vec3::splat(*size)),
            Self::RectangularPrism { size } => box_mesh(Vec3::from_array(*size)),
            Self::Plane { size } => quad_mesh(Vec2::from_array(*size), false),
            Self::Rectangle2d { size } => quad_mesh(Vec2::from_array(*size), true),
            Self::Sphere {
                radius,
                segments,
                rings,
            } => sphere_mesh(*radius, *segments, *rings, 0.0),
            Self::Capsule {
                radius,
                height,
                segments,
                rings,
            } => sphere_mesh(
                *radius,
                *segments,
                rings.saturating_mul(2),
                (*height * 0.5 - *radius).max(0.0),
            ),
            Self::Cylinder {
                radius,
                height,
                segments,
            } => cone_mesh(*radius, *radius, *height, *segments),
            Self::Cone {
                radius,
                height,
                segments,
            } => cone_mesh(*radius, 0.0, *height, *segments),
            Self::Torus {
                major_radius,
                minor_radius,
                segments,
                sides,
            } => torus_mesh(*major_radius, *minor_radius, *segments, *sides),
            Self::Circle2d { radius, segments } => circle_mesh(*radius, *segments),
            Self::Polygon2d { points } => polygon_mesh(points),
        })
    }
}

fn push_vertex(mesh: &mut PrimitiveMesh, position: Vec3, normal: Vec3, uv: Vec2) {
    mesh.positions.push(position.to_array());
    mesh.normals.push(normal.to_array());
    mesh.tex_coords.push(uv.to_array());
}

fn box_mesh(size: Vec3) -> PrimitiveMesh {
    let h = size * 0.5;
    let faces = [
        (
            Vec3::X,
            [
                Vec3::new(h.x, -h.y, -h.z),
                Vec3::new(h.x, -h.y, h.z),
                Vec3::new(h.x, h.y, h.z),
                Vec3::new(h.x, h.y, -h.z),
            ],
        ),
        (
            -Vec3::X,
            [
                Vec3::new(-h.x, -h.y, h.z),
                Vec3::new(-h.x, -h.y, -h.z),
                Vec3::new(-h.x, h.y, -h.z),
                Vec3::new(-h.x, h.y, h.z),
            ],
        ),
        (
            Vec3::Y,
            [
                Vec3::new(-h.x, h.y, -h.z),
                Vec3::new(h.x, h.y, -h.z),
                Vec3::new(h.x, h.y, h.z),
                Vec3::new(-h.x, h.y, h.z),
            ],
        ),
        (
            -Vec3::Y,
            [
                Vec3::new(-h.x, -h.y, h.z),
                Vec3::new(h.x, -h.y, h.z),
                Vec3::new(h.x, -h.y, -h.z),
                Vec3::new(-h.x, -h.y, -h.z),
            ],
        ),
        (
            Vec3::Z,
            [
                Vec3::new(h.x, -h.y, h.z),
                Vec3::new(-h.x, -h.y, h.z),
                Vec3::new(-h.x, h.y, h.z),
                Vec3::new(h.x, h.y, h.z),
            ],
        ),
        (
            -Vec3::Z,
            [
                Vec3::new(-h.x, -h.y, -h.z),
                Vec3::new(h.x, -h.y, -h.z),
                Vec3::new(h.x, h.y, -h.z),
                Vec3::new(-h.x, h.y, -h.z),
            ],
        ),
    ];
    let mut mesh = PrimitiveMesh::default();
    for (normal, vertices) in faces {
        let base = u32::try_from(mesh.positions.len()).unwrap_or(0);
        for (position, uv) in vertices
            .into_iter()
            .zip([Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y])
        {
            push_vertex(&mut mesh, position, normal, uv);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

fn quad_mesh(size: Vec2, xy: bool) -> PrimitiveMesh {
    let h = size * 0.5;
    let positions = if xy {
        [
            Vec3::new(-h.x, -h.y, 0.0),
            Vec3::new(h.x, -h.y, 0.0),
            Vec3::new(h.x, h.y, 0.0),
            Vec3::new(-h.x, h.y, 0.0),
        ]
    } else {
        [
            Vec3::new(-h.x, 0.0, -h.y),
            Vec3::new(h.x, 0.0, -h.y),
            Vec3::new(h.x, 0.0, h.y),
            Vec3::new(-h.x, 0.0, h.y),
        ]
    };
    let normal = if xy { Vec3::Z } else { Vec3::Y };
    let mut mesh = PrimitiveMesh::default();
    for (position, uv) in positions
        .into_iter()
        .zip([Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y])
    {
        push_vertex(&mut mesh, position, normal, uv);
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh
}

fn sphere_mesh(radius: f32, segments: u16, rings: u16, body_half: f32) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    let columns = segments;
    let rows = rings;
    for row in 0..=rows {
        let v = f32::from(row) / f32::from(rows);
        let latitude = v * PI - PI * 0.5;
        let (sin_lat, cos_lat) = latitude.sin_cos();
        for column in 0..=columns {
            let u = f32::from(column) / f32::from(columns);
            let longitude = u * TAU;
            let (sin_lon, cos_lon) = longitude.sin_cos();
            let normal = Vec3::new(cos_lat * cos_lon, sin_lat, cos_lat * sin_lon);
            let offset = if sin_lat > 0.0 {
                body_half
            } else if sin_lat < 0.0 {
                -body_half
            } else {
                0.0
            };
            push_vertex(
                &mut mesh,
                normal * radius + Vec3::Y * offset,
                normal,
                Vec2::new(u, 1.0 - v),
            );
        }
    }
    grid_indices(&mut mesh.indices, u32::from(columns), u32::from(rows));
    mesh
}

fn cone_mesh(bottom_radius: f32, top_radius: f32, height: f32, segments: u16) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    let count = segments;
    let half = height * 0.5;
    for row in 0..=1_u16 {
        let radius = if row == 0 { bottom_radius } else { top_radius };
        let y = if row == 0 { -half } else { half };
        for column in 0..=count {
            let u = f32::from(column) / f32::from(count);
            let angle = u * TAU;
            let (sin, cos) = angle.sin_cos();
            let normal = Vec3::new(cos, (bottom_radius - top_radius) / height, sin).normalize();
            push_vertex(
                &mut mesh,
                Vec3::new(cos * radius, y, sin * radius),
                normal,
                Vec2::new(u, f32::from(row)),
            );
        }
    }
    grid_indices(&mut mesh.indices, u32::from(count), 1);
    for (radius, y, normal, reverse) in [
        (bottom_radius, -half, -Vec3::Y, true),
        (top_radius, half, Vec3::Y, false),
    ] {
        if radius <= f32::EPSILON {
            continue;
        }
        let center = u32::try_from(mesh.positions.len()).unwrap_or(0);
        push_vertex(&mut mesh, Vec3::Y * y, normal, Vec2::splat(0.5));
        for column in 0..=count {
            let angle = f32::from(column) / f32::from(count) * TAU;
            let (sin, cos) = angle.sin_cos();
            push_vertex(
                &mut mesh,
                Vec3::new(cos * radius, y, sin * radius),
                normal,
                Vec2::new(cos * 0.5 + 0.5, sin * 0.5 + 0.5),
            );
        }
        for column in 0..count {
            let column = u32::from(column);
            let a = center + 1 + column;
            let b = center + 1 + column + 1;
            if reverse {
                mesh.indices.extend_from_slice(&[center, b, a]);
            } else {
                mesh.indices.extend_from_slice(&[center, a, b]);
            }
        }
    }
    mesh
}

fn torus_mesh(major: f32, minor: f32, segments: u16, sides: u16) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    let columns = segments;
    let rows = sides;
    for row in 0..=rows {
        let v = f32::from(row) / f32::from(rows);
        let side = v * TAU;
        let (side_sin, side_cos) = side.sin_cos();
        for column in 0..=columns {
            let u = f32::from(column) / f32::from(columns);
            let around = u * TAU;
            let (sin, cos) = around.sin_cos();
            let normal = Vec3::new(cos * side_cos, side_sin, sin * side_cos);
            let center = Vec3::new(cos * major, 0.0, sin * major);
            push_vertex(&mut mesh, center + normal * minor, normal, Vec2::new(u, v));
        }
    }
    grid_indices(&mut mesh.indices, u32::from(columns), u32::from(rows));
    mesh
}

fn circle_mesh(radius: f32, segments: u16) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    push_vertex(&mut mesh, Vec3::ZERO, Vec3::Z, Vec2::splat(0.5));
    let count = segments;
    for index in 0..=count {
        let angle = f32::from(index) / f32::from(count) * TAU;
        let (sin, cos) = angle.sin_cos();
        push_vertex(
            &mut mesh,
            Vec3::new(cos * radius, sin * radius, 0.0),
            Vec3::Z,
            Vec2::new(cos * 0.5 + 0.5, sin * 0.5 + 0.5),
        );
    }
    for index in 0..count {
        let index = u32::from(index);
        mesh.indices.extend_from_slice(&[0, index + 1, index + 2]);
    }
    mesh
}

fn polygon_mesh(points: &[[f32; 2]]) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    for point in points {
        let point = Vec2::from_array(*point);
        push_vertex(&mut mesh, point.extend(0.0), Vec3::Z, point);
    }
    for index in 1..u32::try_from(points.len().saturating_sub(1)).unwrap_or(1) {
        mesh.indices.extend_from_slice(&[0, index, index + 1]);
    }
    mesh
}

fn grid_indices(indices: &mut Vec<u32>, columns: u32, rows: u32) {
    let stride = columns + 1;
    for row in 0..rows {
        for column in 0..columns {
            let a = row * stride + column;
            let b = a + stride;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_primitive_generates_indexed_geometry() {
        let primitives = [
            Primitive::Cube { size: 1.0 },
            Primitive::RectangularPrism {
                size: [1.0, 2.0, 3.0],
            },
            Primitive::Plane { size: [2.0, 2.0] },
            Primitive::Sphere {
                radius: 1.0,
                segments: 8,
                rings: 4,
            },
            Primitive::Cylinder {
                radius: 1.0,
                height: 2.0,
                segments: 8,
            },
            Primitive::Capsule {
                radius: 0.5,
                height: 2.0,
                segments: 8,
                rings: 4,
            },
            Primitive::Cone {
                radius: 1.0,
                height: 2.0,
                segments: 8,
            },
            Primitive::Torus {
                major_radius: 1.0,
                minor_radius: 0.25,
                segments: 8,
                sides: 6,
            },
            Primitive::Rectangle2d { size: [2.0, 1.0] },
            Primitive::Circle2d {
                radius: 1.0,
                segments: 8,
            },
            Primitive::Polygon2d {
                points: vec![[-1.0, -1.0], [1.0, -1.0], [0.0, 1.0]],
            },
        ];
        for primitive in primitives {
            let mesh = primitive.mesh().unwrap();
            assert!(!mesh.positions.is_empty());
            assert_eq!(mesh.positions.len(), mesh.normals.len());
            assert_eq!(mesh.positions.len(), mesh.tex_coords.len());
            assert!(!mesh.indices.is_empty());
            assert!(mesh.indices.iter().all(|index| {
                usize::try_from(*index).is_ok_and(|index| index < mesh.positions.len())
            }));
        }
    }
}
