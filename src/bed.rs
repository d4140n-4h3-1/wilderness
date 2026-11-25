//! Gravel embedded in the channels' beds, as plant_life's Hydroxus Rivers and Streams tool lays it
//! (`assets/terrain/hydroxus-terrain/hydroxus_rivers.py`, `dress_channel_beds`, c624d86) - the
//! bed's bars and scours it shapes are the terrain's (see `Terrain::bed`).
//!
//! Stones are scattered under the water by its area, more where a slow pattern along the current
//! clusters them, each where the bed is under 4.5 cm to 70 cm of water and no steeper than the
//! tool allows, no two in one 6.5 cm cell: a low seven-sided cap, longer along the current, sunk
//! into the bed. Each batch of them is one opaque mesh of at most [`MOST_STONES`], their colours
//! from a small atlas of six wet stones, made as the tool makes its image.
//!
//! The tool dresses one small glade's waterways in one batch; these run hundreds of meters, so
//! each [`STRETCH`] of a channel is a batch of its own, as dense as the glade's.

use crate::random::Random;
use crate::terrain::Terrain;
use fyrox::core::math::TriangleDefinition;
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Vector2, Vector3, Vector4},
        uuid::Uuid,
    },
    material::{Material, MaterialResource},
    resource::texture::{Texture, TextureKind, TexturePixelKind, TextureResource},
    scene::{
        Scene,
        base::BaseBuilder,
        mesh::{
            MeshBuilder,
            buffer::{TriangleBuffer, VertexBuffer},
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
        },
    },
};
use std::collections::HashSet;

/// The most stones in one batch, and how many to a square meter of water: as the tool's.
const MOST_STONES: usize = 480;
const STONES_PER_SQUARE_METER: f32 = 9.0;
/// How long a stretch of channel is one batch, in meters.
const STRETCH: f32 = 40.0;
/// The six stones' colours, linear: as the tool's palette.
const PALETTE: [[f32; 3]; 6] = [
    [0.09, 0.075, 0.057],
    [0.17, 0.135, 0.096],
    [0.24, 0.21, 0.17],
    [0.105, 0.13, 0.105],
    [0.16, 0.135, 0.13],
    [0.29, 0.25, 0.19],
];
/// Each stone's tile in the atlas, in texels: three across, two down.
const TILE: usize = 96;

/// Lays the gravel in the beds under `surfaces` (each channel's water, triangle by triangle: its
/// corners and their flow coordinates), on `terrain`'s ground. Returns how many stones.
pub fn lay_gravel(scene: &mut Scene, surfaces: &[Vec<([Vector3<f32>; 3], [Vector2<f32>; 3])>], terrain: &Terrain, seed: u64) -> usize {
    let material = material();
    let mut stones = 0;
    for (channel, surface) in surfaces.iter().enumerate() {
        // Batches by stretch downstream.
        let mut stretches: Vec<Vec<([Vector3<f32>; 3], [Vector2<f32>; 3])>> = Vec::new();
        for triangle in surface {
            let along = (triangle.1.iter().map(|uv| uv.x).sum::<f32>() / 3.0 / STRETCH).max(0.0) as usize;
            if stretches.len() <= along {
                stretches.resize(along + 1, Vec::new());
            }
            stretches[along].push(*triangle);
        }
        for (stretch, triangles) in stretches.iter().enumerate() {
            let mut random = Random::for_place(seed, channel as i64, 0x6ab0 + stretch as i64);
            if let Some((data, count)) = batch(triangles, terrain, &mut random) {
                MeshBuilder::new(BaseBuilder::new().with_name("Embedded channel gravel").with_cast_shadows(false))
                    .with_surfaces(vec![SurfaceBuilder::new(SurfaceResource::new_embedded(data)).with_material(material.clone()).build()])
                    .build(&mut scene.graph);
                stones += count;
            }
        }
    }
    stones
}

/// One batch of stones under `triangles`: the mesh, and how many.
fn batch(triangles: &[([Vector3<f32>; 3], [Vector2<f32>; 3])], terrain: &Terrain, random: &mut Random) -> Option<(SurfaceData, usize)> {
    // Each triangle's area across the ground, added up, to pick by.
    let mut cumulative = Vec::with_capacity(triangles.len());
    let mut total = 0.0f32;
    for (p, _) in triangles {
        let (a, b, c) = (p[0].xz(), p[1].xz(), p[2].xz());
        total += 0.5 * ((b - a).perp(&(c - a))).abs();
        cumulative.push(total);
    }
    if total <= 1e-6 {
        return None;
    }
    let target = MOST_STONES.min(((total * STONES_PER_SQUARE_METER).round() as usize).max(1));
    let phase = terrain.bed_phase;
    let (mut vertices, mut faces): (Vec<StaticVertex>, Vec<[u32; 3]>) = (Vec::new(), Vec::new());
    let mut occupied: HashSet<(i32, i32)> = HashSet::new();
    for _ in 0..target * 5 {
        if occupied.len() >= target {
            break;
        }
        let pick = random.unit() * total;
        let index = cumulative.partition_point(|&c| c < pick).min(triangles.len() - 1);
        let ([a, b, c], coords) = triangles[index];
        let (mut r, mut t) = (random.unit(), random.unit());
        if r + t > 1.0 {
            (r, t) = (1.0 - r, 1.0 - t);
        }
        let water = a + (b - a) * r + (c - a) * t;
        let uv = coords[0] + (coords[1] - coords[0]) * r + (coords[2] - coords[0]) * t;
        let cluster = 0.50 + 0.28 * (1.18 * uv.x + 1.65 * uv.y + phase).sin() + 0.21 * (3.7 * uv.x - 7.2 * uv.y).sin();
        if random.unit() > cluster {
            continue;
        }
        // The bed under it, and how steep.
        let (x, z) = (water.x, water.z);
        let ground_y = terrain.height(x, z);
        let e = 0.05;
        let normal = Vector3::new(
            terrain.height(x - e, z) - terrain.height(x + e, z),
            2.0 * e,
            terrain.height(x, z - e) - terrain.height(x, z + e),
        )
        .normalize();
        if normal.y < 0.62 {
            continue;
        }
        let depth = water.y - ground_y;
        if !(0.045..0.70).contains(&depth) {
            continue;
        }
        let ground = Vector3::new(x, ground_y, z);
        if !occupied.insert(((x / 0.065).floor() as i32, (z / 0.065).floor() as i32)) {
            continue;
        }
        let radius = 0.027 + 0.138 * random.unit().powf(2.2);
        let height = (depth * 0.56).min(radius * random.between(0.35, 0.83));
        let length = radius * random.between(1.0, 1.55);
        // Along the current: the way the flow's first coordinate grows over the triangle.
        let (dv1, dv2) = (coords[1].y - coords[0].y, coords[2].y - coords[0].y);
        let flow = (b - a) * dv2 - (c - a) * dv1;
        let angle = flow.z.atan2(flow.x) + random.between(-0.95, 0.95);
        let axis = Vector3::new(angle.cos(), 0.0, angle.sin());
        let lateral = Vector3::new(-axis.z, 0.0, axis.x);
        let start = vertices.len() as u32;
        let tile = (random.unit() * PALETTE.len() as f32) as usize % PALETTE.len();
        let (row, col) = ((tile / 3) as f32, (tile % 3) as f32);
        let atlas = |u: f32, v: f32| Vector2::new((col + 0.5 + u) / 3.0, (row + 0.5 + v) / 2.0);
        const SIDES: u32 = 7;
        let up = Vector3::y();
        for i in 0..SIDES {
            let theta = std::f32::consts::TAU * i as f32 / SIDES as f32 + random.between(-0.15, 0.15);
            let radial = (axis * (theta.cos() * length) + lateral * (theta.sin() * radius)) * random.between(0.82, 1.16);
            let (u, v) = (radial.dot(&axis) / length * 0.34, radial.dot(&lateral) / radius * 0.34);
            let foot = ground + radial - up * (height * 0.26);
            let shoulder = ground + radial * 0.74 + up * (height * random.between(0.54, 0.83));
            vertices.push(vertex(foot, atlas(u, v)));
            vertices.push(vertex(shoulder, atlas(0.74 * u, 0.74 * v)));
        }
        let top = ground + axis * random.between(-length * 0.2, length * 0.2) + up * height;
        vertices.push(vertex(top, atlas(0.0, 0.0)));
        // The sides, then the cap; each triangle turned to face out of the stone.
        let middle = ground + up * (height * 0.3);
        let mut face = |corners: [u32; 3]| {
            let [p, q, s] = corners.map(|k| vertices[k as usize].position);
            let outward = (q - p).cross(&(s - p)).dot(&((p + q + s) / 3.0 - middle)) >= 0.0;
            faces.push(if outward { corners } else { [corners[0], corners[2], corners[1]] });
        };
        for i in 0..SIDES {
            let base = start + i * 2;
            let next = start + (i + 1) % SIDES * 2;
            face([base, next, next + 1]);
            face([base, next + 1, base + 1]);
            face([base + 1, next + 1, start + SIDES * 2]);
        }
    }
    if faces.is_empty() {
        return None;
    }
    let count = occupied.len();
    let mut data = SurfaceData::new(
        VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"),
        TriangleBuffer::new(faces.into_iter().map(TriangleDefinition).collect()),
    );
    let _ = data.calculate_normals();
    let _ = data.calculate_tangents();
    Some((data, count))
}

fn vertex(position: Vector3<f32>, tex_coord: Vector2<f32>) -> StaticVertex {
    StaticVertex { position, tex_coord, normal: Vector3::y(), tangent: Vector4::new(1.0, 0.0, 0.0, 1.0) }
}

/// The wet gravel's material: the standard one, its colour from the stones' atlas.
fn material() -> MaterialResource {
    let mut material = Material::standard();
    material.bind("diffuseTexture", atlas());
    MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, material)
}

/// The six wet stones' atlas, as the tool paints it: each stone's colour, broken by broad
/// swirls, flecks and fine veins, stored sRGB.
fn atlas() -> TextureResource {
    let (width, height) = (3 * TILE, 2 * TILE);
    let mut bytes = vec![255u8; width * height * 4];
    let mut random = Random(1407);
    for (index, shade) in PALETTE.iter().enumerate() {
        let phase = random.between(0.0, std::f32::consts::TAU);
        let (row, col) = (index / 3, index % 3);
        for ty in 0..TILE {
            for tx in 0..TILE {
                let (x, y) = (tx as f32 / TILE as f32, ty as f32 / TILE as f32);
                let broad = (15.0 * x + 8.0 * y + phase).sin() * (10.0 * y - 18.0 * x).cos();
                // A normal deviate, by Box and Muller.
                let flecks = 0.12 * (-2.0 * random.unit().max(1e-7).ln()).sqrt() * (std::f32::consts::TAU * random.unit()).cos();
                let veins = (22.0 * x + 10.0 * y + 1.2 * (16.0 * y).sin() + phase).cos().max(0.0).powi(13);
                let relief = 0.90 + 0.15 * broad + flecks + 0.11 * veins;
                let at = ((row * TILE + ty) * width + col * TILE + tx) * 4;
                for k in 0..3 {
                    let linear = (relief * shade[k]).max(0.0);
                    let srgb = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
                    bytes[at + k] = (srgb.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }
    let texture = Texture::from_bytes(
        TextureKind::Rectangle { width: width as u32, height: height as u32 },
        TexturePixelKind::RGBA8,
        bytes,
    )
    .expect("the atlas's size matches its bytes");
    TextureResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stones_lie_under_the_water_in_the_bed() {
        let terrain = Terrain::new(1);
        // A flat square of water, 4 m across, 40 cm over the ground under its middle.
        let middle = terrain.channels[0].points[150];
        let y = terrain.channels[0].water(150);
        let corner = |dx: f32, dz: f32| Vector3::new(middle.x + dx, y, middle.y + dz);
        let uv = |u: f32, v: f32| Vector2::new(u, v);
        let triangles = vec![
            ([corner(-0.5, -2.0), corner(0.5, -2.0), corner(0.5, 2.0)], [uv(0.0, 0.0), uv(0.0, 1.0), uv(4.0, 1.0)]),
            ([corner(-0.5, -2.0), corner(0.5, 2.0), corner(-0.5, 2.0)], [uv(0.0, 0.0), uv(4.0, 1.0), uv(4.0, 0.0)]),
        ];
        let mut random = Random(3);
        let (data, count) = batch(&triangles, &terrain, &mut random).expect("stones");
        assert!(count > 3 && count <= MOST_STONES, "{count} stones");
        // Fifteen points a stone: seven feet, seven shoulders and a top.
        assert_eq!(data.vertex_buffer.vertex_count() as usize, count * 15);
    }

    #[test]
    fn the_atlas_is_six_stones_of_the_tools_colours() {
        let texture = atlas();
        let state = texture.data_ref();
        assert!(matches!(state.kind(), TextureKind::Rectangle { width: 288, height: 192 }));
    }
}
