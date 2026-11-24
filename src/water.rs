//! The water in the river and the streams: a ribbon along each channel at its water's height,
//! drawn with the Birch Glade's flowing-water shader (from plant_life's
//! `environments/birch-glade/hydroxus`): clear over the shallows and darker where deep (it reads
//! the depth of the bed below), the sky and the sun mirrored in its ripples, foam where it runs
//! against banks and stones.
//!
//! The ribbon follows each bank's own waterline, reaching on under the banks as the Birch Glade's
//! does, so that the ground makes the water's edge where the shader fades the shallows out. Its
//! first texture coordinates are in meters, as the shader wants them: along, how far downstream;
//! across, from its right edge.
//!
//! The water runs as gravity drives it: each stretch at the speed its fall gives it (see
//! [`Channel::speed`]), carried in its vertices' normals with how steeply it falls, which the
//! shader carries its ripples along at and roughens them and whitens them by.
//!
//! Where a stream meets the river, the two are joined as plant_life's water join tool
//! (`assets/terrain/hydroxus-terrain/hydroxus_water_join.py`) joins them - see [`join`], a port of
//! its `join_water_channels` - and both are told of the junction it finds, where the shader blends
//! their ripples into one. The water is dimmed with the light of the time of day, and hazed as
//! the renderer's fog would, since it is drawn after the fog.

use crate::terrain::{Channel, Terrain, side_of};
use fyrox::core::math::TriangleDefinition;
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Vector2, Vector3, Vector4},
        color::Color,
        log::Log,
        pool::Handle,
        uuid::Uuid,
    },
    graph::SceneGraph,
    material::{
        Material, MaterialResource,
        shader::{Shader, ShaderResource},
    },
    scene::{
        Scene,
        base::BaseBuilder,
        light::directional::DirectionalLight,
        mesh::{
            MeshBuilder, RenderPath,
            buffer::{TriangleBuffer, VertexBuffer},
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
        },
        node::Node,
    },
};
use std::collections::HashMap;

const SHADER: &str = include_str!("water.shader");
/// How many times a second the ripples' own slow roll turns over, under the flow that carries
/// them: as the Birch Glade's river and stream.
const RIVER_ROLL: f32 = 0.23;
const STREAM_ROLL: f32 = 0.38;
/// How many times a second the shared pattern at a junction rolls over: as plant_life's sample.
const CONFLUENCE_ROLL: f32 = 0.28;
/// How far round a junction the two waters' ripples blend into one, in meters: as plant_life's
/// sample.
const CONFLUENCE_RADIUS: f32 = 2.2;
/// The join tool's `blend_distance`: over how far from the river's edge a stream's water eases
/// to the river's height, in meters.
const JOIN_BLEND: f32 = 0.65;
/// The join tool's EPSILON: an area, or a line's side, too small to count.
const EPSILON: f32 = 1.0e-6;
/// How near the river's edge a vertex lies to be on the seam, in meters. (The tool's 2e-5 is
/// Blender's double-precision; these are single-precision coordinates a hundred meters out.)
const ON_SEAM: f32 = 1.0e-3;
/// How long the flow's clock runs before starting over, in seconds: a whole number of the
/// shader's flow cycles (1.6 s), so starting over does not show.
const FLOW_CLOCK: f32 = 1.6 * 500.0;
/// The light in the shade in full daylight (`game.rs`'s AMBIENT), against which the water is dimmed.
const DAY_AMBIENT: Color = Color::opaque(128, 136, 146);

/// The water: each channel's ribbon, its material, and how fast its ripples roll.
#[derive(Debug, Default, PartialEq)]
pub struct Water {
    ribbons: Vec<(Handle<Node>, MaterialResource, f32)>,
    elapsed: f32,
}

impl Water {
    /// Lays the water along every channel of `terrain`.
    pub fn build(scene: &mut Scene, terrain: &Terrain) -> Self {
        let shader = match Shader::from_string(SHADER) {
            Ok(shader) => ShaderResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, shader),
            Err(error) => {
                Log::err(format!("The water's shader would not build: {error:?}; there is no water"));
                return Self::default();
            }
        };
        let river = ribbon(&terrain.channels[0]);
        let mut ribbons = Vec::new();
        let mut junctions = Vec::new();
        let river_material = water_material(&shader);
        for channel in &terrain.channels[1..] {
            let Some((stream, center)) = join(&ribbon(channel), &river) else {
                Log::warn(format!("{} does not reach the river's water; it is left out", channel.name));
                continue;
            };
            let junction = Vector4::new(center.x, center.y, center.z, CONFLUENCE_RADIUS);
            let material = water_material(&shader);
            material.data_ref().set_property("confluences", vec![junction]);
            junctions.push(junction);
            let node = stream.build(scene, channel.name, &material);
            ribbons.push((node, material, STREAM_ROLL));
        }
        Log::info(format!(
            "Water: {} streams join the river, at {}",
            junctions.len(),
            junctions.iter().map(|j| format!("({:.0}, {:.0})", j.x, j.z)).collect::<Vec<_>>().join(", ")
        ));
        // The river is told of every junction (the shader takes eight).
        river_material.data_ref().set_property("confluences", junctions.iter().take(8).copied().collect::<Vec<_>>());
        let node = river.build(scene, terrain.channels[0].name, &river_material);
        ribbons.push((node, river_material, RIVER_ROLL));
        Self { ribbons, elapsed: 0.0 }
    }

    /// Moves the water on by `dt` seconds, and lights and hazes it as `scene` is now, lit by
    /// `sun`.
    pub fn update(&mut self, scene: &Scene, sun: Handle<DirectionalLight>, dt: f32) {
        self.elapsed = (self.elapsed + dt) % FLOW_CLOCK;
        let options = &scene.rendering_options;
        let linear = |c: Color| {
            let c = c.srgb_to_linear_f32();
            Vector3::new(c.x, c.y, c.z)
        };
        // The light: the shade's, against full daylight's, and the sun's (or moon's) as high as
        // it is; and which way the light comes from, for the highlight on the ripples.
        let ambient = linear(options.ambient_lighting_color);
        let day = linear(DAY_AMBIENT);
        let mut light = ambient.component_div(&day) * 0.45;
        let mut toward_sun = None;
        if let Ok(sun) = scene.graph.try_get(sun) {
            let base = sun.base_light_ref();
            let up = sun.up_vector().try_normalize(f32::EPSILON);
            light += linear(base.color())
                * (base.intensity() * 0.55 * crate::random::smoothstep(-0.05, 0.3, up.map_or(0.0, |v| v.y)));
            toward_sun = up;
        }
        let fog = options.fog.as_ref();
        let shared = (self.elapsed * CONFLUENCE_ROLL).rem_euclid(1.0);
        for (_, material, roll) in &self.ribbons {
            let mut material = material.data_ref();
            material.set_property("flowPhase", (self.elapsed * roll).rem_euclid(1.0));
            material.set_property("flowTime", self.elapsed);
            material.set_property("confluencePhase", shared);
            material.set_property("light", light);
            if let Some(toward_sun) = toward_sun {
                material.set_property("sunDirection", toward_sun);
            }
            if let Some(fog) = fog {
                material.set_property("hazeColor", linear(fog.color));
                material.set_property("hazeDensity", fog.density);
                material.set_property("hazeStart", fog.start);
                material.set_property("hazeBase", fog.base_height);
                material.set_property("hazeFalloff", fog.height_falloff);
            }
        }
    }
}

/// A material for water: the shader, and the frame before the water was drawn to see the bed
/// through (fyrox-gfx's refraction pass copies it, and draws the water after).
fn water_material(shader: &ShaderResource) -> MaterialResource {
    let mut material = Material::from_shader(shader.clone());
    material.bind("sceneColor", fyrox_gfx::refraction::scene_color());
    MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, material)
}

/// A point of a water surface: where it is (y up), its flow coordinates (meters downstream and
/// across), and how fast the water runs there and how steeply it falls.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vert {
    position: Vector3<f32>,
    uv: Vector2<f32>,
    flow: Vector2<f32>,
}

impl Vert {
    /// Where it is across the ground: the tool's XY.
    fn xz(&self) -> Vector2<f32> {
        Vector2::new(self.position.x, self.position.z)
    }

    fn lerp(&self, other: &Vert, t: f32) -> Vert {
        Vert {
            position: self.position.lerp(&other.position, t),
            uv: self.uv.lerp(&other.uv, t),
            flow: self.flow.lerp(&other.flow, t),
        }
    }
}

/// A water surface: its vertices and triangles; and its boundary, the edges of only one triangle
/// each (its banks, and its ends).
#[derive(Debug, Clone, PartialEq)]
struct Surface {
    vertices: Vec<Vert>,
    triangles: Vec<[u32; 3]>,
}

impl Surface {
    fn boundary(&self) -> Vec<(Vert, Vert)> {
        let mut edges: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        edges
            .into_iter()
            .filter(|(_, count)| *count == 1)
            .map(|((a, b), _)| (self.vertices[a as usize], self.vertices[b as usize]))
            .collect()
    }

    fn build(&self, scene: &mut Scene, name: &str, material: &MaterialResource) -> Handle<Node> {
        let vertices: Vec<StaticVertex> = self
            .vertices
            .iter()
            .map(|v| StaticVertex {
                position: v.position,
                tex_coord: v.uv,
                // Not a normal: how fast the water runs here, and how steeply it falls, for the
                // shader to carry the ripples along by (the water's surface is flat but for its
                // ripples, which the shader tilts itself).
                normal: Vector3::new(v.flow.x, v.flow.y, 0.0),
                tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
            })
            .collect();
        let triangles = self.triangles.iter().map(|&t| TriangleDefinition(t)).collect();
        let data = SurfaceData::new(
            VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"),
            TriangleBuffer::new(triangles),
        );
        MeshBuilder::new(BaseBuilder::new().with_name(name).with_cast_shadows(false))
            .with_surfaces(vec![
                SurfaceBuilder::new(SurfaceResource::new_embedded(data))
                    .with_material(material.clone())
                    .build(),
            ])
            .with_render_path(RenderPath::Forward)
            .build(&mut scene.graph)
            .to_base()
    }
}

/// The ribbon along the whole of `channel`, at its water's height: a pair of points across it at
/// each of its points, its right edge then its left, each at its bank's waterline and on under
/// the bank.
fn ribbon(channel: &Channel) -> Surface {
    let points = &channel.points;
    let under = channel.under_banks();
    let mut vertices = Vec::with_capacity(points.len() * 2);
    for (i, &p) in points.iter().enumerate() {
        let left = side_of(points, i);
        let [right_line, left_line] = channel.waterlines[i];
        let (right, left_reach) = (right_line + under, left_line + under);
        let y = channel.water(i);
        let flow = Vector2::new(channel.speed(i), channel.slopes[i]);
        for (at, across) in [(p - left * right, 0.0), (p + left * left_reach, right + left_reach)] {
            vertices.push(Vert { position: Vector3::new(at.x, y, at.y), uv: Vector2::new(channel.distances[i], across), flow });
        }
    }
    let triangles = (0..points.len() as u32 - 1)
        .flat_map(|i| {
            let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
            [[a, b, c], [b, d, c]]
        })
        .collect();
    Surface { vertices, triangles }
}

/// The tool's `signed_area`: which side of the line from `a` to `b` `point` is, across the ground
/// (positive to the left).
fn signed_area(a: Vector2<f32>, b: Vector2<f32>, point: Vector2<f32>) -> f32 {
    (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
}

/// The tool's `split_polygon`: `polygon` split by the line from `a` to `b`, into its parts on the
/// left and on the right.
fn split_polygon(polygon: &[Vert], a: Vector2<f32>, b: Vector2<f32>) -> (Vec<Vert>, Vec<Vert>) {
    let (mut positive, mut negative) = (Vec::new(), Vec::new());
    for (i, start) in polygon.iter().enumerate() {
        let end = &polygon[(i + 1) % polygon.len()];
        let (d0, d1) = (signed_area(a, b, start.xz()), signed_area(a, b, end.xz()));
        if d0 >= -EPSILON {
            positive.push(*start);
        }
        if d0 <= EPSILON {
            negative.push(*start);
        }
        if (d0 > EPSILON && d1 < -EPSILON) || (d1 > EPSILON && d0 < -EPSILON) {
            let crossing = start.lerp(end, d0 / (d0 - d1));
            positive.push(crossing);
            negative.push(crossing);
        }
    }
    (positive, negative)
}

/// The tool's `subtract_triangle`: the convex pieces of `polygon` outside `triangle`.
fn subtract_triangle(polygon: Vec<Vert>, triangle: [Vector2<f32>; 3]) -> Vec<Vec<Vert>> {
    let triangle = if signed_area(triangle[0], triangle[1], triangle[2]) < 0.0 {
        [triangle[2], triangle[1], triangle[0]]
    } else {
        triangle
    };
    let mut pieces = Vec::new();
    let mut polygon = polygon;
    for k in 0..3 {
        let (a, b) = (triangle[k], triangle[(k + 1) % 3]);
        let (inside, outside) = split_polygon(&polygon, a, b);
        if outside.len() >= 3 {
            pieces.push(outside);
        }
        polygon = inside;
        if polygon.len() < 3 {
            break;
        }
    }
    pieces
}

/// The least and most x and z of some points.
fn bounds(points: impl IntoIterator<Item = Vector2<f32>>) -> [f32; 4] {
    points.into_iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
        [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)]
    })
}

/// plant_life's `join_water_channels`, ported: removes the part of the `tributary` surface the
/// `receiving` one covers, and eases the tributary's height into the receiver's within
/// [`JOIN_BLEND`] of the receiver's edges, keeping its flow coordinates. Returns the trimmed
/// tributary and the middle of the seam where they meet (the junction); [`None`] if the receiver
/// covers the whole tributary, or the tributary does not reach the receiver's edge.
fn join(tributary: &Surface, receiving: &Surface) -> Option<(Surface, Vector3<f32>)> {
    let receiver_triangles: Vec<([f32; 4], [Vector2<f32>; 3])> = receiving
        .triangles
        .iter()
        .filter_map(|t| {
            let points = t.map(|i| receiving.vertices[i as usize].xz());
            (signed_area(points[0], points[1], points[2]).abs() > EPSILON).then(|| (bounds(points), points))
        })
        .collect();
    let banks = receiving.boundary();

    let mut seam = Vec::new();
    let mut conform = |vertex: Vert| {
        let point = vertex.xz();
        let mut nearest: Option<Vector3<f32>> = None;
        let mut distance = JOIN_BLEND;
        for (a, b) in &banks {
            let along = b.xz() - a.xz();
            if along.norm_squared() <= EPSILON {
                continue;
            }
            let t = ((point - a.xz()).dot(&along) / along.norm_squared()).clamp(0.0, 1.0);
            let candidate = a.position.lerp(&b.position, t);
            let separation = (point - Vector2::new(candidate.x, candidate.z)).norm();
            if separation < distance {
                nearest = Some(candidate);
                distance = separation;
            }
        }
        let mut result = vertex;
        if let Some(nearest) = nearest {
            let weight = 1.0 - distance / JOIN_BLEND;
            result.position.y += (nearest.y - result.position.y) * weight * weight * (3.0 - 2.0 * weight);
        }
        if distance <= ON_SEAM {
            seam.push(result.position);
        }
        result
    };

    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut indices: HashMap<[i64; 4], u32> = HashMap::new();
    for triangle in &tributary.triangles {
        let polygon: Vec<Vert> = triangle.iter().map(|&i| tributary.vertices[i as usize]).collect();
        let [left, bottom, right, top] = bounds(polygon.iter().map(Vert::xz));
        let mut pieces = vec![polygon];
        for ([west, south, east, north], receiver) in &receiver_triangles {
            if left > *east || right < *west || bottom > *north || top < *south {
                continue;
            }
            pieces = pieces.into_iter().flat_map(|piece| subtract_triangle(piece, *receiver)).collect();
            if pieces.is_empty() {
                break;
            }
        }
        for piece in pieces {
            let first = piece[0];
            for pair in piece[1..].windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if signed_area(first.xz(), a.xz(), b.xz()).abs() <= EPSILON {
                    continue;
                }
                let mut face = [0u32; 3];
                for (corner, vertex) in [first, a, b].into_iter().enumerate() {
                    let key = [vertex.position.x, vertex.position.z, vertex.uv.x, vertex.uv.y].map(|v| (v * 1.0e6).round() as i64);
                    face[corner] = *indices.entry(key).or_insert_with(|| {
                        vertices.push(conform(vertex));
                        vertices.len() as u32 - 1
                    });
                }
                if face[0] != face[1] && face[1] != face[2] && face[0] != face[2] {
                    faces.push(face);
                }
            }
        }
    }
    if faces.is_empty() || seam.len() < 2 {
        return None;
    }
    let center = seam.iter().sum::<Vector3<f32>>() / seam.len() as f32;
    Some((Surface { vertices, triangles: faces }, center))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `point` is under the `surface`.
    fn covered(surface: &Surface, point: Vector2<f32>) -> bool {
        surface.triangles.iter().any(|t| {
            let [a, b, c] = t.map(|i| surface.vertices[i as usize].xz());
            let (d0, d1, d2) = (signed_area(a, b, point), signed_area(b, c, point), signed_area(c, a, point));
            let tol = 1.0e-4;
            (d0 > tol && d1 > tol && d2 > tol) || (d0 < -tol && d1 < -tol && d2 < -tol)
        })
    }

    #[test]
    fn a_joined_stream_meets_the_river_without_overlapping_it_or_a_gap() {
        for seed in 1..6 {
            let terrain = Terrain::new(seed);
            let river = ribbon(&terrain.channels[0]);
            let banks = river.boundary();
            for channel in &terrain.channels[1..] {
                let (stream, center) = join(&ribbon(channel), &river).expect("reaches the river");
                // No part of it over the river: each triangle's middle is clear of it.
                for t in &stream.triangles {
                    let middle = t.iter().map(|&i| stream.vertices[i as usize].xz()).sum::<Vector2<f32>>() / 3.0;
                    assert!(!covered(&river, middle), "seed {seed}: {} over the river at {middle:?}", channel.name);
                }
                // Its seam lies along the river's edge, at the river's height: the two meet.
                let on_seam: Vec<&Vert> = stream
                    .vertices
                    .iter()
                    .filter(|v| {
                        banks.iter().any(|(a, b)| {
                            let along = b.xz() - a.xz();
                            let t = ((v.xz() - a.xz()).dot(&along) / along.norm_squared().max(1e-9)).clamp(0.0, 1.0);
                            (v.xz() - a.xz().lerp(&b.xz(), t)).norm() <= ON_SEAM
                        })
                    })
                    .collect();
                assert!(on_seam.len() >= 2, "seed {seed}: {} has no seam", channel.name);
                let seam_width = on_seam.iter().map(|v| (v.xz() - Vector2::new(center.x, center.z)).norm()).fold(0.0, f32::max) * 2.0;
                assert!(seam_width > channel.half_width * 0.5, "seed {seed}: {} meets the river along only {seam_width} m", channel.name);
                for v in &on_seam {
                    let (_, water) = terrain.along(0, v.position.x, v.position.z).expect("by the river");
                    assert!((v.position.y - water).abs() < 0.02, "seed {seed}: {} at {} where the river is {water}", channel.name, v.position.y);
                }
            }
        }
    }

    #[test]
    fn steeper_water_runs_faster() {
        let terrain = Terrain::new(1);
        for channel in &terrain.channels {
            for i in 0..channel.points.len() {
                for j in 0..channel.points.len() {
                    if channel.slopes[i] > channel.slopes[j] + 1e-4 {
                        assert!(channel.speed(i) >= channel.speed(j));
                    }
                }
            }
        }
        // The river's grade gives it a walking pace, a meter a second or so.
        let river = &terrain.channels[0];
        let middle = river.speed(river.points.len() / 2);
        assert!((0.6..2.0).contains(&middle), "the river runs at {middle} m/s");
    }
}
