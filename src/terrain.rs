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
/// The river's half width, in meters.
pub const RIVER_HALF_WIDTH: f32 = 2.6;
/// Over how many meters before it meets the river a stream's banks ease down to the river's water.
const JOIN_EASE: f32 = 10.0;
/// How far below the banks the water is, as a share of a channel's depth: as the tool lays it.
const WATER_DOWN: f32 = 0.55;
/// Manning's formula, v = R^(2/3) S^(1/2) / n, its R and n taken as a woodland channel's (a third
/// of a meter deep, n 0.035): v = MANNING x the square root of the slope, in meters a second. And
/// the slowest and fastest the water is let run.
const MANNING: f32 = 13.7;
const SLOWEST: f32 = 0.25;
const FASTEST: f32 = 3.0;
/// Over how far either side the slope a channel's water runs down is measured, in meters.
const SLOPE_OVER: f32 = 4.0;
/// The Birch Glade's shelves along the waterline (its `shape_banks`): how far into the water and
/// out of it the bank is reshaped, in meters; how steeply it falls into the water and rises out of
/// it; and how deep the water is beyond the shelf.
const SHELF_IN: f32 = 0.72;
const SHELF_OUT: f32 = 0.73;
const SHELF_FALL: f32 = 0.37;
const SHELF_RISE: f32 = 0.52;
const BEYOND_SHELF: f32 = 0.28;

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
    /// How far downstream each point is, in meters.
    pub distances: Vec<f32>,
    /// Where its water meets each bank at each point, from the middle, in meters: the right bank
    /// (looking downstream: toward -side, see [`side_of`]) then the left. Each bank wanders on its
    /// own, as the Birch Glade's seeded shorelines do.
    pub waterlines: Vec<[f32; 2]>,
    /// How steeply its water falls at each point, over a few meters either side.
    pub slopes: Vec<f32>,
}

impl Channel {
    /// The height of the water at its `index`th point.
    pub fn water(&self, index: usize) -> f32 {
        self.banks[index] - self.depth * WATER_DOWN
    }

    /// How far past its waterlines its water's ribbon reaches under the banks, in meters: as the
    /// Birch Glade lays it, so that the ground, not the ribbon's edge, makes the water's edge.
    pub fn under_banks(&self) -> f32 {
        self.half_width * 0.13 + 0.12
    }

    /// How fast its water runs at its `index`th point, in meters a second: as gravity drives a
    /// channel's water down its fall, by Manning's formula, the speed growing as the square root
    /// of the slope (a channel's roughness and depth taken as a woodland stream's).
    pub fn speed(&self, index: usize) -> f32 {
        (MANNING * self.slopes[index].max(0.0).sqrt()).clamp(SLOWEST, FASTEST)
    }
}

/// Across the line `points` at its `i`th point: a unit vector, to the left looking downstream.
pub fn side_of(points: &[Vector2<f32>], i: usize) -> Vector2<f32> {
    let tangent = points[(i + 1).min(points.len() - 1)] - points[i.saturating_sub(1)];
    Vector2::new(-tangent.y, tangent.x).try_normalize(1e-6).unwrap_or(Vector2::x())
}

/// Where a place lies against a channel: by its nearest segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Near {
    /// That segment's first point, and how far along it toward the next.
    pub index: usize,
    pub t: f32,
    /// How far from the middle line, and to which side: positive to the left, looking downstream.
    pub across: f32,
    /// How far downstream.
    pub along: f32,
    pub bank: f32,
    pub water: f32,
    /// Where the water meets the bank on that side, from the middle.
    pub waterline: f32,
}

impl Near {
    pub fn distance(&self) -> f32 {
        self.across.abs()
    }

    /// How far past the waterline, out of the water (negative: in it).
    pub fn from_waterline(&self) -> f32 {
        self.across.abs() - self.waterline
    }
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
        let mut shores = Random(seed ^ 0x5e0e_1a3d_2741_0000);
        terrain.add_channel("River", river, RIVER_HALF_WIDTH, 0.85, &mut shores);

        // The streams, each from a spring on a hillside, on alternate sides.
        for stream in 0..STREAMS {
            let side = if (stream % 2 == 0) == (side > 0.0) { -1.0 } else { 1.0 };
            let z = random.between(-0.75 * half, 0.75 * half);
            let away = random.between(45.0, 95.0);
            let spring = Vector2::new(terrain.river_x(z) + side * away, z);
            let points = terrain.trace_down(spring);
            const NAMES: [&str; STREAMS] = ["Stream", "Second stream", "Third stream"];
            terrain.add_channel(NAMES[stream], points, 1.1, 0.45, &mut shores);
            terrain.lower_into_river(terrain.channels.len() - 1);
        }
        for channel in &mut terrain.channels {
            channel.slopes = slopes_of(channel);
        }
        terrain
    }

    /// Eases a stream's banks down over its last meters, so that its water comes to the river's
    /// height where they meet, as plant_life's water join tool eases a tributary into the river.
    fn lower_into_river(&mut self, stream: usize) {
        let end = *self.channels[stream].points.last().expect("a stream has points");
        let Some((_, river_water)) = self.along(0, end.x, end.y) else {
            return;
        };
        let channel = &mut self.channels[stream];
        let target = river_water + channel.depth * WATER_DOWN;
        let mut from_end = 0.0;
        for i in (0..channel.points.len()).rev() {
            if i + 1 < channel.points.len() {
                from_end += (channel.points[i + 1] - channel.points[i]).norm();
            }
            let ease = smoothstep(JOIN_EASE, 0.0, from_end);
            let bank = channel.banks[i];
            channel.banks[i] = bank + (target.min(bank) - bank) * ease;
        }
        // Still never rising downstream.
        for i in 1..channel.banks.len() {
            channel.banks[i] = channel.banks[i].min(channel.banks[i - 1]);
        }
    }

    /// Where `x`, `z` lies against the channel `channel`, if within its cut and the easing past it.
    pub fn near(&self, channel: usize, x: f32, z: f32) -> Option<Near> {
        let mut found = None;
        self.each_near(x, z, |c, near| {
            if c == channel {
                found = Some(near);
            }
        });
        found
    }

    /// How far `x`, `z` is from the middle of the channel `channel`, and the height of its water
    /// there, if it is within its cut and the easing past it.
    pub fn along(&self, channel: usize, x: f32, z: f32) -> Option<(f32, f32)> {
        self.near(channel, x, z).map(|n| (n.distance(), n.water))
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
        let mut cut = land;
        self.each_near(x, z, |channel, near| {
            let channel = &self.channels[channel];
            let (w, distance, bank) = (channel.half_width, near.distance(), near.bank);
            let here = if distance < w {
                let share = 1.0 - (distance / w).powi(2);
                bank - channel.depth * share * share
            } else {
                // Past the bank, easing back up to the land, where the bank is lower than it.
                bank + (land - bank).max(0.0) * smoothstep(w, w * (1.0 + BANK_EASE), distance)
            };
            cut = cut.min(here);
        });
        // Then the shelves along each waterline, as the Birch Glade shapes them: a shallow slope
        // into the water, gravel to wade on, and a steeper rise out of it; deeper beyond.
        let mut shaped: Option<f32> = None;
        self.each_near(x, z, |channel, near| {
            let channel = &self.channels[channel];
            let offset = near.from_waterline();
            let total = *channel.distances.last().expect("a channel has points");
            let ends = (near.along / 0.7).min((total - near.along) / 0.7).clamp(0.0, 1.0);
            let mut target = cut;
            if offset > -SHELF_IN && offset < SHELF_OUT {
                let influence = ((offset + SHELF_IN) / 0.44).min((SHELF_OUT - offset) / 0.43).min(1.0) * ends;
                let profile = near.water + offset * if offset < 0.0 { SHELF_FALL } else { SHELF_RISE };
                target = cut + (profile - cut) * influence;
            }
            // Past the shelf, down to deeper water, from the shelf as it eases off.
            if offset < -BEYOND_SHELF {
                let deep = target.min(near.water - BEYOND_SHELF);
                target += (deep - target) * smoothstep(-BEYOND_SHELF, -SHELF_IN, offset) * ends;
            }
            shaped = Some(shaped.map_or(target, |s: f32| s.min(target)));
        });
        shaped.unwrap_or(cut)
    }

    /// Whether `x`, `z` is under water, or nearer to it than `margin` meters.
    pub fn wet(&self, x: f32, z: f32, margin: f32) -> bool {
        let mut wet = false;
        self.each_near(x, z, |_, near| wet |= near.from_waterline() < margin);
        wet
    }

    /// Calls `visit` with each channel whose cut (and the easing past it) reaches `x`, `z`, and
    /// where the place lies against it.
    fn each_near(&self, x: f32, z: f32, mut visit: impl FnMut(usize, Near)) {
        let Some(segments) = self.cells.get(&cell_of(x, z)) else {
            return;
        };
        // Each channel's nearest segment.
        let mut best: [Option<(f32, usize, f32)>; MOST_CHANNELS] = [None; MOST_CHANNELS];
        let at = Vector2::new(x, z);
        for &(channel, first) in segments {
            let c = &self.channels[channel as usize];
            let i = first as usize;
            let (a, b) = (c.points[i], c.points[i + 1]);
            let along = b - a;
            let t = ((at - a).dot(&along) / along.norm_squared().max(1e-8)).clamp(0.0, 1.0);
            let distance = (at - (a + along * t)).norm();
            let slot = &mut best[channel as usize];
            if slot.is_none_or(|(d, _, _)| distance < d) {
                *slot = Some((distance, i, t));
            }
        }
        for (channel, found) in best.iter().enumerate().take(self.channels.len()) {
            let Some((_, i, t)) = *found else {
                continue;
            };
            let c = &self.channels[channel];
            let j = i + 1;
            let (a, b) = (c.points[i], c.points[j]);
            let middle = a + (b - a) * t;
            let left = (b - a).try_normalize(1e-6).map_or(Vector2::x(), |d| Vector2::new(-d.y, d.x));
            let across = (at - middle).dot(&left);
            let side = (across >= 0.0) as usize;
            let lerp = |p: f32, q: f32| p + (q - p) * t;
            visit(
                channel,
                Near {
                    index: i,
                    t,
                    across,
                    along: lerp(c.distances[i], c.distances[j]),
                    bank: lerp(c.banks[i], c.banks[j]),
                    water: lerp(c.water(i), c.water(j)),
                    waterline: lerp(c.waterlines[i][side], c.waterlines[j][side]),
                },
            );
        }
    }

    /// The way water from `spring` runs: downhill by the steepest way, a meter at a time, until
    /// it reaches the river; caught in a hollow, straight on to the river. Smoothed, as the tool's
    /// channels are, of the grid's zigzag.
    fn trace_down(&self, spring: Vector2<f32>) -> Vec<Vector2<f32>> {
        // Into the river's water, which the stream's own is then cut back to (see `water.rs`).
        let reached = |p: Vector2<f32>| (p.x - self.river_x(p.y)).abs() < RIVER_HALF_WIDTH * 0.2;
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
    fn add_channel(&mut self, name: &'static str, points: Vec<Vector2<f32>>, half_width: f32, depth: f32, shores: &mut Random) {
        let mut banks = Vec::with_capacity(points.len());
        let mut lowest = f32::MAX;
        for p in &points {
            lowest = lowest.min(self.land(p.x, p.y));
            banks.push(lowest);
        }
        let mut distances = Vec::with_capacity(points.len());
        let mut distance = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                distance += (p - points[i - 1]).norm();
            }
            distances.push(distance);
        }
        // Each bank wanders on its own at three scales, as the Birch Glade's seeded shorelines.
        let phases: [[f32; 3]; 2] = std::array::from_fn(|_| std::array::from_fn(|_| shores.between(0.0, std::f32::consts::TAU)));
        let waterlines = distances
            .iter()
            .map(|&d| {
                std::array::from_fn(|side| {
                    let [broad, middle, fine] = phases[side];
                    half_width
                        * (0.49 + 0.075 * (0.86 * d + broad).sin() + 0.038 * (1.74 * d + middle).sin() + 0.017 * (3.4 * d + fine).sin())
                })
            })
            .collect();
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
        let slopes = vec![0.0; points.len()];
        self.channels.push(Channel { name, points, banks, half_width, depth, distances, waterlines, slopes });
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

/// How steeply `channel`'s water falls at each point: the drop of its water over [`SLOPE_OVER`]
/// meters either side, over that run.
fn slopes_of(channel: &Channel) -> Vec<f32> {
    let n = channel.points.len();
    (0..n)
        .map(|i| {
            let d = channel.distances[i];
            let upstream = (0..=i).rev().find(|&k| d - channel.distances[k] >= SLOPE_OVER).unwrap_or(0);
            let downstream = (i..n).find(|&k| channel.distances[k] - d >= SLOPE_OVER).unwrap_or(n - 1);
            let run = channel.distances[downstream] - channel.distances[upstream];
            if run <= 1e-3 { 0.0 } else { (channel.water(upstream) - channel.water(downstream)) / run }
        })
        .collect()
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
                assert!(terrain.near(0, end.x, end.y).is_some_and(|n| n.from_waterline() < 0.0), "seed {seed}: {} ends at {end:?}", stream.name);
                // Its water comes down to the river's where they meet.
                let (_, river_water) = terrain.along(0, end.x, end.y).expect("at the river");
                let stream_water = stream.water(stream.points.len() - 1);
                assert!((stream_water - river_water).abs() < 0.02, "seed {seed}: {} water {stream_water}, river {river_water}", stream.name);
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
        assert!(terrain.along(0, p.x, p.y).is_some_and(|(distance, _)| distance < 0.5));
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
