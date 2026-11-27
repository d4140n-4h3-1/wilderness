//! The lie of the land: a valley with a river down it, from north to south, hills rising either
//! side, and streams running down off the hills into the river.
//!
//! The river is laid out first, a meandering line from north to south that passes near where the
//! player starts. The ground is shaped round it: falling gently southward with the river, rising
//! away from it on both sides, and hills on that rise. Then each stream is traced as water finds
//! its way, as plant_life's Hydroxus Rivers and Streams tool does: from a spring on a hillside,
//! step by step downhill by the steepest way, until it reaches the river (or, caught in a hollow,
//! straight on to it). Then every channel is carved: its bed kept falling all the way along, cut
//! below its banks across its width in the tool's profile, the ground easing back up to the
//! land's own height beyond. Last, the basins, as the tool's Pond or Spring makes them (see
//! [`Basin`]): a spring pool at the head of each stream, its outlet left open for the stream to
//! run out of, and still ponds here and there in the woods.
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
/// How near the water's surface the bed's relief never comes, in meters: as plant_life's.
pub const BED_CLEAR: f32 = 0.035;
/// The Birch Glade's Alder Spring: its radii (the longer along its outlet) and depth, in meters,
/// and how far down its stream its level is taken from.
const SPRING_RADII: (f32, f32) = (1.45, 1.05);
const SPRING_DEPTH: f32 = 0.43;
const SPRING_LEVEL_FROM: f32 = 1.25;
/// How far a spring rises from any other stream's course, at the least, in meters.
const SPRINGS_APART: f32 = 12.0;
/// How deep a stream is cut.
const STREAM_DEPTH: f32 = 0.45;
/// How far from the start every stream keeps, in meters.
const START_DRY: f32 = 10.0;
/// How near a stream never runs back to its own course further up, in meters: past the reach
/// of its cut either side, so that the two stretches of it do not cut one bank into a cliff.
/// (Points of a course this far apart along it are further up or down it, not beside.)
const OWN_COURSE_APART: f32 = 6.0;
/// How many still ponds there are, and the Birch Glade's Heather Pond's radii and depth, which
/// each is that size or up to [`POND_GROWS`] times it across.
const PONDS: usize = 3;
const POND_RADII: (f32, f32) = (2.42, 1.72);
const POND_DEPTH: f32 = 0.84;
const POND_GROWS: f32 = 2.4;
/// How far out a basin's water's surface reaches, past its waterline, under its banks; and how far
/// out its carving reaches, eased into the ground over the last [`BASIN_FEATHER`]: as the tool's,
/// in shares of its radius.
pub const BASIN_WATER: f32 = 1.13;
const BASIN_REACH: f32 = 1.34;
const BASIN_FEATHER: f32 = 0.19;

/// A pond or a spring: a basin carved into the ground and filled level, as plant_life's Hydroxus
/// tool's `create_basin` makes them. Its shoreline wanders by its seed; a deep core grades out onto
/// a broad shallow shelf, and its rim rises a little out of the water before easing into the
/// ground. A spring's rim is left open at its outlet, where its stream runs out.
#[derive(Debug, Clone, PartialEq)]
pub struct Basin {
    pub name: &'static str,
    pub center: Vector2<f32>,
    /// Its radii along x and z, in meters (the tool's east-west and north-south).
    pub radii: Vector2<f32>,
    pub depth: f32,
    /// The height of its water.
    pub level: f32,
    /// Its shoreline's two phases, from its seed.
    phases: [f32; 2],
    /// A spring's outlet: the angle toward it (in the basin's own frame, its radii stretched to
    /// one, from +x toward +z), and the channel it feeds.
    pub outlet: Option<(f32, usize)>,
}

impl Basin {
    fn new(name: &'static str, center: Vector2<f32>, radii: Vector2<f32>, depth: f32, level: f32, seed: &mut Random) -> Self {
        let phases = [seed.between(0.0, std::f32::consts::TAU), seed.between(0.0, std::f32::consts::TAU)];
        Self { name, center, radii, depth, level, phases, outlet: None }
    }

    /// How far out its shore lies at `angle`, as a share of its radii: the tool's `shoreline`.
    pub fn shoreline(&self, angle: f32) -> f32 {
        1.0 + 0.11 * (3.0 * angle + self.phases[0]).sin() + 0.058 * (5.0 * angle + self.phases[1]).cos()
    }

    /// How far out `x`, `z` is, 1 at the waterline, and at which angle (in its own frame).
    pub fn polar(&self, x: f32, z: f32) -> (f32, f32) {
        let (dx, dz) = ((x - self.center.x) / self.radii.x, (z - self.center.y) / self.radii.y);
        let angle = dz.atan2(dx);
        (dx.hypot(dz) / self.shoreline(angle), angle)
    }

    /// The place `out` of the way out to its waterline at `angle`.
    pub fn at(&self, angle: f32, out: f32) -> Vector2<f32> {
        let r = out * self.shoreline(angle);
        self.center + Vector2::new(self.radii.x * r * angle.cos(), self.radii.y * r * angle.sin())
    }

    /// How far its carving reaches from its middle, at most, in meters.
    pub fn reach(&self) -> f32 {
        self.radii.x.max(self.radii.y) * BASIN_REACH * 1.17
    }

    /// About how far `x`, `z` is past its waterline, out of the water, in meters (negative: in it).
    pub fn from_waterline(&self, x: f32, z: f32) -> f32 {
        let (out, angle) = self.polar(x, z);
        (out - 1.0) * (self.at(angle, 1.0) - self.center).norm()
    }

    /// Whether it is a spring (its outlet open), not a pond.
    pub fn is_spring(&self) -> bool {
        self.outlet.is_some()
    }

    /// The ground at `x`, `z`, `ground` before it was carved: the tool's carving, but for its
    /// outlet, eased open over a few degrees and centimeters rather than at once.
    fn carve(&self, x: f32, z: f32, ground: f32) -> f32 {
        let (out, angle) = self.polar(x, z);
        if out >= BASIN_REACH {
            return ground;
        }
        let submerged = (1.0 - out).clamp(0.0, 1.0);
        let core = (submerged / 0.72).min(1.0);
        let shelf = (submerged / 0.23).min(1.0);
        let mut target = if out > 1.0 {
            self.level + ((out - 1.0) * self.depth * 1.7).min(0.24)
        } else {
            self.level - self.depth * (0.77 * core.powf(1.45) + 0.23 * shelf)
        };
        let feather = ((BASIN_REACH - out) / BASIN_FEATHER).min(1.0);
        if let Some((outlet, _)) = self.outlet {
            // The downhill channel kept at the outlet, rather than dammed.
            let open = smoothstep(0.82, 0.92, (angle - outlet).cos()) * smoothstep(0.65, 0.75, out);
            target += (target.min(ground) - target) * open;
        }
        ground + (target - ground) * feather
    }
}

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
    /// How far across from the middle line, and to which side: positive to the left, looking
    /// downstream.
    pub across: f32,
    /// How far from the middle line, the nearest way: across it, but round the outside of a
    /// bend (or past an end), to the point there.
    pub away: f32,
    /// How far downstream.
    pub along: f32,
    pub bank: f32,
    pub water: f32,
    /// Where the water meets the bank on that side, from the middle.
    pub waterline: f32,
}

impl Near {
    pub fn distance(&self) -> f32 {
        self.away
    }

    /// How far past the waterline, out of the water (negative: in it).
    pub fn from_waterline(&self) -> f32 {
        self.away - self.waterline
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
    /// The phase of the bed's bars and scours: as plant_life's, from the shores' seed.
    pub bed_phase: f32,
    pub channels: Vec<Channel>,
    /// The springs, one at the head of each stream, then the ponds.
    pub basins: Vec<Basin>,
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
            bed_phase: 0.0,
            channels: Vec::new(),
            basins: Vec::new(),
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
        terrain.bed_phase = shores.between(0.0, std::f32::consts::TAU);
        terrain.add_channel("River", river, RIVER_HALF_WIDTH, 0.85, &mut shores);

        // The streams, each from a spring on a hillside, on alternate sides.
        for stream in 0..STREAMS {
            let side = if (stream % 2 == 0) == (side > 0.0) { -1.0 } else { 1.0 };
            // Not on another stream's course (two streams from one spring), nor where its water
            // would come back past its own course (out of a hollow, past the spring), nor run
            // through the start, nor come out below the river: drawn again if so.
            let mut points;
            let mut tries = 0;
            loop {
                let z = random.between(-0.75 * half, 0.75 * half);
                let away = random.between(45.0, 95.0);
                let spring = Vector2::new(terrain.river_x(z) + side * away, z);
                tries += 1;
                let taken = terrain.channels[1..].iter().any(|c| c.points.iter().any(|p| (p - spring).norm() < SPRINGS_APART));
                points = terrain.trace_down(spring);
                let through_start = points.iter().any(|p| p.norm() < START_DRY);
                // Nor come down below the river's water before it reaches it.
                let end = *points.last().expect("a course has points");
                let lowest = points.iter().map(|p| terrain.land(p.x, p.y)).fold(f32::MAX, f32::min);
                let below = terrain.along(0, end.x, end.y).is_some_and(|(_, river)| lowest < river + STREAM_DEPTH * WATER_DOWN);
                if (!taken && !comes_back(&points) && !through_start && !below) || tries >= 50 {
                    break;
                }
            }
            const NAMES: [&str; STREAMS] = ["Stream", "Second stream", "Third stream"];
            terrain.add_channel(NAMES[stream], points, 1.1, STREAM_DEPTH, &mut shores);
            terrain.lower_into_river(terrain.channels.len() - 1);
        }
        for channel in &mut terrain.channels {
            channel.slopes = slopes_of(channel);
        }

        // A spring at each stream's head, as the Birch Glade's Alder Spring feeds Fern Stream:
        // its longer radius along its outlet, its level just over its stream's water a little way
        // down.
        let mut basins = Random(seed ^ 0xba51_7500_0000_0061);
        for c in 1..terrain.channels.len() {
            let channel = &terrain.channels[c];
            let head = channel.points[0];
            let k = channel.distances.iter().position(|&d| d >= SPRING_LEVEL_FROM).unwrap_or(channel.points.len() - 1);
            let toward = channel.points[k] - head;
            let radii = if toward.x.abs() >= toward.y.abs() {
                Vector2::new(SPRING_RADII.0, SPRING_RADII.1)
            } else {
                Vector2::new(SPRING_RADII.1, SPRING_RADII.0)
            };
            const NAMES: [&str; STREAMS] = ["Spring", "Second spring", "Third spring"];
            let mut spring = Basin::new(NAMES[c - 1], head, radii, SPRING_DEPTH, channel.water(k) + 0.012, &mut basins);
            spring.outlet = Some(((toward.y / radii.y).atan2(toward.x / radii.x), c));
            terrain.basins.push(spring);
        }
        // And still ponds, in hollows of their own: out of the way of the water and the start,
        // on ground about level, the first not far from the start. Each at the tool's level, a
        // little under the ground at its middle.
        const POND_NAMES: [&str; PONDS] = ["Pond", "Second pond", "Third pond"];
        let forest = crate::forest::HALF_EXTENT - 12.0;
        for _ in 0..600 {
            if terrain.basins.len() >= terrain.channels.len() - 1 + PONDS {
                break;
            }
            let first = terrain.basins.len() == terrain.channels.len() - 1;
            let grows = basins.between(1.0, POND_GROWS);
            let radii = Vector2::new(POND_RADII.0, POND_RADII.1) * grows;
            let center = if first {
                let (angle, away) = (basins.between(0.0, std::f32::consts::TAU), basins.between(18.0, 34.0));
                Vector2::new(angle.cos(), angle.sin()) * away
            } else {
                Vector2::new(basins.between(-forest, forest), basins.between(-forest, forest))
            };
            let reach = radii.x.max(radii.y) * BASIN_REACH * 1.17;
            if center.norm() < 12.0 + reach || terrain.wet(center.x, center.y, reach + 6.0) {
                continue;
            }
            let heights: Vec<f32> = (0..12)
                .map(|k| {
                    let angle = k as f32 * std::f32::consts::TAU / 12.0;
                    let at = center + Vector2::new(radii.x * angle.cos(), radii.y * angle.sin()) * BASIN_REACH;
                    terrain.channelled(at.x, at.y)
                })
                .collect();
            let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
            if high - low > 0.45 {
                continue;
            }
            let level = terrain.channelled(center.x, center.y) - (POND_DEPTH * 0.24).min(0.22);
            let name = POND_NAMES[terrain.basins.len() - (terrain.channels.len() - 1)];
            terrain.basins.push(Basin::new(name, center, radii, POND_DEPTH, level, &mut basins));
        }
        terrain
    }

    /// Calls `visit` with each basin whose carving reaches `x`, `z`.
    fn each_basin(&self, x: f32, z: f32, mut visit: impl FnMut(&Basin)) {
        for basin in &self.basins {
            let reach = basin.reach();
            if (x - basin.center.x).abs() < reach && (z - basin.center.y).abs() < reach {
                visit(basin);
            }
        }
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

    /// The ground's height at `x`, `z`: the land, with the channels cut into it, then the basins,
    /// then the beds under all the water shaped.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let mut ground = self.channelled(x, z);
        self.each_basin(x, z, |basin| ground = basin.carve(x, z, ground));
        self.bed(x, z, ground)
    }

    /// The ground at `x`, `z` with the channels cut into it and their banks shaped.
    fn channelled(&self, x: f32, z: f32) -> f32 {
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
        //
        // Where two channels' shelves meet - a stream running into the river - each one's change
        // to the cut is taken apart: the deepest lowering of any; and the highest raising of any,
        // but only where the place is out of every other channel's water (so that the river's
        // bank does not dam a stream's mouth). Each change fades to nothing at the edge of its
        // channel's reach, so the ground has no step where a channel comes into reach or goes.
        let mut nears: [Option<(f32, Near)>; MOST_CHANNELS] = [None; MOST_CHANNELS];
        self.each_near(x, z, |channel, near| {
            let total = *self.channels[channel].distances.last().expect("a channel has points");
            nears[channel] = Some((total, near));
        });
        let mut lowest = 0.0f32;
        let mut highest = 0.0f32;
        for (c, found) in nears.iter().enumerate() {
            let Some((total, near)) = *found else {
                continue;
            };
            let offset = near.from_waterline();
            if offset >= SHELF_OUT {
                continue;
            }
            let ends = (near.along / 0.7).min((total - near.along) / 0.7).clamp(0.0, 1.0);
            let mut target = cut;
            if offset > -SHELF_IN {
                let influence = ((offset + SHELF_IN) / 0.44).min((SHELF_OUT - offset) / 0.43).min(1.0) * ends;
                let profile = near.water + offset * if offset < 0.0 { SHELF_FALL } else { SHELF_RISE };
                target = cut + (profile - cut) * influence;
            }
            // Past the shelf, down to deeper water, from the shelf as it eases off.
            if offset < -BEYOND_SHELF {
                let deep = target.min(near.water - BEYOND_SHELF);
                target += (deep - target) * smoothstep(-BEYOND_SHELF, -SHELF_IN, offset) * ends;
            }
            let change = target - cut;
            if change < 0.0 {
                lowest = lowest.min(change);
            } else {
                let dry = nears
                    .iter()
                    .enumerate()
                    .filter(|(o, other)| *o != c && other.is_some())
                    .map(|(_, other)| smoothstep(-0.2, 0.2, other.expect("some").1.from_waterline()))
                    .product::<f32>();
                highest = highest.max(change * dry);
            }
        }
        cut + lowest + highest
    }

    /// The bed under the water, as plant_life's `dress_channel_beds` shapes it: wherever the
    /// ground lies at least [`BED_CLEAR`] under a channel's water, low gravel bars and troughs
    /// alternating along the current, the deeper runs scoured, raised or lowered more the deeper
    /// the water, never to within [`BED_CLEAR`] of its surface. `ground` is the ground there
    /// before. Where two channels' water lies over it - where a stream runs into the river, both
    /// at one height - each one's relief, along its own current, is blended by how much of it
    /// there is, so that the bed has no step where one gives way to the other. A basin's bed is
    /// shaped the same, along its own flow coordinates; where a stream's head lies in its spring,
    /// the spring's water, not the stream's, is over it, as the join trims the stream there.
    fn bed(&self, x: f32, z: f32, ground: f32) -> f32 {
        let (mut weight, mut relief, mut water) = (0.0f32, 0.0f32, 0.0f32);
        let mut most = 0.0f32;
        let mut covered = 0.0f32;
        let mut add = |along: f32, cross: f32, level: f32, envelope: f32| {
            let bar = (along * 1.18 + 1.65 * cross + self.bed_phase).sin();
            let scour = (along * 0.51 - 2.1 * cross + self.bed_phase).cos();
            let texture = (along * 5.1 + cross * 8.6).sin() * (cross * 6.7 - along * 2.2).cos();
            weight += envelope;
            relief += envelope * (0.079 * bar + 0.062 * scour + 0.018 * texture);
            water += envelope * level;
            most = most.max(envelope);
        };
        self.each_basin(x, z, |basin| {
            // Within its water's surface, faded over its last few centimeters.
            let (out, _) = basin.polar(x, z);
            let inside = smoothstep(0.0, 0.15, (BASIN_WATER - out) * basin.radii.x.min(basin.radii.y));
            covered = covered.max(inside);
            let envelope = ((basin.level - ground - BED_CLEAR) / 0.22).clamp(0.0, 1.0) * inside;
            if envelope > 0.0 {
                // Its flow coordinates, as the tool lays them: meters across it, from its corner.
                add(x - basin.center.x + basin.radii.x, z - basin.center.y + basin.radii.y, basin.level, envelope);
            }
        });
        self.each_near(x, z, |channel, near| {
            let channel = &self.channels[channel];
            // Within the water's ribbon (see `water.rs`), which reaches under the banks.
            // (Faded over its last few centimeters: out over another channel's water, where a
            // stream ends in the river, its edge is no bank.)
            let under = channel.under_banks();
            let inside = smoothstep(0.0, 0.15, near.waterline + under - near.distance());
            let depth = near.water - ground;
            let envelope = ((depth - BED_CLEAR) / 0.22).clamp(0.0, 1.0) * inside * (1.0 - covered);
            if envelope <= 0.0 {
                return;
            }
            // The ribbon's flow coordinates there: along, downstream; across, from its right edge.
            let i = near.index;
            let j = (i + 1).min(channel.points.len() - 1);
            let right = channel.waterlines[i][0] + (channel.waterlines[j][0] - channel.waterlines[i][0]) * near.t + under;
            add(near.along, near.across + right, near.water, envelope);
        });
        if weight <= 0.0 {
            return ground;
        }
        (water / weight - BED_CLEAR).min(ground + relief / weight * most)
    }

    /// How deep the water stands over the ground at `x`, `z`, in meters: 0 out of it.
    pub fn water_depth(&self, x: f32, z: f32) -> f32 {
        let ground = self.height(x, z);
        let mut depth = 0.0f32;
        self.each_near(x, z, |_, near| {
            if near.from_waterline() < 0.0 {
                depth = depth.max(near.water - ground);
            }
        });
        for basin in &self.basins {
            if (Vector2::new(x, z) - basin.center).norm() < basin.reach() && basin.from_waterline(x, z) < 0.0 {
                depth = depth.max(basin.level - ground);
            }
        }
        depth.max(0.0)
    }

    /// Whether `x`, `z` is under water, or nearer to it than `margin` meters.
    pub fn wet(&self, x: f32, z: f32, margin: f32) -> bool {
        let mut wet = false;
        self.each_near(x, z, |_, near| wet |= near.from_waterline() < margin);
        for basin in &self.basins {
            wet |= (Vector2::new(x, z) - basin.center).norm() < basin.reach() + margin && basin.from_waterline(x, z) < margin;
        }
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
        // (The nearest segment can change from one to the next at a bend, its `across` with it;
        // but the distance is to the nearest point of either, and so has no step there.)
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
            let Some((away, i, t)) = *found else {
                continue;
            };
            let c = &self.channels[channel];
            let j = i + 1;
            let (a, b) = (c.points[i], c.points[j]);
            let middle = a + (b - a) * t;
            let left = (b - a).try_normalize(1e-6).map_or(Vector2::x(), |d| Vector2::new(-d.y, d.x));
            let across = (at - middle).dot(&left);
            let lerp = |p: f32, q: f32| p + (q - p) * t;
            // The waterline on the place's side; across the middle, easing from one bank's to the
            // other's, so the distance to it has no step there.
            let (right, left_line) = (lerp(c.waterlines[i][0], c.waterlines[j][0]), lerp(c.waterlines[i][1], c.waterlines[j][1]));
            let middle_band = 0.5 * right.min(left_line);
            let waterline = right + (left_line - right) * smoothstep(-middle_band, middle_band, across);
            visit(
                channel,
                Near {
                    index: i,
                    t,
                    across,
                    away,
                    along: lerp(c.distances[i], c.distances[j]),
                    bank: lerp(c.banks[i], c.banks[j]),
                    water: lerp(c.water(i), c.water(j)),
                    waterline,
                },
            );
        }
    }

    /// The way water from `spring` runs: downhill by the steepest way, a meter at a time, until
    /// it reaches the river; caught in a hollow, straight on to the river. Its turns rounded, as
    /// the tool rounds a tributary's.
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
        // Its turns rounded, as the tool's `smooth_channel_path` rounds a tributary's: each point
        // the average of up to four either side, the nearer weighing more, the ends kept. (Its
        // height is the banks', which never rise downstream.)
        (0..path.len())
            .map(|i| {
                let radius = 4.min(i).min(path.len() - 1 - i);
                let (mut sum, mut weights) = (Vector2::zeros(), 0.0);
                for offset in -(radius as i64)..=radius as i64 {
                    let weight = (radius as i64 + 1 - offset.abs()) as f32;
                    sum += path[(i as i64 + offset) as usize] * weight;
                    weights += weight;
                }
                sum / weights
            })
            .collect()
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

/// Whether the course `points` (about a meter apart) comes back within [`OWN_COURSE_APART`] of
/// itself further up.
fn comes_back(points: &[Vector2<f32>]) -> bool {
    let apart = (3.0 * OWN_COURSE_APART) as usize;
    (apart..points.len()).any(|j| points[..j - apart].iter().any(|p| (p - points[j]).norm() < OWN_COURSE_APART))
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
        for seed in (1..6).chain([crate::game::SEED]) {
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
                assert!(!comes_back(&stream.points), "seed {seed}: {} comes back past itself", stream.name);
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
        // No step: neighbours a centimeter apart never more than two centimeters apart in
        // height (the steepest underwater drop-off, where a shelf falls to a scoured run, is a
        // little over one in one), and none nearer the water, where the bed is shaped.
        for seed in [1, crate::game::SEED] {
        let terrain = Terrain::new(seed);
        let mut random = Random(5);
        for k in 0..80_000 {
            let (x, z) = if k % 2 == 0 {
                (random.between(-150.0, 150.0), random.between(-150.0, 150.0))
            } else if k % 4 == 1 {
                // In and round the basins.
                let basin = &terrain.basins[(random.unit() * terrain.basins.len() as f32) as usize % terrain.basins.len()];
                let reach = basin.reach();
                (basin.center.x + random.between(-reach, reach), basin.center.y + random.between(-reach, reach))
            } else {
                // By the water, every channel's, junctions and all.
                let channel = &terrain.channels[(random.unit() * terrain.channels.len() as f32) as usize % terrain.channels.len()];
                let p = channel.points[(random.unit() * (channel.points.len() - 1) as f32) as usize];
                (p.x + random.between(-3.0, 3.0), p.y + random.between(-0.5, 0.5))
            };
            let (a, b) = (terrain.height(x, z), terrain.height(x + 0.01, z));
            // (A basin's rim, where it is cut into rising ground, eases into it over the tool's
            // 19 centimeters: a bank up to four in one, but no step.)
            let in_basin = terrain.basins.iter().any(|basin| basin.polar(x, z).0 < BASIN_REACH + 0.05);
            let most = if in_basin { 0.05 } else { 0.02 };
            assert!((a - b).abs() < most, "seed {seed}: a step at {x}, {z}: {a} to {b}");
        }
        }
    }

    #[test]
    fn a_spring_heads_every_stream_and_ponds_lie_in_the_woods() {
        for seed in (1..6).chain([crate::game::SEED]) {
            let terrain = Terrain::new(seed);
            let springs: Vec<&Basin> = terrain.basins.iter().filter(|b| b.is_spring()).collect();
            assert_eq!(springs.len(), STREAMS, "seed {seed}");
            for spring in &springs {
                let (_, stream) = spring.outlet.unwrap();
                let channel = &terrain.channels[stream];
                assert_eq!(channel.points[0], spring.center);
                // Its water just over its stream's, and its middle deep under it.
                assert!(spring.level > channel.water(channel.points.len() - 1));
                let bed = terrain.height(spring.center.x, spring.center.y);
                assert!(spring.level - bed > SPRING_DEPTH * 0.8, "seed {seed}: {} only {} deep", spring.name, spring.level - bed);
                // Its outlet open: the stream's bed, out through the rim, under the spring's water.
                let out = spring.at(spring.outlet.unwrap().0, 1.1);
                assert!(terrain.height(out.x, out.y) < spring.level, "seed {seed}: {} dammed", spring.name);
            }
            let ponds: Vec<&Basin> = terrain.basins.iter().filter(|b| !b.is_spring()).collect();
            assert_eq!(ponds.len(), PONDS, "seed {seed}");
            assert!(ponds[0].center.norm() < 40.0, "seed {seed}: the first pond is {} m out", ponds[0].center.norm());
            for pond in &ponds {
                let bed = terrain.height(pond.center.x, pond.center.y);
                assert!(pond.level - bed > POND_DEPTH * 0.8, "seed {seed}: {} only {} deep", pond.name, pond.level - bed);
                assert!(terrain.wet(pond.center.x, pond.center.y, 0.0));
                // Closed: its rim above its water all the way round.
                for k in 0..36 {
                    let rim = pond.at(k as f32 * std::f32::consts::TAU / 36.0, 1.13);
                    assert!(terrain.height(rim.x, rim.y) > pond.level, "seed {seed}: {} spills at {rim:?}", pond.name);
                }
            }
        }
    }

    #[test]
    fn no_step_in_or_round_a_basin() {
        // Every basin, and round it, densely: the rims, the outlets, a stream's head.
        for seed in (1..6).chain([crate::game::SEED]) {
            let terrain = Terrain::new(seed);
            for basin in &terrain.basins {
                let r = basin.reach() + 1.0;
                for i in 0..160 {
                    for j in 0..160 {
                        let x = basin.center.x - r + 2.0 * r * i as f32 / 160.0;
                        let z = basin.center.y - r + 2.0 * r * j as f32 / 160.0;
                        let a = terrain.height(x, z);
                        for (dx, dz) in [(0.01, 0.0), (0.0, 0.01)] {
                            let b = terrain.height(x + dx, z + dz);
                            assert!((a - b).abs() < 0.05, "seed {seed}: a step by {} at {x}, {z}: {a} to {b}", basin.name);
                        }
                    }
                }
            }
        }
    }
}
