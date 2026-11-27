//! The forest floor: shrubs, ferns, grasses, wildflowers and granite rocks, among the trees.
//!
//! Each kind grows where it would: heather, grasses, poppies and broom in the glades; bilberry,
//! hard fern, wood sorrel and wood melick in the shade of the thickets; bracken and bluebells in
//! both. Plants of a kind gather in patches, as they spread, and the floor is thicker in the open
//! than under a dark canopy. Rocks lie scattered, and crowd together in a few stony places.
//!
//! The floor is laid out whole, from the seed, in square chunks; but a plant is put into the
//! scene only while the player is near enough to see it, and taken out again once left behind,
//! so that walking across the forest never piles up more plants than can be seen. How near
//! depends on its height: a hawthorn shows from far off, a tuft of fescue only from a few steps.
//! The models are detailed, thousands of triangles to a tuft, so this keeps the floor's share of
//! the frame small. Plants too low to cast a shadow worth tracing cast none.
//!
//! Over the soil between, a carpet of low growth - grasses, sorrel, heather and ferns, [`COVER`] -
//! grows thicker still, but only near the player: each tuft shows within
//! 4.5, 7 or 10 m of them ([`COVER_BANDS`]), more of them the nearer, so the carpet is whole
//! underfoot and thins out to the floor's own plants a few steps off, rather than ending at a line.
//! It casts no shadow, and stands still in the wind (see [`COVER_SWAYS`]): a carpet's shadows
//! hardly show, and tracing them, and bending thousands of tufts every frame, cost the most of it.

use crate::{
    forest::{self, Thicket, Tree},
    terrain::Terrain,
    models::{Prepared, mesh_of},
    random::{Noise, Random, smoothstep},
    wind::{self, Profile, Sway, Wind},
};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector2, Vector3},
        pool::Handle,
    },
    fxhash::FxHashMap,
    graph::SceneGraph,
    resource::model::{ModelResource, ModelResourceExtension},
    scene::{
        Scene,
        base::BaseBuilder,
        collider::{ColliderBuilder, ColliderShape},
        graph::Graph,
        node::Node,
        pivot::PivotBuilder,
        rigidbody::{RigidBodyBuilder, RigidBodyType},
        transform::TransformBuilder,
    },
};

/// A kind of plant or rock on the forest floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kind {
    pub name: &'static str,
    /// Its model.
    pub path: &'static str,
    /// How it sways, if it does.
    pub wind: Option<Profile>,
    /// How tall it is, in meters, at the model's size: how far off it is seen from.
    pub height: f32,
    /// How common it is in the open and in deep shade, against the others.
    pub open: f32,
    pub shade: f32,
    /// How wide its patches are, roughly, in meters.
    pub patch: f32,
    /// The range of its size, as a scale of the model.
    pub scale: (f32, f32),
    /// How far it spreads from where it stands, in meters, at the model's size.
    pub radius: f32,
}

const fn plant(
    name: &'static str,
    path: &'static str,
    wind: Profile,
    height: f32,
    (open, shade): (f32, f32),
    patch: f32,
    scale: (f32, f32),
    radius: f32,
) -> Kind {
    Kind {
        name,
        path,
        wind: Some(wind),
        height,
        open,
        shade,
        patch,
        scale,
        radius,
    }
}

/// Every plant of the forest floor.
pub const PLANTS: [Kind; 17] = [
    plant("Bilberry", "data/plants/bilberry.glb", wind::SHRUB, 0.67, (1.0, 5.0), 10.0, (0.7, 1.3), 0.38),
    plant("Common heather", "data/plants/common-heather.glb", wind::SHRUB, 0.6, (3.0, 0.3), 12.0, (0.7, 1.3), 0.5),
    plant("Common broom", "data/plants/common-broom.glb", wind::SHRUB, 1.9, (0.8, 0.05), 18.0, (0.7, 1.2), 1.1),
    plant("Common hawthorn", "data/plants/common-hawthorn.glb", wind::SHRUB, 2.8, (0.5, 0.15), 25.0, (0.7, 1.2), 1.4),
    plant("Dog rose", "data/plants/dog-rose.glb", wind::SHRUB, 1.95, (0.5, 0.1), 20.0, (0.7, 1.15), 1.3),
    plant("Burnet rose", "data/plants/burnet-rose.glb", wind::SHRUB, 0.87, (0.7, 0.05), 15.0, (0.7, 1.2), 0.85),
    plant("Bracken", "data/plants/bracken.glb", wind::FERN, 0.92, (4.0, 1.2), 14.0, (0.8, 1.4), 0.6),
    plant("Hard fern", "data/plants/hard-fern.glb", wind::FERN, 0.45, (0.4, 3.0), 8.0, (0.8, 1.3), 0.45),
    plant("Sheep fescue", "data/plants/sheep-fescue.glb", wind::GRASS, 0.35, (6.0, 0.8), 9.0, (0.8, 1.3), 0.27),
    plant("Tufted hairgrass", "data/plants/tufted-hairgrass.glb", wind::GRASS, 0.96, (3.0, 1.2), 11.0, (0.8, 1.25), 0.3),
    plant("Wood melick", "data/plants/wood-melick.glb", wind::GRASS, 0.52, (1.0, 4.0), 9.0, (0.8, 1.3), 0.3),
    plant("Meadow foxtail", "data/plants/meadow-foxtail.glb", wind::GRASS, 0.82, (4.0, 0.2), 10.0, (0.8, 1.2), 0.3),
    plant("Foxglove", "data/plants/foxglove.glb", wind::WILDFLOWER, 1.49, (1.2, 0.4), 12.0, (0.8, 1.2), 0.33),
    plant("Wood sorrel", "data/plants/wood-sorrel.glb", wind::WILDFLOWER, 0.19, (0.2, 3.0), 6.0, (0.9, 1.4), 0.2),
    plant("Oxeye daisy", "data/plants/oxeye-daisy.glb", wind::WILDFLOWER, 0.55, (2.0, 0.1), 9.0, (0.9, 1.2), 0.22),
    plant("English bluebell", "data/plants/english-bluebell.glb", wind::WILDFLOWER, 0.41, (0.6, 2.0), 8.0, (0.9, 1.3), 0.3),
    plant("Common poppy", "data/plants/common-poppy.glb", wind::WILDFLOWER, 0.68, (1.0, 0.0), 10.0, (0.9, 1.2), 0.3),
];

/// A kind of rock, and the box the player bumps into, if any: its half extents at the model's
/// size, and how much of its height is sunk into the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rock {
    pub kind: Kind,
    pub solid: Option<Vector3<f32>>,
    pub sink: f32,
}

const fn rock(name: &'static str, path: &'static str, height: f32, scale: (f32, f32), radius: f32) -> Kind {
    Kind {
        name,
        path,
        wind: None,
        height,
        open: 1.0,
        shade: 1.0,
        patch: 1.0,
        scale,
        radius,
    }
}

/// Every kind of rock.
pub const ROCKS: [Rock; 3] = [
    Rock {
        kind: rock("Granite boulder", "data/rocks/granite-boulder.glb", 0.95, (0.8, 2.6), 0.45),
        solid: Some(Vector3::new(0.4, 0.475, 0.32)),
        sink: 0.15,
    },
    Rock {
        kind: rock("Granite slab", "data/rocks/granite-slab.glb", 0.32, (0.8, 2.2), 0.66),
        solid: Some(Vector3::new(0.6, 0.16, 0.34)),
        sink: 0.1,
    },
    Rock {
        kind: rock("Granite cobble", "data/rocks/granite-cobble.glb", 0.18, (0.6, 1.8), 0.16),
        solid: None,
        sink: 0.2,
    },
];

/// Every kind on the floor, plants then rocks: what [`Placement::kind`] counts through.
pub fn kinds() -> impl Iterator<Item = &'static Kind> {
    PLANTS.iter().chain(ROCKS.iter().map(|rock| &rock.kind))
}

/// How wide a chunk is, in meters: small, so that the nearest band, of the smallest plants,
/// reaches little further than it should; and a whole share of the traced shadows' batches of
/// still meshes, so that a chunk coming or going puts only one of them together again.
const CHUNK: f32 = 8.0;
/// How far off a plant is seen, in meters for each meter of its height, and no nearer or
/// further than this.
const REACH_PER_HEIGHT: f32 = 36.0;
const REACH: (f32, f32) = (10.0, 75.0);
/// The distances plants are put into the scene within, in meters, each plant at the first not
/// short of its reach: the plants of a chunk come and go together, a band at a time. The first
/// [`COVER_BANDS`] are the ground cover's alone.
const BANDS: [f32; 8] = [4.5, 7.0, 10.0, 12.0, 20.0, 32.0, 48.0, 75.0];
/// The nearest of the [`BANDS`] the floor's own plants show in: the ground cover's are nearer.
const NEAREST_PLANTS: f32 = 12.0;
/// How many of the [`BANDS`] the ground cover grows in, nearest first, and the share of it in
/// each: the most underfoot, fewer further off.
const COVER_BANDS: usize = 3;
const COVER_SHARES: [f32; COVER_BANDS] = [0.5, 0.3, 0.2];
/// How many of the cover's bands, nearest first, sway in the wind; the rest stand still. None:
/// a tuft swaying is bent anew every frame, for Yoctite to draw and for Candelite's rays and the
/// shadows' to trace - swaying in the nearest band alone took the frame from about 11 ms of the
/// GPU's to 14, all of it to 17 (30 FPS). Standing still, the carpet costs under a millisecond,
/// and the floor's own plants in it - bracken, bilberry, bluebells - still sway.
const COVER_SWAYS: usize = 0;
/// The ground cover's kinds: low, as thick as a carpet, and light - a few thousand triangles a
/// tuft at most, drawn whole so near. Bilberry (7,600) and bluebells (5,000) stay the floor's
/// own: in the carpet they took the frame past 60 FPS's. Each grows where it would (see
/// [`Kind::open`]), and it keeps clear of trunks, rocks and the water as the rest of the floor.
const COVER: [&str; 6] = [
    "Sheep fescue",
    "Tufted hairgrass",
    "Wood melick",
    "Wood sorrel",
    "Common heather",
    "Hard fern",
];
/// How many tufts of ground cover grow on a square meter at most; and how small they are, as a
/// share of their kind's size: low, under the floor's own plants. Twice as thick took Yoctite's
/// drawing from 2.5 ms to 5.4, the frame past 60 FPS's.
const MOST_COVER: f32 = 2.0;
const COVER_SCALE: f32 = 0.8;
/// How far from the water's edge plants keep, in meters; and the share of rocks that may lie in
/// the water.
const PLANTS_OFF_WATER: f32 = 0.1;
const ROCKS_IN_WATER: f32 = 0.25;
/// About how many meters of river there are to each boulder standing in it.
const RIVER_BOULDER_EVERY: f32 = 30.0;
/// How much further than its band a plant is taken out again, in meters: walking back and forth
/// over the line does not put it in and take it out over and over.
const LINGER: f32 = 6.0;
/// Between these distances from the player, in meters, a floor plant's sway fades out; past them
/// it stands still. Too small to see that far off, the sway of thousands of plants was posed
/// every update - and every one of them bent again for the renderer and the shadows.
const SWAY: (f32, f32) = (25.0, 40.0);
/// How many plants and rocks are put into the scene in a frame, at most, walking about.
const SPAWNS_PER_FRAME: usize = 400;
/// Lower than this, in meters, a plant casts no shadow: with traced shadows, and with shadow
/// maps, which draw every caster again for each of the sun's maps.
const SHADOW_HEIGHT: (f32, f32) = (0.5, 1.2);
/// How many plants grow on a square meter at most, in a lush glade.
const MOST_PLANTS: f32 = 2.4;
/// How many rocks lie on a square meter at most, in the stoniest place.
const MOST_ROCKS: f32 = 0.07;

/// A plant or rock on the floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Which of [`kinds`] it is.
    pub kind: usize,
    pub position: Vector3<f32>,
    pub yaw: f32,
    pub scale: f32,
    /// How much of the wind reaches it.
    pub shelter: f32,
    /// For ground cover, which of the first [`COVER_BANDS`] it shows in; [`None`] for the rest,
    /// which show by their height.
    pub cover: Option<usize>,
}

impl Placement {
    pub fn kind(&self) -> &'static Kind {
        kinds().nth(self.kind).unwrap()
    }

    /// How tall it stands, in meters.
    fn height(&self) -> f32 {
        self.kind().height * self.scale
    }

    /// Which of the [`BANDS`] it comes and goes in: the ground cover's own, or the first past
    /// those not short of its reach.
    fn band(&self) -> usize {
        if let Some(band) = self.cover {
            return band;
        }
        let reach = (REACH_PER_HEIGHT * self.height()).clamp(REACH.0, REACH.1);
        let reach = (reach * 0.85).max(NEAREST_PLANTS);
        BANDS.iter().position(|band| *band >= reach).unwrap_or(BANDS.len() - 1)
    }

    fn rock(&self) -> Option<&'static Rock> {
        self.kind.checked_sub(PLANTS.len()).map(|i| &ROCKS[i])
    }
}

/// One chunk of the floor: its plants and rocks, by band, and those in the scene.
#[derive(Debug, Default, PartialEq)]
struct Chunk {
    middle: Vector2<f32>,
    placements: Vec<Placement>,
    bands: [Vec<Placement>; BANDS.len()],
    spawned: [Option<Spawned>; BANDS.len()],
}

/// A band of a chunk's plants in the scene.
#[derive(Debug, PartialEq)]
struct Spawned {
    root: Handle<Node>,
    sways: Vec<Sway>,
}

/// The whole forest floor.
#[derive(Debug, PartialEq)]
pub struct Floor {
    chunks: Vec<Chunk>,
    /// Every kind's model, in the order of [`kinds`], all loaded, and its surfaces seen from
    /// both sides.
    models: Vec<ModelResource>,
    double_sided: Vec<Vec<bool>>,
    /// Swaying the plants, with their own randomness.
    wind: bool,
    random: Random,
}

impl Floor {
    /// Lays out the floor among `trees`, on `terrain`'s ground and out of its water. The same
    /// seed lays out the same floor.
    pub fn lay_out(
        seed: u64,
        terrain: &Terrain,
        trees: &[Tree],
        models: Vec<ModelResource>,
        double_sided: Vec<Vec<bool>>,
        wind: bool,
    ) -> Self {
        Self {
            chunks: lay_out(seed, terrain, trees),
            models,
            double_sided,
            wind,
            random: Random::for_place(seed, 0xf1, 0x00),
        }
    }

    /// Leaves out the kinds whose models did not load.
    pub fn retain_loaded(&mut self) {
        let models = self.models.clone();
        self.retain(|p| models[p.kind].is_ok());
    }

    /// Keeps only the plants and rocks `keep` says to.
    pub fn retain(&mut self, keep: impl Fn(&Placement) -> bool) {
        for chunk in &mut self.chunks {
            chunk.placements.retain(&keep);
            for band in &mut chunk.bands {
                band.retain(&keep);
            }
        }
    }

    /// How many plants and rocks there are, everywhere.
    pub fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.placements.len()).sum()
    }

    /// A static body with a collider for every rock big enough to bump into.
    pub fn build_rocks(&self, graph: &mut Graph) -> Handle<Node> {
        let mut body = BaseBuilder::new().with_name("Rocks");
        for placement in self.chunks.iter().flat_map(|c| &c.placements) {
            let Some((rock, solid)) = placement.rock().and_then(|r| Some((r, r.solid?))) else {
                continue;
            };
            let half = solid * placement.scale;
            let height = 2.0 * half.y;
            let collider = ColliderBuilder::new(
                BaseBuilder::new().with_local_transform(
                    TransformBuilder::new()
                        .with_local_position(placement.position + Vector3::new(0.0, half.y - rock.sink * height, 0.0))
                        .with_local_rotation(UnitQuaternion::from_axis_angle(&Vector3::y_axis(), placement.yaw))
                        .build(),
                ),
            )
            .with_shape(ColliderShape::cuboid(half.x, half.y, half.z))
            .build(graph);
            body = body.with_child(collider);
        }
        RigidBodyBuilder::new(body)
            .with_body_type(RigidBodyType::Static)
            .build(graph)
            .to_base()
    }

    /// The rocks' bounds, least corner and most: their solid shapes' (as `build_rocks` makes
    /// them, turned with them), or for a cobble, which has none, its spread and height. For the
    /// water to ripple round those that stand in it.
    pub fn rock_bounds(&self) -> Vec<(Vector3<f32>, Vector3<f32>)> {
        self.chunks
            .iter()
            .flat_map(|c| &c.placements)
            .filter_map(|placement| {
                let rock = placement.rock()?;
                let (sin, cos) = (placement.yaw.sin().abs(), placement.yaw.cos().abs());
                let (half, bottom) = match rock.solid {
                    Some(solid) => {
                        let half = solid * placement.scale;
                        (Vector3::new(cos * half.x + sin * half.z, half.y, sin * half.x + cos * half.z), -rock.sink * 2.0 * half.y)
                    }
                    None => {
                        let (radius, height) = (rock.kind.radius * placement.scale, rock.kind.height * placement.scale);
                        (Vector3::new(radius, 0.5 * height, radius), -rock.sink * height)
                    }
                };
                let low = placement.position + Vector3::new(-half.x, bottom, -half.z);
                Some((low, low + Vector3::new(2.0 * half.x, 2.0 * half.y, 2.0 * half.z)))
            })
            .collect()
    }

    /// Puts the plants near enough `player` into the scene and takes far ones out (at most a
    /// few hundred a frame, unless `all`), and sways those in it.
    pub fn update(
        &mut self,
        scene: &mut Scene,
        prepared: &mut Prepared,
        player: Vector3<f32>,
        wind: &Wind,
        dt: f32,
        all: bool,
    ) {
        let distance = |chunk: &Chunk| {
            let d = (chunk.middle - player.xz()).abs() - Vector2::repeat(CHUNK / 2.0);
            d.sup(&Vector2::zeros()).norm()
        };
        // Nearest first, by how far inside its band each is.
        let mut wanted: Vec<(f32, usize, usize)> = Vec::new();
        for (index, chunk) in self.chunks.iter().enumerate() {
            let d = distance(chunk);
            for (band, reach) in BANDS.iter().enumerate() {
                if d < *reach && chunk.spawned[band].is_none() && !chunk.bands[band].is_empty() {
                    wanted.push((d / reach, index, band));
                }
            }
        }
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut budget = if all { usize::MAX } else { SPAWNS_PER_FRAME };
        for (_, index, band) in wanted {
            let count = self.chunks[index].bands[band].len();
            if count > budget {
                break;
            }
            budget -= count;
            let spawned = self.spawn(scene, prepared, index, band);
            self.chunks[index].spawned[band] = Some(spawned);
        }

        let graph = &mut scene.graph;
        for chunk in &mut self.chunks {
            let d = distance(chunk);
            for (band, spawned) in chunk.spawned.iter_mut().enumerate() {
                let Some(group) = spawned else {
                    continue;
                };
                if d > BANDS[band] + LINGER {
                    graph.remove_node(group.root);
                    *spawned = None;
                    continue;
                }
                for sway in &mut group.sways {
                    let distance = (sway.at() - player.xz()).norm();
                    let shown = 1.0 - crate::random::smoothstep(SWAY.0, SWAY.1, distance);
                    sway.update(wind, dt, graph, shown, wind::detail_at(distance), wind::every_at(distance));
                }
            }
        }
    }

    /// How many plants and rocks are in the scene.
    pub fn spawned(&self) -> usize {
        self.chunks
            .iter()
            .flat_map(|c| c.spawned.iter().zip(&c.bands))
            .filter(|(spawned, _)| spawned.is_some())
            .map(|(_, band)| band.len())
            .sum()
    }

    fn spawn(&mut self, scene: &mut Scene, prepared: &mut Prepared, index: usize, band: usize) -> Spawned {
        let chunk = &self.chunks[index];
        let mut roots = Vec::new();
        let mut sways = Vec::new();
        for placement in &chunk.bands[band] {
            let kind = placement.kind();
            let root = self.models[placement.kind].instantiate(scene);
            let graph = &mut scene.graph;
            let sink = placement.rock().map_or(0.0, |rock| rock.sink * placement.height());
            graph[root]
                .local_transform_mut()
                .set_position(placement.position - Vector3::new(0.0, sink, 0.0))
                .set_rotation(UnitQuaternion::from_axis_angle(&Vector3::y_axis(), placement.yaw))
                .set_scale(Vector3::repeat(placement.scale));
            prepared.prepare(graph, root, &self.double_sided[placement.kind], self.wind);
            let shadow_height = if crate::traced_shadows() { SHADOW_HEIGHT.0 } else { SHADOW_HEIGHT.1 };
            if placement.height() < shadow_height || placement.cover.is_some() {
                let nodes: Vec<Handle<Node>> = graph.traverse_handle_iter(root).collect();
                for node in nodes {
                    graph[node].set_cast_shadows(false);
                }
            }
            // Of the ground cover, only the tufts in its first COVER_SWAYS bands sway.
            let swaying = placement.cover.is_none_or(|band| band < COVER_SWAYS);
            if let (true, Some(profile), true) = (self.wind, kind.wind, swaying) {
                sways.extend(mesh_of(graph, root).and_then(|mesh| {
                    Sway::new(
                        graph,
                        mesh,
                        placement.position,
                        placement.yaw,
                        profile,
                        placement.shelter,
                        &mut self.random,
                    )
                }));
            }
            roots.push(root);
        }
        let graph = &mut scene.graph;
        let root = PivotBuilder::new(roots.into_iter().fold(BaseBuilder::new().with_name("Floor"), BaseBuilder::with_child))
            .build(graph)
            .to_base();
        Spawned { root, sways }
    }
}

/// How far the floor reaches from the middle along x and z: to the walls round the ground.
fn extent() -> f32 {
    forest::HALF_EXTENT + crate::game::BORDER - 1.0
}

/// Things that stand in the way of a new plant or rock: tree trunks and rocks, by where they
/// stand and how far round them is taken.
struct Obstacles {
    cells: FxHashMap<(i32, i32), Vec<(Vector2<f32>, f32)>>,
}

impl Obstacles {
    /// How wide a cell is, in meters: wider than anything is round.
    const CELL: f32 = 4.0;

    fn cell(p: Vector2<f32>) -> (i32, i32) {
        ((p.x / Self::CELL).floor() as i32, (p.y / Self::CELL).floor() as i32)
    }

    fn add(&mut self, at: Vector2<f32>, radius: f32) {
        self.cells.entry(Self::cell(at)).or_default().push((at, radius.min(Self::CELL)));
    }

    /// Whether something `radius` round at `at` would stand in another's way.
    fn blocked(&self, at: Vector2<f32>, radius: f32) -> bool {
        let (cx, cz) = Self::cell(at);
        (cx - 1..=cx + 1).any(|x| {
            (cz - 1..=cz + 1).any(|z| {
                self.cells.get(&(x, z)).is_some_and(|things| {
                    things.iter().any(|(p, r)| (p - at).norm_squared() < (r + radius) * (r + radius))
                })
            })
        })
    }
}

/// Lays out every chunk's plants and rocks among `trees`, on `terrain`: the plants out of the
/// water, the rocks mostly.
fn lay_out(seed: u64, terrain: &Terrain, trees: &[Tree]) -> Vec<Chunk> {
    let extent = extent();
    let across = (2.0 * extent / CHUNK).ceil() as usize;
    let thicket = Thicket::new(seed);
    let clumps = Noise(seed ^ 0x51c3_77aa);
    let stony = Noise(seed ^ 0x0b01_d3e5);
    let patches: Vec<Noise> = (0..PLANTS.len())
        .map(|k| Noise(seed ^ (k as u64 + 1).wrapping_mul(0x2545_f491_4f6c_dd1d)))
        .collect();

    let mut obstacles = Obstacles { cells: FxHashMap::default() };
    for tree in trees {
        let species = &forest::SPECIES[tree.species];
        obstacles.add(tree.position.xz(), species.trunk * tree.scale + 0.1);
    }

    let mut chunks: Vec<Chunk> = (0..across * across)
        .map(|i| Chunk {
            middle: Vector2::new(
                -extent + ((i % across) as f32 + 0.5) * CHUNK,
                -extent + ((i / across) as f32 + 0.5) * CHUNK,
            ),
            ..Default::default()
        })
        .collect();
    let inside = |p: Vector2<f32>| p.x.abs() < extent && p.y.abs() < extent;
    let spot = |random: &mut Random, middle: Vector2<f32>| {
        middle + Vector2::new(random.between(-0.5, 0.5), random.between(-0.5, 0.5)) * CHUNK
    };

    // The rocks first, everywhere, so that the plants can keep out of them.
    for (index, chunk) in chunks.iter_mut().enumerate() {
        let mut random = Random::for_place(seed, index as i64, 1);
        for _ in 0..(MOST_ROCKS * CHUNK * CHUNK).round() as usize {
            let at = spot(&mut random, chunk.middle);
            let stones = smoothstep(0.55, 0.8, stony.at(at.x / 35.0, at.y / 35.0));
            if !inside(at) || at.norm() < 4.0 || random.unit() * MOST_ROCKS > 0.003 + 0.067 * stones {
                continue;
            }
            // Boulders and slabs mostly where it is stony; cobbles anywhere.
            let Some(rock) = random.pick(&[0.12 + 0.3 * stones, 0.15 + 0.2 * stones, 0.5]) else {
                continue;
            };
            let kind = &ROCKS[rock].kind;
            let scale = random.between(kind.scale.0, kind.scale.1);
            let radius = kind.radius * scale * 0.8;
            // A rock in the water now and then, as stones lie in a stream bed.
            if obstacles.blocked(at, radius) || (terrain.wet(at.x, at.y, 0.0) && random.unit() > ROCKS_IN_WATER) {
                continue;
            }
            obstacles.add(at, radius);
            chunk.placements.push(Placement {
                kind: PLANTS.len() + rock,
                position: Vector3::new(at.x, terrain.height(at.x, at.y), at.y),
                yaw: random.between(0.0, std::f32::consts::TAU),
                scale,
                shelter: 1.0,
                cover: None,
            });
        }
    }

    shores(seed, terrain, &thicket, &mut obstacles, &mut chunks, extent, across);

    for (index, chunk) in chunks.iter_mut().enumerate() {
        let mut random = Random::for_place(seed, index as i64, 2);
        for _ in 0..(MOST_PLANTS * CHUNK * CHUNK).round() as usize {
            let at = spot(&mut random, chunk.middle);
            if !inside(at) || at.norm() < 1.0 || terrain.wet(at.x, at.y, PLANTS_OFF_WATER) {
                continue;
            }
            let thickness = thicket.at(at.x, at.y);
            // Thicker in the open than in the shade, and in clumps.
            let density = (0.35 + 1.1 * (1.0 - thickness)) * (0.35 + 1.3 * clumps.at(at.x / 7.0, at.y / 7.0));
            if random.unit() * MOST_PLANTS > density {
                continue;
            }
            let weights: Vec<f32> = PLANTS
                .iter()
                .zip(&patches)
                .map(|(kind, patches)| {
                    let habitat = kind.open + (kind.shade - kind.open) * thickness;
                    let patch = smoothstep(0.45, 0.75, patches.at(at.x / kind.patch, at.y / kind.patch));
                    habitat * (0.08 + 2.0 * patch)
                })
                .collect();
            let Some(plant) = random.pick(&weights) else {
                continue;
            };
            let kind = &PLANTS[plant];
            let scale = random.between(kind.scale.0, kind.scale.1);
            if obstacles.blocked(at, 0.3 * kind.radius * scale) {
                continue;
            }
            chunk.placements.push(Placement {
                kind: plant,
                position: Vector3::new(at.x, terrain.height(at.x, at.y), at.y),
                yaw: random.between(0.0, std::f32::consts::TAU),
                scale,
                shelter: 1.0 - 0.6 * thickness,
                cover: None,
            });
        }
    }
    // WILDERNESS_COVER=0 leaves the ground cover out.
    if std::env::var("WILDERNESS_COVER").as_deref() != Ok("0") {
        ground_cover(seed, terrain, &thicket, &clumps, &patches, &obstacles, &mut chunks);
    }
    for chunk in &mut chunks {
        for placement in &chunk.placements {
            chunk.bands[placement.band()].push(*placement);
        }
    }
    chunks
}

/// The ground cover over the soil (see the module's description): as thick as [`MOST_COVER`] in
/// the open, thinner in the shade, in clumps, each kind in its patches - those of the floor's own
/// plants, so the carpet is of the plants growing over it.
fn ground_cover(
    seed: u64,
    terrain: &Terrain,
    thicket: &Thicket,
    clumps: &Noise,
    patches: &[Noise],
    obstacles: &Obstacles,
    chunks: &mut [Chunk],
) {
    let extent = extent();
    let kinds: Vec<usize> = COVER
        .iter()
        .map(|name| PLANTS.iter().position(|kind| kind.name == *name).expect("a floor plant"))
        .collect();
    for (index, chunk) in chunks.iter_mut().enumerate() {
        let mut random = Random::for_place(seed, index as i64, 3);
        for _ in 0..(MOST_COVER * CHUNK * CHUNK).round() as usize {
            let at = chunk.middle + Vector2::new(random.between(-0.5, 0.5), random.between(-0.5, 0.5)) * CHUNK;
            if at.x.abs() >= extent || at.y.abs() >= extent || at.norm() < 1.0 || terrain.wet(at.x, at.y, PLANTS_OFF_WATER) {
                continue;
            }
            let thickness = thicket.at(at.x, at.y);
            // The share of the most there is room for that grows here: thinner in the shade, and
            // in clumps.
            let share = ((0.35 + 0.65 * (1.0 - thickness)) * (0.4 + 0.8 * clumps.at(at.x / 5.0, at.y / 5.0))).min(1.0);
            if random.unit() > share {
                continue;
            }
            let weights: Vec<f32> = kinds
                .iter()
                .map(|&k| {
                    let kind = &PLANTS[k];
                    let habitat = kind.open + (kind.shade - kind.open) * thickness;
                    habitat * (0.3 + smoothstep(0.4, 0.75, patches[k].at(at.x / kind.patch, at.y / kind.patch)))
                })
                .collect();
            let Some(pick) = random.pick(&weights) else {
                continue;
            };
            let kind = &PLANTS[kinds[pick]];
            let scale = COVER_SCALE * random.between(kind.scale.0, kind.scale.1);
            if obstacles.blocked(at, 0.3 * kind.radius * scale) {
                continue;
            }
            let Some(band) = random.pick(&COVER_SHARES) else {
                continue;
            };
            chunk.placements.push(Placement {
                kind: kinds[pick],
                position: Vector3::new(at.x, terrain.height(at.x, at.y), at.y),
                yaw: random.between(0.0, std::f32::consts::TAU),
                scale,
                shelter: 1.0 - 0.6 * thickness,
                cover: Some(band),
            });
        }
    }
}

/// The shores, as the Birch Glade scatters them along its seeded waterlines: gravel in uneven bars
/// along each bank, some of it in the water, with gaps between; hard fern and wood melick in
/// clumps a little up the bank where the gravel is thickest; and now and then a boulder standing
/// in the river.
fn shores(seed: u64, terrain: &Terrain, thicket: &Thicket, obstacles: &mut Obstacles, chunks: &mut [Chunk], extent: f32, across: usize) {
    let index_of = |kind: &str| kinds().position(|k| k.name == kind).expect("a floor kind");
    let (cobble, boulder) = (index_of("Granite cobble"), index_of("Granite boulder"));
    let (fern, melick) = (index_of("Hard fern"), index_of("Wood melick"));
    let place = |chunks: &mut [Chunk], kind: usize, at: Vector2<f32>, yaw: f32, scale: f32| {
        if at.x.abs() >= extent || at.y.abs() >= extent {
            return;
        }
        let column = (((at.x + extent) / CHUNK) as usize).min(across - 1);
        let row = (((at.y + extent) / CHUNK) as usize).min(across - 1);
        chunks[row * across + column].placements.push(Placement {
            kind,
            position: Vector3::new(at.x, terrain.height(at.x, at.y), at.y),
            yaw,
            scale,
            shelter: 1.0 - 0.6 * thicket.at(at.x, at.y),
            cover: None,
        });
    };
    for (c, channel) in terrain.channels.iter().enumerate() {
        let total = *channel.distances.last().expect("a channel has points");
        for side in 0..2 {
            let mut random = Random::for_place(seed, c as i64, 0x5407 + side as i64);
            let phase = random.between(0.0, std::f32::consts::TAU);
            for (i, &middle) in channel.points.iter().enumerate() {
                let along = channel.distances[i];
                // Not within the river where a stream has run into it, nor at a channel's ends.
                if along < 0.5 || along > total - 0.65 || (c > 0 && terrain.near(0, middle.x, middle.y).is_some_and(|n| n.from_waterline() < 0.5)) {
                    continue;
                }
                let patch = 0.50 + 0.30 * (0.72 * along + phase).sin() + 0.20 * (1.83 * along - phase).sin();
                if random.unit() > 0.12 + 0.70 * patch {
                    continue;
                }
                let outward = crate::terrain::side_of(&channel.points, i) * if side == 0 { -1.0 } else { 1.0 };
                let edge = middle + outward * channel.waterlines[i][side];
                for _ in 0..1 + (random.unit() * 3.0) as usize {
                    let at = edge
                        + outward * random.between(-0.31, 0.58)
                        + Vector2::new(random.between(-0.2, 0.2), random.between(-0.2, 0.2));
                    let size = random.between(0.25, 0.79).powi(2);
                    let yaw = random.between(0.0, std::f32::consts::TAU);
                    obstacles.add(at, ROCKS[2].kind.radius * size * 0.8);
                    place(chunks, cobble, at, yaw, size);
                }
                if patch > 0.56 && random.unit() < 0.48 {
                    let at = edge + outward * random.between(0.54, 1.25);
                    let kind = if random.unit() < 0.4 { fern } else { melick };
                    let scale = random.between(0.42, 0.86);
                    let yaw = random.between(0.0, std::f32::consts::TAU);
                    if !terrain.wet(at.x, at.y, PLANTS_OFF_WATER) && !obstacles.blocked(at, 0.3 * PLANTS[kind].radius * scale) {
                        place(chunks, kind, at, yaw, scale);
                    }
                }
            }
        }
    }
    // The ponds' and springs' wet margins, as the Birch Glade dresses them: round every other
    // point of the water's edge, a little in or out, a cobble now and then, else hard fern, wood
    // melick or heather (not in the water); and at each spring, three boulders behind it, away
    // from its outlet, standing in its water.
    let heather = index_of("Common heather");
    for (b, basin) in terrain.basins.iter().enumerate() {
        let mut random = Random::for_place(seed, b as i64, 0xba51);
        for i in (1..=72).step_by(2) {
            let angle = std::f32::consts::TAU * (i - 1) as f32 / 72.0;
            let edge = basin.at(angle, crate::terrain::BASIN_WATER);
            let at = basin.center + (edge - basin.center) * random.between(0.86, 1.14);
            let yaw = random.between(0.0, std::f32::consts::TAU);
            if random.unit() < 0.30 {
                let size = random.between(0.26, 0.69);
                obstacles.add(at, ROCKS[2].kind.radius * size * 0.8);
                place(chunks, cobble, at, yaw, size);
            } else {
                let kind = [fern, melick, melick, heather][(random.unit() * 4.0) as usize % 4];
                let scale = random.between(0.64, 1.08);
                if !terrain.wet(at.x, at.y, 0.0) && !obstacles.blocked(at, 0.3 * PLANTS[kind].radius * scale) {
                    place(chunks, kind, at, yaw, scale);
                }
            }
        }
        if let Some((outlet, _)) = basin.outlet {
            // The Birch Glade's, its outlet toward +x, turned to this one's.
            let (sin, cos) = (outlet.sin(), outlet.cos());
            for (dx, dz, scale) in [(-1.07, -0.35, 0.82), (-0.98, 0.42, 0.67), (-0.64, 0.63, 0.52)] {
                let at = basin.center + Vector2::new(dx * cos - dz * sin, dx * sin + dz * cos);
                obstacles.add(at, ROCKS[0].kind.radius * scale);
                place(chunks, boulder, at, random.between(0.0, std::f32::consts::TAU), scale);
            }
        }
    }
    // Boulders in the river, about one every RIVER_BOULDER_EVERY meters, in its water.
    let river = &terrain.channels[0];
    let mut random = Random::for_place(seed, 0, 0xb01d);
    for (i, &middle) in river.points.iter().enumerate() {
        if random.unit() * RIVER_BOULDER_EVERY > 1.0 {
            continue;
        }
        let side = (random.unit() < 0.5) as usize;
        let outward = crate::terrain::side_of(&river.points, i) * if side == 0 { -1.0 } else { 1.0 };
        let at = middle + outward * river.waterlines[i][side] * random.between(0.2, 0.7);
        let scale = random.between(0.65, 0.98);
        let yaw = random.between(0.0, std::f32::consts::TAU);
        obstacles.add(at, ROCKS[0].kind.radius * scale);
        place(chunks, boulder, at, yaw, scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_floor() {
        let terrain = Terrain::new(2);
        let trees = forest::lay_out(2, &terrain);
        let a: Vec<Placement> = lay_out(2, &terrain, &trees).into_iter().flat_map(|c| c.placements).collect();
        let b: Vec<Placement> = lay_out(2, &terrain, &trees).into_iter().flat_map(|c| c.placements).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn floor_keeps_off_trunks_and_grows_every_kind() {
        let terrain = Terrain::new(1);
        let trees = forest::lay_out(1, &terrain);
        let placements: Vec<Placement> = lay_out(1, &terrain, &trees).into_iter().flat_map(|c| c.placements).collect();
        let extent = extent();
        for p in &placements {
            assert!(p.position.x.abs() < extent && p.position.z.abs() < extent);
            for tree in &trees {
                let trunk = forest::SPECIES[tree.species].trunk * tree.scale;
                assert!((tree.position.xz() - p.position.xz()).norm() >= trunk, "{} in a trunk", p.kind().name);
            }
        }
        for (index, kind) in kinds().enumerate() {
            let count = placements.iter().filter(|p| p.kind == index).count();
            assert!(count > 20, "only {count} {}", kind.name);
            let band = placements.iter().find(|p| p.kind == index).unwrap().band();
            assert!(band < BANDS.len());
        }
        let area = (2.0 * extent) * (2.0 * extent);
        let density = placements.iter().filter(|p| p.cover.is_none()).count() as f32 / area;
        assert!((0.3..1.5).contains(&density), "{density} a square meter");
    }

    #[test]
    fn ground_cover_is_low_thick_and_only_near() {
        let terrain = Terrain::new(1);
        let trees = forest::lay_out(1, &terrain);
        let chunks = lay_out(1, &terrain, &trees);
        let cover: Vec<&Placement> = chunks.iter().flat_map(|c| &c.placements).filter(|p| p.cover.is_some()).collect();
        let extent = extent();
        let density = cover.len() as f32 / ((2.0 * extent) * (2.0 * extent));
        assert!((0.4..MOST_COVER).contains(&density), "{density} tufts a square meter");
        for p in &cover {
            assert!(COVER.contains(&p.kind().name), "{} is no ground cover", p.kind().name);
            assert!(p.band() < COVER_BANDS && p.height() < 1.0, "{} {} m tall, band {}", p.kind().name, p.height(), p.band());
            for tree in &trees {
                let trunk = forest::SPECIES[tree.species].trunk * tree.scale;
                assert!((tree.position.xz() - p.position.xz()).norm() >= trunk, "{} in a trunk", p.kind().name);
            }
        }
        // More of it underfoot than further off.
        let in_band = |b: usize| cover.iter().filter(|p| p.band() == b).count();
        assert!(in_band(0) > in_band(1) && in_band(1) > in_band(2));
        // The rest of the floor shows from as far as it did: none in the cover's bands.
        let rest = chunks.iter().flat_map(|c| &c.placements).filter(|p| p.cover.is_none());
        assert!(rest.clone().all(|p| p.band() >= COVER_BANDS && BANDS[p.band()] >= NEAREST_PLANTS));
    }

    #[test]
    #[ignore = "prints how many of each kind grow"]
    fn counts() {
        let terrain = Terrain::new(1);
        let trees = forest::lay_out(1, &terrain);
        let placements: Vec<Placement> = lay_out(1, &terrain, &trees).into_iter().flat_map(|c| c.placements).collect();
        for (index, kind) in kinds().enumerate() {
            println!("{}: {}", kind.name, placements.iter().filter(|p| p.kind == index).count());
        }
        println!("all: {}", placements.len());
    }

    #[test]
    fn gravel_lines_the_shores_and_boulders_stand_in_the_river() {
        let terrain = Terrain::new(1);
        let trees = forest::lay_out(1, &terrain);
        let placements: Vec<Placement> = lay_out(1, &terrain, &trees).into_iter().flat_map(|c| c.placements).collect();
        let named = |p: &Placement, name: &str| p.kind().name == name;
        let at_shore = |p: &Placement| {
            terrain.channels.iter().enumerate().any(|(c, _)| {
                terrain.near(c, p.position.x, p.position.z).is_some_and(|n| n.from_waterline().abs() < 1.0)
            })
        };
        let shore_gravel = placements.iter().filter(|p| named(p, "Granite cobble") && at_shore(p)).count();
        let river_length = *terrain.channels[0].distances.last().unwrap();
        assert!(shore_gravel as f32 > river_length, "only {shore_gravel} cobbles along {river_length} m of river");
        let in_river = placements
            .iter()
            .filter(|p| named(p, "Granite boulder") && terrain.near(0, p.position.x, p.position.z).is_some_and(|n| n.from_waterline() < 0.0))
            .count();
        assert!(in_river >= 4, "{in_river} boulders in the river");
        // No plant in the water.
        for p in placements.iter().filter(|p| p.kind < PLANTS.len()) {
            assert!(!terrain.wet(p.position.x, p.position.z, 0.0), "{} in the water at {:?}", p.kind().name, p.position);
        }
    }
}
