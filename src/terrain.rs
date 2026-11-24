//! The lie of the land: a valley with a river down it, from north to south, hills rising either
//! side, and streams running down off the hills into the river.
//!
//! The river is laid out first, a meandering line from north to south that passes near where the
//! player starts. The ground is shaped round it: falling gently southward with the river, rising
//! away from it on both sides, and hills on that rise. Then each stream is traced as water finds
//! its way, as plant_life's Hydroxus Rivers and Streams tool does: from a spring on a hillside,
//! step by step downhill by the steepest way, until it reaches the river (or, caught in a hollow,
//! straight on to it). Last, every channel is carved: its bed kept falling all the way along, cut
//! below its banks across its width in the tool's profile, the ground easing back up to the
//! land's own height beyond.
//!
//! The ground is drawn in [`CHUNK`]-meter squares of [`SPACING`]-meter triangles, each its own mesh
//! (so the parts out of view are culled, and Candelite's cards of each are fine enough), each with
//! a matching solid shape for walking on.

use crate::random::{Noise, Random, smoothstep};
use fyrox::core::math::TriangleDefinition;
use fyrox::{
    core::{
        algebra::{Vector2, Vector3, Vector4},
        pool::Handle,
    },
    material::MaterialResource,
    scene::{
        base::BaseBuilder,
        collider::{ColliderBuilder, ColliderShape, GeometrySource},
        graph::Graph,
        mesh::{
            MeshBuilder,
            buffer::{TriangleBuffer, VertexBuffer},
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
        },
        node::Node,
        rigidbody::{RigidBodyBuilder, RigidBodyType},
    },
};
use std::collections::HashMap;

/// How far the ground reaches from the middle along x and z, in meters: the forest and the border
/// past it.
pub fn half_size() -> f32 {
    crate::forest::HALF_EXTENT + crate::game::BORDER
}

/// How much the river's bed falls for each meter it runs south.
const GRADE: f32 = 0.012;
/// How much higher the land is far from the river than beside it, in meters, and about how far
/// out it has risen most of that.
const VALLEY_RISE: f32 = 3.5;
const VALLEY_WIDTH: f32 = 32.0;
/// How high the hills on the valley's sides are, in meters either way, and how wide.
const HILLS: f32 = 3.0;
const HILL_SIZE: f32 = 38.0;
/// About how far from where the player starts the river runs, in meters.
const RIVER_FROM_START: f32 = 13.0;
/// How many streams run down into the river.
const STREAMS: usize = 3;
/// The ground's triangles' size, and its squares', in meters.
const SPACING: f32 = 0.5;
/// (Large: Candelite gives each mesh cards of at most 256 texels a side, so the ground's cards
/// are a 0.42 m texel at this size, and the whole ground fits its atlas; nine squares still let
/// those out of view be culled, and Yoctite culls the rest cluster by cluster.)
pub const CHUNK: f32 = 108.0;
/// How far past its banks a channel's cut eases back into the land, as a share of its half width.
const BANK_EASE: f32 = 1.5;
/// How far below the banks the water is, as a share of a channel's depth: as the tool lays it.
const WATER_DOWN: f32 = 0.55;

/// A river or a stream: the line it runs along, downstream, and how wide and deep it is cut.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: &'static str,
    /// Its middle, every meter or so, downstream, in x and z.
    pub points: Vec<Vector2<f32>>,
    /// The height of its banks at each point, never rising downstream.
    pub banks: Vec<f32>,
    /// How far its cut reaches either side of its middle, and how deep it is there, in meters.
    pub half_width: f32,
    pub depth: f32,
}

impl Channel {
    /// The height of the water at its `index`th point.
    pub fn water(&self, index: usize) -> f32 {
        self.banks[index] - self.depth * WATER_DOWN
    }

    /// How far either side of the middle the water reaches: where the cut's bed comes up to it.
    pub fn water_half_width(&self) -> f32 {
        // The bed is `depth * (1 - (d / w)^2)^2` below the banks: up at the water where that is
        // WATER_DOWN of the depth.
        self.half_width * (1.0 - WATER_DOWN.sqrt()).sqrt()
    }
}

/// Where a channel runs past a place: which channel, how far from its middle, and the bank and
/// water heights there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    pub channel: usize,
    pub distance: f32,
    pub bank: f32,
    pub water: f32,
}

/// How wide the cells are in which channels' segments are found, in meters.
const CELL: f32 = 8.0;
/// The most channels there can be: the river and the streams.
const MOST_CHANNELS: usize = 1 + STREAMS;

#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    hills: Noise,
    /// The river's meanders: two waves' phases, and how far it is moved across so that it runs
    /// [`RIVER_FROM_START`] from the start.
    phases: [f32; 2],
    shift: f32,
    pub channels: Vec<Channel>,
    cells: HashMap<(i32, i32), Vec<(u32, u32)>>,
}

impl Terrain {
    /// The land a seed shapes: the same seed, the same land.
    pub fn new(seed: u64) -> Self {
        let mut random = Random(seed ^ 0x7e44_a1d5_0c3b_9f21);
        let phases = [random.between(0.0, std::f32::consts::TAU), random.between(0.0, std::f32::consts::TAU)];
        let side = if random.unit() < 0.5 { -1.0 } else { 1.0 };
        let mut terrain = Self {
            hills: Noise(seed ^ 0x51ab_02c4_88e1_d7f3),
            phases,
            shift: 0.0,
            channels: Vec::new(),
            cells: HashMap::new(),
        };
        terrain.shift = side * RIVER_FROM_START - terrain.meander(0.0);

        // The river, from the northern edge to the southern.
        let half = half_size();
        let river: Vec<Vector2<f32>> = (0..=(2.0 * half) as usize)
            .map(|step| {
                let z = half - step as f32;
                Vector2::new(terrain.river_x(z), z)
            })
            .collect();
        terrain.add_channel("River", river, 2.6, 0.85);

        // The streams, each from a spring on a hillside, on alternate sides.
        for stream in 0..STREAMS {
            let side = if (stream % 2 == 0) == (side > 0.0) { -1.0 } else { 1.0 };
            let z = random.between(-0.75 * half, 0.75 * half);
            let away = random.between(45.0, 95.0);
            let spring = Vector2::new(terrain.river_x(z) + side * away, z);
            let points = terrain.trace_down(spring);
            const NAMES: [&str; STREAMS] = ["Stream", "Second stream", "Third stream"];
            terrain.add_channel(NAMES[stream], points, 1.1, 0.45);
        }
        terrain
    }

    fn meander(&self, z: f32) -> f32 {
        9.0 * (z / 37.0 + self.phases[0]).sin() + 5.0 * (z / 17.0 + self.phases[1]).sin()
    }

    /// Where the river's middle is across at `z`.
    pub fn river_x(&self, z: f32) -> f32 {
        self.shift + self.meander(z)
    }

    /// The land's own height, before any channel is cut into it.
    fn land(&self, x: f32, z: f32) -> f32 {
        let across = (x - self.river_x(z)).abs();
        let fall = GRADE * (z + half_size());
        let valley = VALLEY_RISE * across * across / (across * across + VALLEY_WIDTH * VALLEY_WIDTH);
        let hills = HILLS * smoothstep(5.0, 30.0, across) * (self.hills.at(x / HILL_SIZE, z / HILL_SIZE) - 0.5) * 2.0;
        fall + valley + hills
    }

    /// The ground's height at `x`, `z`: the land, with the channels cut into it.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let land = self.land(x, z);
        let mut height = land;
        self.each_near(x, z, |channel, distance, bank| {
            let w = channel.half_width;
            let cut = if distance < w {
                let share = 1.0 - (distance / w).powi(2);
                bank - channel.depth * share * share
            } else {
                // Past the bank, easing back up to the land, where the bank is lower than it.
                bank + (land - bank).max(0.0) * smoothstep(w, w * (1.0 + BANK_EASE), distance)
            };
            height = height.min(cut);
        });
        height
    }

    /// The channel running nearest past `x`, `z`, within its cut, if any.
    pub fn nearest(&self, x: f32, z: f32) -> Option<Nearest> {
        let mut nearest: Option<Nearest> = None;
        self.each_near_indexed(x, z, |channel, distance, bank, water| {
            if nearest.is_none_or(|n| distance < n.distance) {
                nearest = Some(Nearest { channel, distance, bank, water });
            }
        });
        nearest
    }

    /// Whether `x`, `z` is under water, or nearer to it than `margin` meters.
    pub fn wet(&self, x: f32, z: f32, margin: f32) -> bool {
        let mut wet = false;
        self.each_near_indexed(x, z, |channel, distance, _, _| {
            wet |= distance < self.channels[channel].water_half_width() + margin;
        });
        wet
    }

    /// Calls `visit` with each channel whose cut (and the easing past it) reaches `x`, `z`: how
    /// far its nearest segment's middle line is, and the bank height there.
    fn each_near(&self, x: f32, z: f32, mut visit: impl FnMut(&Channel, f32, f32)) {
        self.each_near_indexed(x, z, |channel, distance, bank, _| visit(&self.channels[channel], distance, bank));
    }

    fn each_near_indexed(&self, x: f32, z: f32, mut visit: impl FnMut(usize, f32, f32, f32)) {
        let Some(segments) = self.cells.get(&cell_of(x, z)) else {
            return;
        };
        // Each channel's nearest segment.
        let mut best = [(f32::MAX, 0.0, 0.0); MOST_CHANNELS];
        let at = Vector2::new(x, z);
        for &(channel, first) in segments {
            let c = &self.channels[channel as usize];
            let (i, j) = (first as usize, first as usize + 1);
            let (a, b) = (c.points[i], c.points[j]);
            let along = b - a;
            let t = ((at - a).dot(&along) / along.norm_squared().max(1e-8)).clamp(0.0, 1.0);
            let distance = (at - (a + along * t)).norm();
            let slot = &mut best[channel as usize];
            if distance < slot.0 {
                let bank = c.banks[i] + (c.banks[j] - c.banks[i]) * t;
                let water = c.water(i) + (c.water(j) - c.water(i)) * t;
                *slot = (distance, bank, water);
            }
        }
        for (channel, &(distance, bank, water)) in best.iter().enumerate().take(self.channels.len()) {
            if distance < f32::MAX {
                visit(channel, distance, bank, water);
            }
        }
    }

    /// The way water from `spring` runs: downhill by the steepest way, a meter at a time, until
    /// it reaches the river; caught in a hollow, straight on to the river. Smoothed, as the tool's
    /// channels are, of the grid's zigzag.
    fn trace_down(&self, spring: Vector2<f32>) -> Vec<Vector2<f32>> {
        let reached = |p: Vector2<f32>| (p.x - self.river_x(p.y)).abs() < 2.6 * 0.8;
        let mut path = vec![spring];
        let mut at = spring;
        let half = half_size() - 1.0;
        // Caught in a hollow, how high its floor is: the water fills it and runs on over its lip
        // toward the river, until the land falls below that floor again.
        let mut filling: Option<f32> = None;
        for _ in 0..2000 {
            if reached(at) {
                break;
            }
            let here = self.land(at.x, at.y);
            if let Some(floor) = filling {
                if here < floor - 0.01 {
                    filling = None;
                } else {
                    let toward = Vector2::new(self.river_x(at.y) - at.x, 0.0);
                    at += toward.normalize();
                    path.push(at);
                    continue;
                }
            }
            let mut lowest: Option<(f32, Vector2<f32>)> = None;
            for k in 0..16 {
                let angle = k as f32 * std::f32::consts::TAU / 16.0;
                let next = at + Vector2::new(angle.cos(), angle.sin());
                if next.x.abs() > half || next.y.abs() > half {
                    continue;
                }
                let drop = here - self.land(next.x, next.y);
                if drop > 1e-4 && lowest.is_none_or(|(d, _)| drop > d) {
                    lowest = Some((drop, next));
                }
            }
            at = match lowest {
                Some((_, next)) => next,
                // A hollow: filled, and over its lip toward the river.
                None => {
                    filling = Some(here);
                    let toward = Vector2::new(self.river_x(at.y) - at.x, 0.0);
                    at + toward.normalize()
                }
            };
            path.push(at);
        }
        // Smoothed over seven points, the ends kept.
        let smooth: Vec<Vector2<f32>> = (0..path.len())
            .map(|i| {
                if i == 0 || i + 1 == path.len() {
                    return path[i];
                }
                let window = &path[i.saturating_sub(3)..(i + 4).min(path.len())];
                window.iter().sum::<Vector2<f32>>() / window.len() as f32
            })
            .collect();
        smooth
    }

    /// Adds a channel along `points`: its banks at the land's height, never rising downstream; and
    /// its segments to the grid, wherever its cut and the easing past it reach.
    fn add_channel(&mut self, name: &'static str, points: Vec<Vector2<f32>>, half_width: f32, depth: f32) {
        let mut banks = Vec::with_capacity(points.len());
        let mut lowest = f32::MAX;
        for p in &points {
            lowest = lowest.min(self.land(p.x, p.y));
            banks.push(lowest);
        }
        let index = self.channels.len() as u32;
        let reach = half_width * (1.0 + BANK_EASE);
        for i in 0..points.len().saturating_sub(1) {
            let (a, b) = (points[i], points[i + 1]);
            let (x0, z0) = cell_of(a.x.min(b.x) - reach, a.y.min(b.y) - reach);
            let (x1, z1) = cell_of(a.x.max(b.x) + reach, a.y.max(b.y) + reach);
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    self.cells.entry((cx, cz)).or_default().push((index, i as u32));
                }
            }
        }
        self.channels.push(Channel { name, points, banks, half_width, depth });
    }

    /// Puts the ground into `graph`, in squares drawn with `material`, solid underfoot. Returns
    /// the squares.
    pub fn build_ground(&self, graph: &mut Graph, material: &MaterialResource) -> Vec<Handle<Node>> {
        let half = half_size();
        let chunks = (2.0 * half / CHUNK).ceil() as i32;
        let cells = (CHUNK / SPACING).round() as usize;
        let mut meshes = Vec::new();
        for cx in 0..chunks {
            for cz in 0..chunks {
                let origin = Vector2::new(-half + cx as f32 * CHUNK, -half + cz as f32 * CHUNK);
                meshes.push(self.build_chunk(graph, material, origin, cells));
            }
        }
        let mut body = BaseBuilder::new().with_name("Ground body");
        for &mesh in &meshes {
            body = body.with_child(
                ColliderBuilder::new(BaseBuilder::new())
                    .with_shape(ColliderShape::trimesh(vec![GeometrySource(mesh)]))
                    .build(graph),
            );
        }
        RigidBodyBuilder::new(body).with_body_type(RigidBodyType::Static).build(graph);
        meshes
    }

    fn build_chunk(&self, graph: &mut Graph, material: &MaterialResource, origin: Vector2<f32>, cells: usize) -> Handle<Node> {
        let across = cells + 1;
        let mut vertices = Vec::with_capacity(across * across);
        for j in 0..across {
            for i in 0..across {
                let x = origin.x + i as f32 * SPACING;
                let z = origin.y + j as f32 * SPACING;
                let y = self.height(x, z);
                // The normal across the neighbours either side.
                let e = SPACING;
                let dx = self.height(x + e, z) - self.height(x - e, z);
                let dz = self.height(x, z + e) - self.height(x, z - e);
                let normal = Vector3::new(-dx, 2.0 * e, -dz).normalize();
                vertices.push(StaticVertex {
                    position: Vector3::new(x, y, z),
                    // A meter of the loam's tile to a meter of ground.
                    tex_coord: Vector2::new(x, z),
                    normal,
                    tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
                });
            }
        }
        let mut triangles = Vec::with_capacity(cells * cells * 2);
        for j in 0..cells {
            for i in 0..cells {
                let a = (j * across + i) as u32;
                let (b, c, d) = (a + 1, a + across as u32, a + across as u32 + 1);
                // Wound to face up.
                triangles.push(TriangleDefinition([a, c, b]));
                triangles.push(TriangleDefinition([b, c, d]));
            }
        }
        let mut data = SurfaceData::new(
            VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"),
            TriangleBuffer::new(triangles),
        );
        let _ = data.calculate_tangents();
        MeshBuilder::new(BaseBuilder::new().with_name("Ground").with_cast_shadows(false))
            .with_surfaces(vec![
                SurfaceBuilder::new(SurfaceResource::new_embedded(data))
                    .with_material(material.clone())
                    .build(),
            ])
            .build(graph)
            .to_base()
    }
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_river_runs_near_the_start_and_falls_southward() {
        let terrain = Terrain::new(1);
        assert!((terrain.river_x(0.0).abs() - RIVER_FROM_START).abs() < 1e-3);
        let river = &terrain.channels[0];
        assert!(river.banks.windows(2).all(|w| w[1] <= w[0]), "banks never rise downstream");
        assert!(river.banks[0] > river.banks[river.banks.len() - 1] + 3.0);
        // Its bed is cut below its banks, and the water lies between.
        let p = river.points[river.points.len() / 2];
        let bed = terrain.height(p.x, p.y);
        let water = river.water(river.points.len() / 2);
        assert!(bed < water && water < river.banks[river.points.len() / 2], "bed {bed}, water {water}");
    }

    #[test]
    fn every_stream_reaches_the_river_downhill() {
        for seed in 1..6 {
            let terrain = Terrain::new(seed);
            assert_eq!(terrain.channels.len(), 1 + STREAMS);
            for stream in &terrain.channels[1..] {
                let end = *stream.points.last().unwrap();
                assert!((end.x - terrain.river_x(end.y)).abs() < 2.6, "seed {seed}: {} ends at {end:?}", stream.name);
                assert!(stream.points.len() > 20, "seed {seed}: {} is {} points", stream.name, stream.points.len());
                assert!(stream.banks.windows(2).all(|w| w[1] <= w[0]));
            }
        }
    }

    #[test]
    fn the_start_is_dry_and_the_river_wet() {
        let terrain = Terrain::new(1);
        assert!(!terrain.wet(0.0, 0.0, 1.0));
        let p = terrain.channels[0].points[100];
        assert!(terrain.wet(p.x, p.y, 0.0));
        assert!(terrain.nearest(p.x, p.y).is_some_and(|n| n.channel == 0 && n.distance < 0.5));
    }

    #[test]
    fn the_ground_is_continuous() {
        // No step steeper than one in one between neighbours a few centimeters apart.
        let terrain = Terrain::new(1);
        let mut random = Random(5);
        for _ in 0..20_000 {
            let x = random.between(-150.0, 150.0);
            let z = random.between(-150.0, 150.0);
            let (a, b) = (terrain.height(x, z), terrain.height(x + 0.05, z));
            assert!((a - b).abs() < 0.05, "a step at {x}, {z}: {a} to {b}");
        }
    }
}
