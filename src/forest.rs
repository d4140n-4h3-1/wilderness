//! The forest: which trees grow in it, where each one stands, and putting them into the scene.
//!
//! The trees are plant_life models (see [`crate::models`]): a bark surface first, then foliage
//! (alpha-masked cards), and for some cones or berries.

use crate::{
    models::{Prepared, mesh_of},
    random::{Noise, Random, smoothstep},
    wind::{self, Profile, Sway},
};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector3},
        pool::Handle,
    },
    resource::model::{ModelResource, ModelResourceExtension},
    scene::{
        Scene,
        base::BaseBuilder,
        collider::{ColliderBuilder, ColliderShape},
        node::Node,
        pivot::PivotBuilder,
        rigidbody::{RigidBodyBuilder, RigidBodyType},
        transform::TransformBuilder,
    },
};

/// A kind of tree that grows in the forest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Species {
    pub name: &'static str,
    /// Its model.
    pub path: &'static str,
    /// How common it is, against the others.
    pub weight: f32,
    /// Whether it grows more in the open, at glade edges, than in deep forest.
    pub likes_light: bool,
    /// About how far its crown reaches from the trunk, in meters, at full size.
    pub crown: f32,
    /// The trunk's radius near the ground, in meters, at full size: what the player bumps into.
    pub trunk: f32,
    /// How it sways.
    pub wind: Profile,
}

/// Every kind of tree in the forest. Conifers make up most of it; larch, birch, rowan and oak,
/// which need light, gather where the forest opens up.
pub const SPECIES: [Species; 7] = [
    Species {
        name: "Norway spruce",
        path: "data/trees/norway-spruce.glb",
        weight: 3.0,
        likes_light: false,
        crown: 2.8,
        trunk: 0.28,
        wind: wind::CONIFER,
    },
    Species {
        name: "Pine",
        path: "data/trees/pine.glb",
        weight: 2.5,
        likes_light: false,
        crown: 2.4,
        trunk: 0.28,
        wind: wind::CONIFER,
    },
    Species {
        name: "Silver fir",
        path: "data/trees/silver-fir.glb",
        weight: 1.5,
        likes_light: false,
        crown: 3.6,
        trunk: 0.3,
        wind: wind::CONIFER,
    },
    Species {
        name: "European larch",
        path: "data/trees/european-larch.glb",
        weight: 1.5,
        likes_light: true,
        crown: 3.4,
        trunk: 0.27,
        wind: wind::CONIFER,
    },
    Species {
        name: "Silver birch",
        path: "data/trees/silver-birch.glb",
        weight: 1.5,
        likes_light: true,
        crown: 4.0,
        trunk: 0.18,
        wind: wind::DECIDUOUS,
    },
    Species {
        name: "Rowan",
        path: "data/trees/rowan.glb",
        weight: 0.8,
        likes_light: true,
        crown: 3.5,
        trunk: 0.2,
        wind: wind::DECIDUOUS,
    },
    Species {
        name: "English oak",
        path: "data/trees/english-oak.glb",
        weight: 0.7,
        likes_light: true,
        crown: 5.2,
        trunk: 0.45,
        wind: wind::DECIDUOUS,
    },
];

/// How thick the forest is, anywhere: from 0 in a glade to 1 in a thicket. The trees thin out
/// where it is low, and the forest floor and its wind follow it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thicket(Noise);

impl Thicket {
    pub fn new(seed: u64) -> Self {
        Self(Noise(seed.wrapping_mul(31).wrapping_add(7)))
    }

    pub fn at(&self, x: f32, z: f32) -> f32 {
        smoothstep(0.3, 0.6, self.0.at(x / GLADE_SIZE, z / GLADE_SIZE))
    }
}

/// How far the forest reaches from the middle along x and z, in meters.
pub const HALF_EXTENT: f32 = 150.0;
/// The open ground around where the player starts, in meters.
pub const CLEARING: f32 = 9.0;
/// How close two trees may stand, as a share of their crowns added together: below 1, crowns
/// overlap, as they do in a real forest.
const SPACING: f32 = 0.9;
/// How many places are tried for a tree. Most are turned down, for being too close to another or
/// in a glade.
const TRIES: usize = 60_000;
/// How wide the forest's glades and thickets are, roughly, in meters.
pub const GLADE_SIZE: f32 = 45.0;
/// The size range of the trees, as a scale of the model.
const SCALE: (f32, f32) = (0.75, 1.2);
/// How tall a trunk collider is, in meters, from the ground: above head height is enough.
const TRUNK_HEIGHT: f32 = 4.0;

/// One tree in the forest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tree {
    /// Which of [`SPECIES`] it is.
    pub species: usize,
    /// Where its trunk meets the ground.
    pub position: Vector3<f32>,
    /// Its turn about the vertical, in radians.
    pub yaw: f32,
    pub scale: f32,
}

/// Lays out a forest: where every tree stands. The same seed lays out the same forest.
pub fn lay_out(seed: u64) -> Vec<Tree> {
    let mut random = Random(seed ^ 0x9e37_79b9_7f4a_7c15);
    let glades = Thicket::new(seed);
    // Trees are kept in a grid of cells as wide as the furthest two trees can be required to
    // stand apart, so only the cells around a new tree need checking.
    let max_reach = SPECIES.iter().map(|s| s.crown).fold(0.0, f32::max) * SCALE.1 * 2.0 * SPACING;
    let cells_across = (2.0 * HALF_EXTENT / max_reach).ceil() as usize + 1;
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); cells_across * cells_across];
    let cell_of = |p: Vector3<f32>| {
        let x = ((p.x + HALF_EXTENT) / max_reach) as usize;
        let z = ((p.z + HALF_EXTENT) / max_reach) as usize;
        (x.min(cells_across - 1), z.min(cells_across - 1))
    };

    let mut trees: Vec<Tree> = Vec::new();
    for _ in 0..TRIES {
        let position = Vector3::new(
            random.between(-HALF_EXTENT, HALF_EXTENT),
            0.0,
            random.between(-HALF_EXTENT, HALF_EXTENT),
        );
        if position.xz().norm() < CLEARING {
            continue;
        }
        // How thick the forest is here, from 0 in a glade to 1 in a thicket.
        let thickness = glades.at(position.x, position.z);
        if random.unit() > 0.08 + 0.92 * thickness {
            continue;
        }
        let species = pick_species(&mut random, thickness);
        let scale = random.between(SCALE.0, SCALE.1);
        let reach = SPECIES[species].crown * scale;

        let (cx, cz) = cell_of(position);
        let crowded = (cx.saturating_sub(1)..=(cx + 1).min(cells_across - 1)).any(|x| {
            (cz.saturating_sub(1)..=(cz + 1).min(cells_across - 1)).any(|z| {
                cells[z * cells_across + x].iter().any(|&other| {
                    let other = &trees[other];
                    let apart = (reach + SPECIES[other.species].crown * other.scale) * SPACING;
                    (other.position - position).norm_squared() < apart * apart
                })
            })
        });
        if crowded {
            continue;
        }
        cells[cz * cells_across + cx].push(trees.len());
        trees.push(Tree {
            species,
            position,
            yaw: random.between(0.0, std::f32::consts::TAU),
            scale,
        });
    }
    trees
}

/// Picks a species for a tree where the forest is `thickness` thick: the light-loving ones are
/// more likely in the open, the others in the thick of it.
fn pick_species(random: &mut Random, thickness: f32) -> usize {
    let weight = |s: &Species| {
        s.weight * if s.likes_light { 1.6 - thickness } else { 0.5 + thickness }
    };
    let total: f32 = SPECIES.iter().map(weight).sum();
    let mut pick = random.unit() * total;
    for (index, species) in SPECIES.iter().enumerate() {
        pick -= weight(species);
        if pick <= 0.0 {
            return index;
        }
    }
    SPECIES.len() - 1
}

/// Puts the forest into the scene: a copy of its model for every tree, under one node, and a
/// static body holding a collider for every trunk. `models` are the species' models, in the
/// order of [`SPECIES`], all loaded, and `double_sided` their surfaces seen from both sides (see
/// [`crate::models`]). Returns the forest's node and, with `wind`, every tree's sway.
pub fn plant(
    scene: &mut Scene,
    models: &[ModelResource],
    double_sided: &[Vec<bool>],
    trees: &[Tree],
    prepared: &mut Prepared,
    wind: bool,
    seed: u64,
) -> (Handle<Node>, Vec<Sway>) {
    let mut random = Random::for_place(seed, 0x7ee, 0x5);
    let mut planted = Vec::with_capacity(trees.len());
    let mut trunks = Vec::with_capacity(trees.len());
    let mut sways = Vec::with_capacity(trees.len());
    for tree in trees {
        let root = models[tree.species].instantiate(scene);
        let graph = &mut scene.graph;
        graph[root]
            .local_transform_mut()
            .set_position(tree.position)
            .set_rotation(UnitQuaternion::from_axis_angle(&Vector3::y_axis(), tree.yaw))
            .set_scale(Vector3::repeat(tree.scale));
        prepared.prepare(graph, root, &double_sided[tree.species], wind);
        if wind {
            let species = &SPECIES[tree.species];
            sways.extend(mesh_of(graph, root).and_then(|mesh| {
                Sway::new(graph, mesh, tree.position, tree.yaw, species.wind, 1.0, &mut random)
            }));
        }
        planted.push(root);

        let half_height = TRUNK_HEIGHT / 2.0;
        trunks.push(
            ColliderBuilder::new(
                BaseBuilder::new().with_local_transform(
                    TransformBuilder::new()
                        .with_local_position(tree.position + Vector3::new(0.0, half_height, 0.0))
                        .build(),
                ),
            )
            .with_shape(ColliderShape::cylinder(half_height, SPECIES[tree.species].trunk * tree.scale))
            .build(graph),
        );
    }
    let graph = &mut scene.graph;
    let mut body = BaseBuilder::new().with_name("Trunks");
    for trunk in trunks {
        body = body.with_child(trunk);
    }
    RigidBodyBuilder::new(body)
        .with_body_type(RigidBodyType::Static)
        .build(graph);

    let mut forest = BaseBuilder::new().with_name("Forest");
    for tree in planted {
        forest = forest.with_child(tree);
    }
    (PivotBuilder::new(forest).build(graph).to_base(), sways)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_forest() {
        assert_eq!(lay_out(3), lay_out(3));
    }

    #[test]
    fn trees_keep_apart_and_off_the_clearing() {
        let trees = lay_out(1);
        assert!(trees.len() > 300, "only {} trees", trees.len());
        for (i, a) in trees.iter().enumerate() {
            assert!(a.position.xz().norm() >= CLEARING);
            assert!(a.position.x.abs() <= HALF_EXTENT && a.position.z.abs() <= HALF_EXTENT);
            for b in &trees[i + 1..] {
                let apart = (SPECIES[a.species].crown * a.scale + SPECIES[b.species].crown * b.scale) * SPACING;
                assert!((a.position - b.position).norm() >= apart);
            }
        }
    }

    #[test]
    #[ignore = "prints glades: places with trees round them but none near, for sunlit views"]
    fn glades() {
        let trees = lay_out(1);
        let mut found = 0;
        for z in (-120..=120).step_by(6) {
            for x in (-120..=120).step_by(6) {
                let at = Vector3::new(x as f32, 0.0, z as f32);
                let near = trees.iter().filter(|t| (t.position - at).norm() < 8.0).count();
                let round = trees.iter().filter(|t| (t.position - at).norm() < 22.0).count();
                if near == 0 && round >= 4 && found < 8 {
                    println!("glade at {x}, {z}: {round} trees within 22 m");
                    found += 1;
                }
            }
        }
    }

    #[test]
    #[ignore = "prints how many trees each seed grows"]
    fn counts() {
        for seed in 1..6 {
            let trees = lay_out(seed);
            let per_species: Vec<usize> =
                (0..SPECIES.len()).map(|s| trees.iter().filter(|t| t.species == s).count()).collect();
            println!("seed {seed}: {} trees {per_species:?}", trees.len());
        }
    }

    #[test]
    fn every_species_grows() {
        let trees = lay_out(1);
        for index in 0..SPECIES.len() {
            assert!(trees.iter().any(|t| t.species == index), "no {}", SPECIES[index].name);
        }
    }
}
