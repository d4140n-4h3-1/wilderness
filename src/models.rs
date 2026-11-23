//! Getting the plant_life models ready to be planted many times over.
//!
//! The models (`~/Documents/blender/wildlife/plant_life`, CC0) are copied into `data`. Each is
//! one mesh with its origin where it meets the ground, its surfaces in the order of the glTF
//! file's primitives, and the wind's morph targets (see [`crate::wind`]).
//!
//! Each model's mesh is made ready once, before it is planted; all copies share their surface
//! data, so the first copy's changes hold for every one. Surfaces meant to be seen from both
//! sides (the glTF materials' `doubleSided`, which the importer drops) get a reversed twin of
//! every triangle: the renderer culls faces seen from behind, and a foliage card or grass blade
//! is a single sheet. The twins get the same wind offsets as the vertices they copy, or the backs
//! of the leaves would stand still while the fronts swayed. Without wind, the wind's targets are
//! dropped altogether.

use fyrox::{
    core::{
        algebra::{Vector3, Vector4},
        log::Log,
        math::TriangleDefinition,
        pool::Handle,
    },
    fxhash::{FxHashMap, FxHashSet},
    graph::SceneGraph,
    scene::{
        animation::AnimationPlayer,
        graph::Graph,
        mesh::{
            Mesh,
            buffer::{VertexAttributeUsage, VertexReadTrait, VertexWriteTrait},
            surface::{BlendShapesContainer, InputBlendShapeData, SurfaceData},
        },
        node::Node,
    },
};
use half::f16;

/// Which of a glTF model's primitives, in order, are meant to be seen from both sides. Read from
/// the file itself, as the importer drops it. Empty if the file cannot be read.
pub fn double_sided(path: &str) -> Vec<bool> {
    let json = std::fs::read(path).ok().and_then(|bytes| {
        // A binary glTF: a 12-byte header, then the JSON chunk's length, type, and the JSON.
        let length = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
        serde_json::from_slice::<serde_json::Value>(bytes.get(20..20 + length)?).ok()
    });
    let Some(json) = json else {
        Log::warn(format!("Could not read the glTF of {path}: no surface is made double-sided"));
        return Vec::new();
    };
    let materials = json["materials"].as_array().cloned().unwrap_or_default();
    json["meshes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|mesh| mesh["primitives"].as_array().cloned().unwrap_or_default())
        .map(|primitive| {
            primitive["material"]
                .as_u64()
                .and_then(|m| materials.get(m as usize))
                .and_then(|m| m["doubleSided"].as_bool())
                .unwrap_or(false)
        })
        .collect()
}

/// What has been done already, across all models.
#[derive(Debug, Default, PartialEq)]
pub struct Prepared {
    /// Surface data already made ready.
    surfaces: FxHashSet<u64>,
}

impl Prepared {
    /// Makes the meshes of a model copy under `root` ready to plant (see the module's
    /// description). `double_sided` is [`double_sided`] of its file; `wind` whether it will
    /// sway.
    pub fn prepare(&mut self, graph: &mut Graph, root: Handle<Node>, double_sided: &[bool], wind: bool) {
        let nodes: Vec<Handle<Node>> = graph.traverse_handle_iter(root).collect();
        for &handle in &nodes {
            // The baked wind clip's player: the wind is driven from here instead, and the clip
            // would only fight it (the importer's binding to the weights does not even resolve).
            if graph[handle].cast::<AnimationPlayer>().is_some() {
                graph.remove_node(handle);
            }
        }
        for handle in nodes {
            let Some(mesh) = graph.try_get_node(handle).ok().and_then(|n| n.cast::<Mesh>()) else {
                continue;
            };
            for (index, surface) in mesh.surfaces().iter().enumerate() {
                let resource = surface.data();
                if !self.surfaces.insert(resource.key()) {
                    continue;
                }
                let mut data = resource.data_ref();
                if double_sided.get(index).copied().unwrap_or(false) {
                    double(&mut data);
                }
                if !wind {
                    data.blend_shapes_container = None;
                }
            }
        }
    }
}

/// The first mesh under `root`.
pub fn mesh_of(graph: &Graph, root: Handle<Node>) -> Option<Handle<Node>> {
    graph.traverse_handle_iter(root).find(|h| graph[*h].cast::<Mesh>().is_some())
}

/// Gives every triangle of `data` a reversed twin, with its own copies of the vertices, normals
/// turned over; and the twins the same wind offsets as the vertices they copy.
fn double(data: &mut SurfaceData) {
    let original_count = data.vertex_buffer.vertex_count();
    let raw = data.vertex_buffer.raw_data().to_vec();
    let vertex_size = data.vertex_buffer.vertex_size() as usize;
    {
        let mut vertices = data.vertex_buffer.modify();
        for vertex in raw.chunks_exact(vertex_size) {
            let _ = vertices.push_vertex_raw(vertex);
        }
        for mut vertex in vertices.iter_mut().skip(original_count as usize) {
            if let Ok(normal) = vertex.read_3_f32(VertexAttributeUsage::Normal) {
                let _ = vertex.write_3_f32(VertexAttributeUsage::Normal, -normal);
            }
            // The tangent follows the texture's u across the card, which the flip leaves as it
            // was; with the normal turned over, the bitangent (normal x tangent, times w) would
            // turn over too, so w is flipped to keep it following v.
            if let Ok(t) = vertex.read_4_f32(VertexAttributeUsage::Tangent) {
                let _ = vertex.write_4_f32(VertexAttributeUsage::Tangent, Vector4::new(t.x, t.y, t.z, -t.w));
            }
        }
    }
    let reversed: Vec<TriangleDefinition> = data
        .geometry_buffer
        .iter()
        .map(|t| TriangleDefinition([t.0[0] + original_count, t.0[2] + original_count, t.0[1] + original_count]))
        .collect();
    {
        let mut triangles = data.geometry_buffer.modify();
        for triangle in reversed {
            triangles.push(triangle);
        }
    }

    // The wind offsets, packed by the engine (`BlendShapesContainer::from_lists`) as a volume
    // texture: a layer per target, each vertex's position, normal and tangent offsets one after
    // another as three half-float RGB texels, the vertices in order, rows of up to 512.
    let Some(container) = data.blend_shapes_container.as_ref() else {
        return;
    };
    let Some(storage) = container.blend_shape_storage.as_ref() else {
        return;
    };
    let n = original_count as usize;
    let width = n.min(512);
    let height = n.div_ceil(width.max(1));
    let texture = storage.data_ref();
    let bytes = texture.data();
    let layers = container.blend_shapes.len();
    if bytes.len() < layers * width * height * 18 {
        Log::warn("Wind offsets not as expected; the backs of leaves will not sway");
        return;
    }
    let half_at = |offset: usize| f16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let vector_at = |offset: usize| Vector3::new(half_at(offset), half_at(offset + 2), half_at(offset + 4));
    let inputs: Vec<InputBlendShapeData> = container
        .blend_shapes
        .iter()
        .enumerate()
        .map(|(layer, shape)| {
            let mut input = InputBlendShapeData {
                default_weight: shape.weight,
                name: shape.name.clone(),
                positions: FxHashMap::default(),
                normals: FxHashMap::default(),
                tangents: FxHashMap::default(),
            };
            for vertex in 0..n {
                let at = (layer * width * height + vertex) * 18;
                let (position, normal, tangent) = (vector_at(at), vector_at(at + 6), vector_at(at + 12));
                let twin = (vertex + n) as u32;
                let vertex = vertex as u32;
                input.positions.insert(vertex, position);
                input.positions.insert(twin, position);
                input.normals.insert(vertex, normal);
                input.normals.insert(twin, normal.map(|c| -c));
                input.tangents.insert(vertex, tangent);
                input.tangents.insert(twin, tangent);
            }
            input
        })
        .collect();
    drop(texture);
    data.blend_shapes_container = Some(BlendShapesContainer::from_lists(&data.vertex_buffer, &inputs));
}
