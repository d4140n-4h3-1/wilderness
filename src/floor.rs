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

use crate::{
    forest::{self, Thicket, Tree},
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
/// short of its reach: the plants of a chunk come and go together, a band at a time.
const BANDS: [f32; 5] = [12.0, 20.0, 32.0, 48.0, 75.0];
/// How much further than its band a plant is taken out again, in meters: walking back and forth
/// over the line does not put it in and take it out over and over.
const LINGER: f32 = 6.0;
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
}

impl Placement {
    pub fn kind(&self) -> &'static Kind {
        kinds().nth(self.kind).unwrap()
    }

    /// How tall it stands, in meters.
    fn height(&self) -> f32 {
        self.kind().height * self.scale
    }

    /// Which of the [`BANDS`] it comes and goes in.
    fn band(&self) -> usize {
        let reach = (REACH_PER_HEIGHT * self.height()).clamp(REACH.0, REACH.1);
        BANDS.iter().position(|band| *band >= reach * 0.85).unwrap_or(BANDS.len() - 1)
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
    /// Lays out the floor among `trees`. The same seed lays out the same floor.
    pub fn lay_out(seed: u64, trees: &[Tree], models: Vec<ModelResource>, double_sided: Vec<Vec<bool>>, wind: bool) -> Self {
        Self {
            chunks: lay_out(seed, trees),
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
                    let detail = wind::detail_at((sway.at() - player.xz()).norm());
                    sway.update(wind, dt, graph, 1.0, detail);
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
            if placement.height() < shadow_height {
                let nodes: Vec<Handle<Node>> = graph.traverse_handle_iter(root).collect();
                for node in nodes {
                    graph[node].set_cast_shadows(false);
                }
            }
            if let (true, Some(profile)) = (self.wind, kind.wind) {
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

/// Lays out every chunk's plants and rocks among `trees`.
fn lay_out(seed: u64, trees: &[Tree]) -> Vec<Chunk> {
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
            if obstacles.blocked(at, radius) {
                continue;
            }
            obstacles.add(at, radius);
            chunk.placements.push(Placement {
                kind: PLANTS.len() + rock,
                position: Vector3::new(at.x, 0.0, at.y),
                yaw: random.between(0.0, std::f32::consts::TAU),
                scale,
                shelter: 1.0,
            });
        }
    }

    for (index, chunk) in chunks.iter_mut().enumerate() {
        let mut random = Random::for_place(seed, index as i64, 2);
        for _ in 0..(MOST_PLANTS * CHUNK * CHUNK).round() as usize {
            let at = spot(&mut random, chunk.middle);
            if !inside(at) || at.norm() < 1.0 {
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
                position: Vector3::new(at.x, 0.0, at.y),
                yaw: random.between(0.0, std::f32::consts::TAU),
                scale,
                shelter: 1.0 - 0.6 * thickness,
            });
        }
    }
    for chunk in &mut chunks {
        for placement in &chunk.placements {
            chunk.bands[placement.band()].push(*placement);
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_floor() {
        let trees = forest::lay_out(2);
        let a: Vec<Placement> = lay_out(2, &trees).into_iter().flat_map(|c| c.placements).collect();
        let b: Vec<Placement> = lay_out(2, &trees).into_iter().flat_map(|c| c.placements).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn floor_keeps_off_trunks_and_grows_every_kind() {
        let trees = forest::lay_out(1);
        let placements: Vec<Placement> = lay_out(1, &trees).into_iter().flat_map(|c| c.placements).collect();
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
        let density = placements.len() as f32 / area;
        assert!((0.3..1.5).contains(&density), "{density} a square meter");
    }

    #[test]
    #[ignore = "prints how many of each kind grow"]
    fn counts() {
        let trees = forest::lay_out(1);
        let placements: Vec<Placement> = lay_out(1, &trees).into_iter().flat_map(|c| c.placements).collect();
        for (index, kind) in kinds().enumerate() {
            println!("{}: {}", kind.name, placements.iter().filter(|p| p.kind == index).count());
        }
        println!("all: {}", placements.len());
    }
}
