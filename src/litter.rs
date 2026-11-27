//! Fallen leaves round the broad-leaved trees in autumn, as plant_life's Birch Glade scatters them
//! for a snapshot (`add_litter` in its `seasonal_growth.py`): low, irregular six-sided leaves on
//! the dry ground within a few meters of each trunk, in three dead browns, more of them the further
//! its crown's leaf fall has gone - up to [`MOST_LEAVES`] a tree once the last leaves are down.
//!
//! plant_life's snapshots have none before a crown's leaves start falling, so a snapshot of January
//! has none; here the leaves, there since the autumn, rot away instead through the winter, gone by
//! the time the crown leafs out again (the Wilderness's own).
//!
//! The leaves are laid out once, each tree's in the order they fall; the meshes, a chunk of the
//! forest each, are made again when how many lie under some kind of tree changes.

use crate::{forest::{Tree, SPECIES}, random::Random, terrain::Terrain};
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Vector2, Vector3, Vector4},
        math::TriangleDefinition,
        pool::Handle,
        uuid::Uuid,
    },
    fxhash::FxHashMap,
    graph::SceneGraph,
    material::{Material, MaterialResource},
    resource::texture::{Texture, TextureKind, TexturePixelKind, TextureResource},
    scene::{
        base::BaseBuilder,
        graph::Graph,
        mesh::{
            buffer::{TriangleBuffer, VertexBuffer},
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
            MeshBuilder,
        },
        node::Node,
    },
};

/// The most leaves under a tree, once its leaves are all down: plant_life's.
pub const MOST_LEAVES: usize = 28;
/// How far from the trunk they lie, least and most, in meters.
const SPREAD: (f32, f32) = (0.40, 2.9);
/// The three dead browns, linear: plant_life's.
const SHADES: [[f32; 3]; 3] = [[0.23, 0.076, 0.025], [0.35, 0.15, 0.025], [0.45, 0.235, 0.05]];
/// A leaf's outline, along and across it, as a share of its length and width.
const SHAPE: [(f32, f32); 6] = [(-0.50, 0.0), (-0.24, -0.39), (0.10, -0.50), (0.5, 0.0), (0.16, 0.43), (-0.23, 0.48)];
/// How wide a chunk of the litter is, in meters.
const CHUNK: f32 = 48.0;
/// Which trees drop leaves that lie, by species name, and their leaves' material (see
/// `seasons.rs`): the broad-leaved ones; the larch's needles are too fine to show.
const SHEDS: [(&str, &str); 5] = [
    ("Silver birch", "Silver Birch Leaves"),
    ("Rowan", "Rowan Leaves"),
    ("English oak", "English Oak Leaves"),
    ("Common alder", "Common alder Leaves"),
    ("White willow", "White willow Leaves"),
];

/// One fallen leaf: which of its tree's falls it is, its shade, and its corners.
#[derive(Debug, Clone, PartialEq)]
struct Leaf {
    rank: usize,
    shade: usize,
    corners: [Vector3<f32>; 6],
}

/// The leaves of one chunk, by which of [`SHEDS`] dropped them, and its mesh, if any lie.
#[derive(Debug, Default, PartialEq)]
struct Chunk {
    leaves: Vec<(usize, Leaf)>,
    node: Handle<Node>,
}

/// The fallen leaves, and how many lie under each kind of tree now.
#[derive(Debug, PartialEq)]
pub struct Litter {
    chunks: Vec<Chunk>,
    material: MaterialResource,
    lying: Option<Vec<usize>>,
}

impl Litter {
    /// Lays out where every leaf under `trees` will lie, on `terrain`'s dry ground.
    pub fn lay_out(seed: u64, terrain: &Terrain, trees: &[Tree]) -> Self {
        let mut by_chunk: FxHashMap<(i32, i32), Vec<(usize, Leaf)>> = FxHashMap::default();
        for (t, tree) in trees.iter().enumerate() {
            let Some(sheds) = SHEDS.iter().position(|(name, _)| *name == SPECIES[tree.species].name) else {
                continue;
            };
            let mut random = Random::for_place(seed ^ 0x893fe, t as i64, 0);
            for rank in 0..MOST_LEAVES {
                let angle = random.between(0.0, std::f32::consts::TAU);
                let radius = random.between(SPREAD.0, SPREAD.1);
                let center = tree.position.xz() + Vector2::new(angle.cos(), angle.sin()) * radius;
                let heading = random.between(0.0, std::f32::consts::TAU);
                let length = random.between(0.035, 0.084);
                let width = length * random.between(0.40, 0.64);
                let shade = (random.unit() * SHADES.len() as f32) as usize % SHADES.len();
                if terrain.wet(center.x, center.y, 0.0) {
                    continue;
                }
                let axis = Vector2::new(heading.cos(), heading.sin());
                let across = Vector2::new(-axis.y, axis.x);
                let corners = SHAPE.map(|(u, v)| {
                    let point = center + axis * (u * length) + across * (v * width);
                    Vector3::new(point.x, terrain.height(point.x, point.y) + 0.009 + 0.006 * v.abs(), point.y)
                });
                let key = ((center.x / CHUNK).floor() as i32, (center.y / CHUNK).floor() as i32);
                by_chunk.entry(key).or_default().push((sheds, Leaf { rank, shade, corners }));
            }
        }
        let chunks = by_chunk.into_values().map(|leaves| Chunk { leaves, node: Handle::NONE }).collect();
        Self { chunks, material: material(), lying: None }
    }

    /// Lays the leaves lying on `day` (1 on January 1, fractions too): each chunk's mesh made
    /// again when how many lie under some kind of tree has changed.
    pub fn update(&mut self, graph: &mut Graph, day: f32) {
        let lying: Vec<usize> = SHEDS
            .iter()
            .map(|(_, leaves)| (crate::seasons::fallen(leaves, day) * MOST_LEAVES as f32).round() as usize)
            .collect();
        if self.lying.as_ref() == Some(&lying) {
            return;
        }
        for chunk in &mut self.chunks {
            if graph.is_valid_handle(chunk.node) {
                graph.remove_node(chunk.node);
            }
            chunk.node = Handle::NONE;
            let shown: Vec<&Leaf> = chunk.leaves.iter().filter(|(sheds, leaf)| leaf.rank < lying[*sheds]).map(|(_, leaf)| leaf).collect();
            if shown.is_empty() {
                continue;
            }
            let mut vertices = Vec::with_capacity(shown.len() * 6);
            let mut triangles = Vec::with_capacity(shown.len() * 4);
            for leaf in shown {
                let first = vertices.len() as u32;
                // Each shade a texel of the material's texture.
                let tex_coord = Vector2::new((leaf.shade as f32 + 0.5) / SHADES.len() as f32, 0.5);
                for corner in leaf.corners {
                    vertices.push(StaticVertex { position: corner, tex_coord, normal: Vector3::y(), tangent: Vector4::new(1.0, 0.0, 0.0, 1.0) });
                }
                // A fan from the first corner, facing up.
                for k in 1..5 {
                    triangles.push(TriangleDefinition([first, first + k + 1, first + k]));
                }
            }
            let data = SurfaceData::new(
                VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"),
                TriangleBuffer::new(triangles),
            );
            chunk.node = MeshBuilder::new(BaseBuilder::new().with_name("Fallen leaves").with_cast_shadows(false))
                .with_surfaces(vec![SurfaceBuilder::new(SurfaceResource::new_embedded(data)).with_material(self.material.clone()).build()])
                .build(graph)
                .to_base();
        }
        self.lying = Some(lying);
    }

    /// How many leaves lie now.
    pub fn lying(&self) -> usize {
        let Some(lying) = &self.lying else {
            return 0;
        };
        self.chunks.iter().flat_map(|c| &c.leaves).filter(|(sheds, leaf)| leaf.rank < lying[*sheds]).count()
    }
}

/// The leaves' material: the standard one, rough, its colour one of three texels of a tiny
/// texture, stored sRGB.
fn material() -> MaterialResource {
    let bytes: Vec<u8> = SHADES
        .iter()
        .flat_map(|shade| {
            let srgb = shade.map(|c| {
                let c = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                (c.clamp(0.0, 1.0) * 255.0).round() as u8
            });
            [srgb[0], srgb[1], srgb[2], 255]
        })
        .collect();
    let mut texture = Texture::from_bytes(TextureKind::Rectangle { width: SHADES.len() as u32, height: 1 }, TexturePixelKind::SRGBA8, bytes)
        .expect("the shades' size matches their bytes");
    texture.set_magnification_filter(fyrox::resource::texture::TextureMagnificationFilter::Nearest);
    texture.set_minification_filter(fyrox::resource::texture::TextureMinificationFilter::Nearest);
    let mut material = Material::standard();
    material.bind("diffuseTexture", TextureResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, texture));
    MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, material)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_lie_in_autumn_and_winter_and_rot_by_spring() {
        let terrain = Terrain::new(2);
        let trees = crate::forest::lay_out(2, &terrain);
        let mut litter = Litter::lay_out(2, &terrain, &trees);
        let mut graph = Graph::new();
        let mut on = |day: f32| {
            litter.update(&mut graph, day);
            litter.lying()
        };
        let (summer, autumn, november, winter, spring) = (on(200.0), on(290.0), on(330.0), on(20.0), on(160.0));
        assert_eq!(summer, 0);
        assert!(autumn > 0 && november > autumn, "{autumn} then {november}");
        assert!(winter > 0 && winter < november, "{winter} in January");
        assert_eq!(spring, 0);
        // None in the water.
        for leaf in litter.chunks.iter().flat_map(|c| &c.leaves) {
            let middle = leaf.1.corners.iter().sum::<Vector3<f32>>() / 6.0;
            assert!(!terrain.wet(middle.x, middle.z, -0.05));
        }
    }
}
