//! Snow building up as it falls, as plant_life's Birch Glade accumulates it (its
//! `environments/birch-glade/tools/weather.py`, 503da06): on the forest floor and on the
//! upward-facing branches, over 24 hours of snowfall.
//!
//! Each place has a readiness, how soon snow lies there - on the floor plant_life's patches of
//! gentle ground first, thinned by how steep it is, everywhere dry by the end; on a branch, how
//! much it faces up - and a depth it settles to: 43-47 cm inland, deep enough to bury the grass;
//! 12-20 cm within 42 cm of the water; a centimeter and a half on a shelf in the shallowest margins
//! (water under 8.5 cm), the deeper water open; and 3.5-9 cm on a branch. After `h` hours of
//! snowfall snow lies wherever the readiness passes `1 - 0.98 h/24`, as deep as `h/24` of its
//! settled depth, tapering to nothing at the edge of each young patch. The snow has plant_life's
//! crystalline normal and roughness maps: facets of ice among rougher grains.
//!
//! Here the hours go by on the calendar as it snows; and plant_life's cutout at the patches' edge
//! is cut into the triangles instead, so that the snow's texture coordinates can tile its
//! crystals. A branch's snow is a surface added to its tree's mesh, and sways with the wind as
//! the bark under it does.

use crate::terrain::Terrain;
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Vector2, Vector3, Vector4},
        color::Color,
        log::Log,
        math::TriangleDefinition,
        pool::Handle,
        uuid::Uuid,
    },
    fxhash::{FxHashMap, FxHashSet},
    graph::SceneGraph,
    material::{Material, MaterialResource},
    resource::{
        model::ModelResource,
        texture::{Texture, TextureImportOptions, TextureKind, TextureResource},
    },
    scene::{
        base::BaseBuilder,
        graph::Graph,
        mesh::{
            buffer::{TriangleBuffer, VertexAttributeUsage, VertexBuffer, VertexReadTrait},
            surface::{BlendShapesContainer, InputBlendShapeData, SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
            Mesh, MeshBuilder,
        },
        node::Node,
    },
};
use half::f16;

/// The hours of snowfall it takes for the whole dry floor to be covered: plant_life's.
pub const FULL_HOURS: f32 = 24.0;
/// How many hours of snow lie when it is snowing from the start, unless WILDERNESS_SNOW_HOURS
/// says otherwise: plant_life's default.
pub const DEFAULT_HOURS: f32 = 12.0;
/// How finely the snow follows the hours: its depth and reach are made again each time this
/// many hours have passed.
const STEP_HOURS: f32 = 0.25;
/// The floor's snow's grid, in meters, and its squares': twice the ground's, a quarter of the
/// triangles - on the ground's own grid, the snow cost some four frames a second - and still
/// lying over it, the ground under the snow being gentle where snow lies.
const SPACING: f32 = 1.0;
const SQUARE: f32 = crate::terrain::CHUNK / 2.0;
/// How finely the branches' snow follows the hours: every tree's mesh is given its kind's snow
/// anew each time, which the renderer places again, so not often; on a branch, a few centimeters
/// at most, it hardly shows.
const BRANCH_STEP_HOURS: f32 = 2.0;
/// How many of the floor's squares are made again a frame, when the snow has changed.
const SQUARES_A_FRAME: usize = 4;
/// How wide a tile of the crystals is, in meters, on the floor and on a branch: plant_life's.
const FLOOR_TILE: f32 = 0.42;
const BRANCH_TILE: f32 = 0.34;

/// The readiness snow lies past after `hours` of snowfall: plant_life's `snow_cutoff`.
pub fn cutoff(hours: f32) -> f32 {
    1.0 - 0.98 * (hours / FULL_HOURS)
}

/// How deep the snow lies after `hours`, where it is `readiness` ready and settles to
/// `thickness`: plant_life's `settled_depth`.
pub fn settled_depth(readiness: f32, thickness: f32, hours: f32) -> f32 {
    let fraction = hours / FULL_HOURS;
    let edge = ((readiness - cutoff(hours)) / (0.025 + 0.16 * (1.0 - fraction))).clamp(0.0, 1.0);
    thickness * fraction * edge * edge * (3.0 - 2.0 * edge)
}

/// The floor at `x`, `z`, where the ground's normal is `normal`: how ready, the height snow
/// starts from, and how deep it settles. As plant_life's `apply_ground` (its y is z here).
pub fn floor_at(terrain: &Terrain, x: f32, z: f32, normal: Vector3<f32>) -> (f32, f32, f32) {
    let ground = terrain.height(x, z);
    let water = terrain.water_depth(x, z);
    let y = z;
    let mut patch = 0.63 + 0.21 * (1.6 * x + 0.6 * y + 0.7 * y.sin()).sin();
    patch += 0.14 * (2.9 * y - 1.9 * x).sin() + 0.08 * (5.2 * x + 4.4 * y).cos();
    let slope = ((normal.y - 0.48) / 0.5).clamp(0.0, 1.0);
    let texture = (patch * (0.43 + 0.57 * slope)).clamp(0.0, 1.0);
    if water > 0.0 {
        // Only a narrow shelf beside the shallowest margins can collect snow.
        let readiness = if water < 0.085 { 0.16 * (1.0 - water / 0.11) } else { 0.0 };
        return (readiness, ground + water + 0.008, 0.015);
    }
    let close_to_stream = [(0.42, 0.0), (-0.42, 0.0), (0.0, 0.42), (0.0, -0.42)]
        .iter()
        .any(|&(dx, dz)| terrain.water_depth(x + dx, z + dz) > 0.0);
    let thickness = if close_to_stream { 0.12 + 0.08 * texture } else { 0.43 + 0.04 * texture };
    (0.045 + 0.94 * texture, ground + 0.012, thickness)
}

/// A point of snow before it is cut to the hours: where it lies, bare (x and z; the height it
/// starts from), its normal, how ready, how deep it settles, and its wind offsets, if any.
#[derive(Debug, Clone, PartialEq)]
struct Point {
    position: Vector3<f32>,
    normal: Vector3<f32>,
    readiness: f32,
    thickness: f32,
    wind: Vec<Vector3<f32>>,
}

impl Point {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(&other.position, t),
            normal: self.normal.lerp(&other.normal, t).try_normalize(1e-6).unwrap_or(self.normal),
            readiness: self.readiness + (other.readiness - self.readiness) * t,
            thickness: self.thickness + (other.thickness - self.thickness) * t,
            wind: self.wind.iter().zip(&other.wind).map(|(a, b)| a.lerp(b, t)).collect(),
        }
    }
}

/// The snow on `points` after `hours`: each triangle of `triangles` cut where its readiness passes
/// the cutoff, the rest raised by its depth plus `lift`; the texture coordinates its x and z over
/// `tile`. The points made, and the triangles; none if no snow lies on them.
fn cut(points: &[Point], triangles: &[[u32; 3]], hours: f32, lift: f32, tile: f32) -> (Vec<Point>, Vec<[u32; 3]>) {
    let cutoff = cutoff(hours);
    let mut made: Vec<Point> = Vec::new();
    let mut kept: Vec<[u32; 3]> = Vec::new();
    let mut used: FxHashMap<u32, u32> = FxHashMap::default();
    let _ = tile;
    for triangle in triangles {
        let over = triangle.map(|i| points[i as usize].readiness - cutoff);
        if over.iter().all(|&o| o <= 0.0) {
            continue;
        }
        if over.iter().all(|&o| o > 0.0) {
            let corners = triangle.map(|i| {
                *used.entry(i).or_insert_with(|| {
                    made.push(points[i as usize].clone());
                    made.len() as u32 - 1
                })
            });
            kept.push(corners);
            continue;
        }
        // Cut: the part over the cutoff, corners in order, then a fan.
        let mut polygon: Vec<u32> = Vec::with_capacity(4);
        for k in 0..3 {
            let (a, b) = (triangle[k], triangle[(k + 1) % 3]);
            let (oa, ob) = (over[k], over[(k + 1) % 3]);
            if oa > 0.0 {
                polygon.push(*used.entry(a).or_insert_with(|| {
                    made.push(points[a as usize].clone());
                    made.len() as u32 - 1
                }));
            }
            if (oa > 0.0) != (ob > 0.0) {
                let t = oa / (oa - ob);
                let mut point = points[a as usize].lerp(&points[b as usize], t);
                point.readiness = cutoff;
                made.push(point);
                polygon.push(made.len() as u32 - 1);
            }
        }
        for k in 1..polygon.len().saturating_sub(1) {
            kept.push([polygon[0], polygon[k], polygon[k + 1]]);
        }
    }
    for point in &mut made {
        point.position.y += lift + settled_depth(point.readiness, point.thickness, hours);
    }
    (made, kept)
}

/// A surface of `points` and `triangles`, its texture coordinates x and z over `tile`; with the
/// wind's shapes named as `shapes`, if any.
fn surface_of(points: &[Point], triangles: &[[u32; 3]], tile: f32, shapes: &[(String, f32)]) -> SurfaceData {
    let vertices: Vec<StaticVertex> = points
        .iter()
        .map(|p| StaticVertex {
            position: p.position,
            tex_coord: Vector2::new(p.position.x / tile, p.position.z / tile),
            normal: p.normal,
            tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
        })
        .collect();
    let buffer = VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout");
    let mut data = SurfaceData::new(buffer, TriangleBuffer::new(triangles.iter().map(|&t| TriangleDefinition(t)).collect()));
    if !shapes.is_empty() {
        let inputs: Vec<InputBlendShapeData> = shapes
            .iter()
            .enumerate()
            .map(|(layer, (name, weight))| InputBlendShapeData {
                default_weight: *weight,
                name: name.clone(),
                positions: points
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| p.wind.get(layer).map(|o| (i as u32, o.map(f16::from_f32))))
                    .collect(),
                normals: Default::default(),
                tangents: Default::default(),
            })
            .collect();
        data.blend_shapes_container = Some(BlendShapesContainer::from_lists(&data.vertex_buffer, &inputs));
    }
    data
}

/// One square of the floor's snow: its points and triangles, bare, and its mesh now.
#[derive(Debug)]
struct Square {
    points: Vec<Point>,
    triangles: Vec<[u32; 3]>,
    node: Handle<Node>,
    data: Option<SurfaceResource>,
}

/// One kind of tree's branch snow: the upward-facing bark of its model, the keys of its bark's
/// surface data (to know its trees by), the wind's shapes, and the surface made for the hours.
#[derive(Debug)]
struct Branches {
    bark: FxHashSet<u64>,
    points: Vec<Point>,
    triangles: Vec<[u32; 3]>,
    shapes: Vec<(String, f32)>,
    made: Option<SurfaceResource>,
}

/// The snow lying, made for some hours of snowfall.
#[derive(Debug)]
pub struct Snow {
    squares: Vec<Square>,
    branches: Vec<Branches>,
    material: MaterialResource,
    /// The hours the snow is made for, and the squares still to make again for them; and the
    /// hours the branches' is.
    hours: f32,
    branch_hours: f32,
    stale: Vec<usize>,
    /// The surfaces let go, held a while: a resource's key is where it is in memory, and the
    /// renderer's caches, keyed by it, would take a new surface made where one just let go was
    /// for that one, until they have forgotten it.
    let_go: std::collections::VecDeque<(std::time::Instant, SurfaceResource)>,
}

/// How long a surface let go is held.
const HELD: std::time::Duration = std::time::Duration::from_secs(10);

impl Snow {
    /// The snow's places on `terrain`'s floor and on the branches of `trees` (each model with its
    /// file's path): nothing lying yet.
    pub fn lay_out(terrain: &Terrain, trees: &[(&str, &ModelResource)]) -> Self {
        let started = std::time::Instant::now();
        let half = crate::terrain::half_size();
        let count = (2.0 * half / SQUARE).ceil() as i32;
        let cells = (SQUARE / SPACING).round() as usize;
        let across = cells + 1;
        let mut squares = Vec::new();
        for sx in 0..count {
            for sz in 0..count {
                let origin = Vector2::new(-half + sx as f32 * SQUARE, -half + sz as f32 * SQUARE);
                let mut points = Vec::with_capacity(across * across);
                for j in 0..across {
                    for i in 0..across {
                        let x = origin.x + i as f32 * SPACING;
                        let z = origin.y + j as f32 * SPACING;
                        let e = SPACING;
                        let dx = terrain.height(x + e, z) - terrain.height(x - e, z);
                        let dz = terrain.height(x, z + e) - terrain.height(x, z - e);
                        let normal = Vector3::new(-dx, 2.0 * e, -dz).normalize();
                        let (readiness, base, thickness) = floor_at(terrain, x, z, normal);
                        points.push(Point { position: Vector3::new(x, base, z), normal, readiness, thickness, wind: Vec::new() });
                    }
                }
                let mut triangles = Vec::with_capacity(cells * cells * 2);
                for j in 0..cells {
                    for i in 0..cells {
                        let a = (j * across + i) as u32;
                        let (b, c, d) = (a + 1, a + across as u32, a + across as u32 + 1);
                        // Wound to face up, as the ground's.
                        triangles.push([a, c, b]);
                        triangles.push([b, c, d]);
                    }
                }
                squares.push(Square { points, triangles, node: Handle::NONE, data: None });
            }
        }
        let branches: Vec<Branches> = trees.iter().filter_map(|&(path, model)| branches_of(path, model)).collect();
        Log::info(format!(
            "Snow: laid out over the floor in {} squares and on the branches of {} kinds of tree, in {:.2} s",
            squares.len(),
            branches.len(),
            started.elapsed().as_secs_f32()
        ));
        Self { squares, branches, material: material(), hours: 0.0, branch_hours: 0.0, stale: Vec::new(), let_go: Default::default() }
    }

    /// Lies as `hours` of snowfall leave it (0 to [`FULL_HOURS`]), in steps of [`STEP_HOURS`]: the
    /// branches at once, the floor a few squares a frame (all of them, with `all`).
    pub fn update(&mut self, graph: &mut Graph, hours: f32, all: bool) {
        let hours = ((hours.clamp(0.0, FULL_HOURS) / STEP_HOURS).round() * STEP_HOURS).clamp(0.0, FULL_HOURS);
        if hours != self.hours {
            self.hours = hours;
            self.stale = (0..self.squares.len()).collect();
        }
        // The branches, in their own coarser steps; bare as soon as it has all melted.
        let branch_hours = if hours <= 0.0 { 0.0 } else { (hours / BRANCH_STEP_HOURS).ceil() * BRANCH_STEP_HOURS }.min(FULL_HOURS);
        if branch_hours != self.branch_hours {
            self.branch_hours = branch_hours;
            self.dress_branches(graph);
        }
        let now = std::time::Instant::now();
        while self.let_go.front().is_some_and(|(when, _)| now.duration_since(*when) > HELD) {
            self.let_go.pop_front();
        }
        let take = if all { self.stale.len() } else { SQUARES_A_FRAME.min(self.stale.len()) };
        for index in self.stale.drain(..take).collect::<Vec<_>>() {
            let square = &mut self.squares[index];
            if graph.is_valid_handle(square.node) {
                graph.remove_node(square.node);
            }
            square.node = Handle::NONE;
            if let Some(data) = square.data.take() {
                self.let_go.push_back((now, data));
            }
            if self.hours <= 0.0 {
                continue;
            }
            let (points, triangles) = cut(&square.points, &square.triangles, self.hours, 0.0, FLOOR_TILE);
            if triangles.is_empty() {
                continue;
            }
            let data = SurfaceResource::new_embedded(surface_of(&points, &triangles, FLOOR_TILE, &[]));
            square.data = Some(data.clone());
            square.node = MeshBuilder::new(BaseBuilder::new().with_name("Snow").with_cast_shadows(false))
                .with_surfaces(vec![SurfaceBuilder::new(data).with_material(self.material.clone()).build()])
                .build(graph)
                .to_base();
        }
    }

    /// The hours of snowfall the floor's snow is made for.
    pub fn hours(&self) -> f32 {
        self.hours
    }

    /// How high the snow's surface is over the ground at `x`, `z` now, in meters: 0 where none
    /// lies. From the nearest point of its grid.
    pub fn depth_at(&self, terrain: &Terrain, x: f32, z: f32) -> f32 {
        if self.hours <= 0.0 {
            return 0.0;
        }
        let half = crate::terrain::half_size();
        let count = (2.0 * half / SQUARE).ceil() as i32;
        let (sx, sz) = (((x + half) / SQUARE).floor() as i32, ((z + half) / SQUARE).floor() as i32);
        if sx < 0 || sz < 0 || sx >= count || sz >= count {
            return 0.0;
        }
        let square = &self.squares[(sx * count + sz) as usize];
        let across = (SQUARE / SPACING).round() as usize + 1;
        let origin = Vector2::new(-half + sx as f32 * SQUARE, -half + sz as f32 * SQUARE);
        let i = (((x - origin.x) / SPACING).round() as usize).min(across - 1);
        let j = (((z - origin.y) / SPACING).round() as usize).min(across - 1);
        let point = &square.points[j * across + i];
        let depth = settled_depth(point.readiness, point.thickness, self.hours);
        if point.readiness <= cutoff(self.hours) {
            return 0.0;
        }
        (point.position.y + depth - terrain.height(x, z)).max(0.0)
    }

    /// Gives every tree in `graph` its kind's branch snow for the hours, in place of what it had.
    fn dress_branches(&mut self, graph: &mut Graph) {
        let old: FxHashSet<u64> = self.branches.iter().filter_map(|b| b.made.as_ref().map(|m| m.key())).collect();
        let now = std::time::Instant::now();
        for branches in &mut self.branches {
            if let Some(made) = branches.made.take() {
                self.let_go.push_back((now, made));
            }
            branches.made = (self.branch_hours > 0.0).then(|| {
                let (points, triangles) = cut(&branches.points, &branches.triangles, self.branch_hours, 0.009, BRANCH_TILE);
                (!triangles.is_empty()).then(|| SurfaceResource::new_embedded(surface_of(&points, &triangles, BRANCH_TILE, &branches.shapes)))
            }).flatten();
        }
        let meshes: Vec<Handle<Node>> = graph.linear_iter().filter_map(|n| n.cast::<Mesh>()).map(|m| m.handle()).collect();
        let mut dressed = 0;
        for handle in meshes {
            let Some(mesh) = graph[handle].cast_mut::<Mesh>() else {
                continue;
            };
            let Some(kind) = self.branches.iter().find(|b| mesh.surfaces().iter().any(|s| b.bark.contains(&s.data_ref().key()))) else {
                continue;
            };
            let had = mesh.surfaces().iter().any(|s| old.contains(&s.data_ref().key()));
            if !had && kind.made.is_none() {
                continue;
            }
            let mut surfaces: Vec<_> = mesh.surfaces().iter().filter(|s| !old.contains(&s.data_ref().key())).cloned().collect();
            if let Some(made) = &kind.made {
                surfaces.push(SurfaceBuilder::new(made.clone()).with_material(self.material.clone()).build());
                dressed += 1;
            }
            mesh.set_surfaces(surfaces);
        }
        let _ = dressed;
    }
}

/// The upward-facing bark of the tree model at `path`, with its wind offsets: plant_life's
/// `branch_snow`, by face (its normal up at least a quarter) and by vertex (how much it faces up).
fn branches_of(path: &str, model: &ModelResource) -> Option<Branches> {
    if !model.is_ok() {
        return None;
    }
    let names = crate::seasons::material_names(path);
    let model = model.data_ref();
    let graph = &model.get_scene().graph;
    let mesh = graph.linear_iter().find_map(|node| node.cast::<Mesh>())?;
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    let mut bark = FxHashSet::default();
    let shapes: Vec<(String, f32)> = mesh.blend_shapes().iter().map(|s| (s.name.clone(), s.weight)).collect();
    for (surface, name) in mesh.surfaces().iter().zip(&names) {
        if !name.ends_with(" Bark") {
            continue;
        }
        bark.insert(surface.data_ref().key());
        let resource = surface.data();
        let data = resource.data_ref();
        let wind = wind_offsets(&data);
        let mut used: FxHashMap<u32, u32> = FxHashMap::default();
        let positions: Vec<Vector3<f32>> = data.vertex_buffer.iter().map(|v| v.read_3_f32(VertexAttributeUsage::Position).unwrap_or_default()).collect();
        let normals: Vec<Vector3<f32>> = data.vertex_buffer.iter().map(|v| v.read_3_f32(VertexAttributeUsage::Normal).unwrap_or_default()).collect();
        for triangle in data.geometry_buffer.iter() {
            let [a, b, c] = triangle.0.map(|i| positions[i as usize]);
            let Some(face) = (b - a).cross(&(c - a)).try_normalize(1e-9) else {
                continue;
            };
            if face.y < 0.25 {
                continue;
            }
            let corners = triangle.0.map(|i| {
                *used.entry(i).or_insert_with(|| {
                    let normal = normals[i as usize];
                    let top = ((normal.y - 0.22) / 0.68).clamp(0.0, 1.0);
                    points.push(Point {
                        position: positions[i as usize],
                        normal: Vector3::new(normal.x * 0.3, 1.0, normal.z * 0.3).normalize(),
                        readiness: 0.06 + 0.82 * top,
                        thickness: 0.035 + 0.055 * top,
                        wind: wind.iter().map(|layer| layer.get(i as usize).copied().unwrap_or_default()).collect(),
                    });
                    points.len() as u32 - 1
                })
            });
            triangles.push(corners);
        }
    }
    if triangles.is_empty() {
        return None;
    }
    // Wind offsets for every point, or for none.
    let shapes = if points.iter().all(|p| p.wind.len() == shapes.len()) { shapes } else { Vec::new() };
    Some(Branches { bark, points, triangles, shapes, made: None })
}

/// The wind's position offsets of each of `data`'s vertices, a list for each shape, as the engine
/// packs them (`BlendShapesContainer::from_lists`): a layer of a volume texture for each, each
/// vertex's position, normal and tangent offsets three half-float RGB texels, the vertices in
/// order. None without wind.
fn wind_offsets(data: &SurfaceData) -> Vec<Vec<Vector3<f32>>> {
    let Some(container) = data.blend_shapes_container.as_ref() else {
        return Vec::new();
    };
    let Some(storage) = container.blend_shape_storage.as_ref() else {
        return Vec::new();
    };
    let texture = storage.data_ref();
    let TextureKind::Volume { width, height, .. } = texture.kind() else {
        return Vec::new();
    };
    let (width, height) = (width as usize / 3, height as usize);
    let bytes = texture.data();
    let count = data.vertex_buffer.vertex_count() as usize;
    let layers = container.blend_shapes.len();
    if count > width * height || bytes.len() < layers * width * height * 18 {
        return Vec::new();
    }
    let half_at = |offset: usize| f16::from_le_bytes([bytes[offset], bytes[offset + 1]]).to_f32();
    (0..layers)
        .map(|layer| {
            (0..count)
                .map(|vertex| {
                    let at = (layer * width * height + vertex) * 18;
                    Vector3::new(half_at(at), half_at(at + 2), half_at(at + 4))
                })
                .collect()
        })
        .collect()
}

/// The snow's material: plant_life's colour - (.83, .90, 1) times its shade, here its middle -
/// with its crystals' normals and roughness.
fn material() -> MaterialResource {
    let (normals, roughness) = crystal_maps();
    let mut material = Material::standard();
    let shade = 0.88;
    let to_srgb = |c: f32| {
        let c = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    material.set_property("diffuseColor", Color::opaque(to_srgb(0.83 * shade), to_srgb(0.90 * shade), to_srgb(shade)));
    for (name, image) in [("normalTexture", normals), ("roughnessTexture", roughness)] {
        match image.and_then(|png| Texture::load_from_memory(&png, TextureImportOptions::default()).map_err(|e| format!("{e:?}"))) {
            Ok(texture) => material.bind(name, TextureResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, texture)),
            Err(error) => Log::warn(format!("Snow: its {name} could not be made ({error}); it is left smooth")),
        }
    }
    MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, material)
}

/// plant_life's `crystal_maps`: a 512-texel tile of 112 x 112 cells, each a facet of a random
/// tilt and turn, flattest at its edges, some of them ice, smoother. As PNG files: the normals,
/// and the roughness (its green, as the standard material reads its red).
fn crystal_maps() -> (Result<Vec<u8>, String>, Result<Vec<u8>, String>) {
    const SIZE: usize = 512;
    const CELLS: usize = 112;
    let mut random = crate::random::Random::for_place(6301, 0, 0);
    let seeds: Vec<[f32; 2]> = (0..CELLS * CELLS).map(|_| [random.between(0.12, 0.88), random.between(0.12, 0.88)]).collect();
    let angles: Vec<f32> = (0..CELLS * CELLS).map(|_| random.between(0.0, std::f32::consts::TAU)).collect();
    let slopes: Vec<f32> = (0..CELLS * CELLS).map(|_| random.between(0.045, 0.52)).collect();
    let ices: Vec<bool> = (0..CELLS * CELLS).map(|_| random.unit() < 0.17).collect();
    let mut normal = Vec::with_capacity(SIZE * SIZE * 3);
    let mut rough = Vec::with_capacity(SIZE * SIZE);
    let scale = CELLS as f32 / SIZE as f32;
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (x, y) = ((px as f32 + 0.5) * scale, (py as f32 + 0.5) * scale);
            let (i, j) = (x as i32, y as i32);
            let (mut nearest, mut second) = (f32::INFINITY, f32::INFINITY);
            let mut cell = 0;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let a = (i + dx).rem_euclid(CELLS as i32) as usize;
                    let b = (j + dy).rem_euclid(CELLS as i32) as usize;
                    let offset = seeds[b * CELLS + a];
                    let d = (i as f32 + dx as f32 + offset[0] - x).powi(2) + (j as f32 + dy as f32 + offset[1] - y).powi(2);
                    if d < nearest {
                        second = nearest;
                        nearest = d;
                        cell = b * CELLS + a;
                    } else if d < second {
                        second = d;
                    }
                }
            }
            let facet = ((second.sqrt() - nearest.sqrt()) / 0.22).clamp(0.0, 1.0);
            let xy = slopes[cell] * (0.5 + 0.5 * facet);
            let n = [xy * angles[cell].cos(), xy * angles[cell].sin(), (1.0 - xy * xy).sqrt()];
            normal.extend(n.map(|c| ((c * 0.5 + 0.5) * 255.0).round() as u8));
            let ice = if ices[cell] { facet } else { 0.0 };
            let roughness = 0.67 + 0.16 * (1.0 - facet) - 0.55 * ice;
            rough.push((roughness.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    let png = |bytes: &[u8], color: image::ExtendedColorType| {
        let mut out = Vec::new();
        image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut out), bytes, SIZE as u32, SIZE as u32, color)
            .map(|_| out)
            .map_err(|e| e.to_string())
    };
    (png(&normal, image::ExtendedColorType::Rgb8), png(&rough, image::ExtendedColorType::L8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snow_fills_the_gentle_ground_first_and_covers_the_dry_floor_by_a_day() {
        let terrain = Terrain::new(crate::game::SEED);
        let mut at = Vec::new();
        for i in -40..40 {
            for j in -40..40 {
                let (x, z) = (i as f32 * 1.7, j as f32 * 1.7);
                let e = 0.5;
                let dx = terrain.height(x + e, z) - terrain.height(x - e, z);
                let dz = terrain.height(x, z + e) - terrain.height(x, z - e);
                let normal = Vector3::new(-dx, 2.0 * e, -dz).normalize();
                at.push((terrain.water_depth(x, z), floor_at(&terrain, x, z, normal)));
            }
        }
        let dry: Vec<_> = at.iter().filter(|(water, _)| *water <= 0.0).map(|(_, f)| *f).collect();
        let covered = |hours: f32| dry.iter().filter(|(r, ..)| *r > cutoff(hours)).count() as f32 / dry.len() as f32;
        assert_eq!(covered(0.0), 0.0);
        assert!(covered(6.0) > 0.0 && covered(6.0) < covered(12.0) && covered(12.0) < 1.0, "{} {}", covered(6.0), covered(12.0));
        assert_eq!(covered(24.0), 1.0);
        // Inland, 43-47 cm after a day; none in the deep water.
        let deepest = dry.iter().map(|&(r, _, t)| settled_depth(r, t, 24.0)).fold(0.0, f32::max);
        assert!((0.43..=0.47).contains(&deepest), "{deepest}");
        for (water, (r, ..)) in &at {
            if *water >= 0.085 {
                assert_eq!(*r, 0.0);
            }
        }
    }

    #[test]
    fn a_cut_keeps_only_the_snow_past_the_cutoff_and_tapers_to_the_edge() {
        let point = |x: f32, readiness: f32| Point {
            position: Vector3::new(x, 0.0, 0.0),
            normal: Vector3::y(),
            readiness,
            thickness: 0.4,
            wind: vec![Vector3::new(x, 0.0, 0.0)],
        };
        let points = vec![point(0.0, 0.1), point(1.0, 0.9), point(0.0, 0.9)];
        let hours = 12.0;
        let (made, triangles) = cut(&points, &[[0, 1, 2]], hours, 0.0, 1.0);
        assert!(!triangles.is_empty());
        for p in &made {
            assert!(p.readiness >= cutoff(hours) - 1e-5);
        }
        // Where it is cut, no deeper than the ground; the wind's offsets carried along.
        let edge = made.iter().find(|p| (p.readiness - cutoff(hours)).abs() < 1e-5).unwrap();
        assert!(edge.position.y.abs() < 1e-5);
        assert!((edge.wind[0].x - edge.position.x).abs() < 1e-5);
        let (_, none) = cut(&points, &[[0, 1, 2]], 0.0, 0.0, 1.0);
        assert!(none.is_empty());
    }

    #[test]
    fn the_crystals_are_made() {
        let (normals, roughness) = crystal_maps();
        assert!(Texture::load_from_memory(&normals.unwrap(), TextureImportOptions::default()).is_ok());
        assert!(Texture::load_from_memory(&roughness.unwrap(), TextureImportOptions::default()).is_ok());
    }
}

