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
//! their ripples into one. The water is hazed as the renderer's fog would, since it is drawn after
//! the fog.
//!
//! The water mirrors what stands near it on screen, and ripples in rings round what stands in it -
//! rocks, and the player wading - as plant_life's `set_reflection_strength` and
//! `update_obstacles` have it (each surface told of at most eight contacts, those nearest the eye),
//! the rings drawn out downstream as far as the water runs fast there, round where it is still.
//!
//! The ponds and springs are each a level surface over their basin, as the tool's Pond or Spring
//! lays it (see [`basin`]), their ripples slower and lower and their foam less, as plant_life's
//! sample sets them; each spring's stream is joined to it as the stream is to the river.

use crate::terrain::{BASIN_WATER, Basin, Channel, Terrain, side_of};
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
    material::{
        Material, MaterialResource,
        shader::{Shader, ShaderResource},
    },
    scene::{
        Scene,
        base::BaseBuilder,
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
/// And round where a stream leaves its spring: as the Birch Glade's.
const SPRING_CONFLUENCE_RADIUS: f32 = 3.0;
/// The ponds' and springs' ripples: how fast they roll, how high, and how much foam, as
/// plant_life's sample sets them (`FlowingWater::attach` and `set_surface_activity`). Still
/// water does not run: no speed to carry the ripples along.
const POND_ROLL: f32 = 0.035;
const POND_ACTIVITY: (f32, f32) = (0.30, 0.0);
const SPRING_ROLL: f32 = 0.10;
const SPRING_ACTIVITY: (f32, f32) = (0.52, FOAM);
/// How strongly each kind of water mirrors the scenery near it: as plant_life's sample.
/// (All of them fully: clearer water mirrors more. plant_life's sample has 0.55, 0.35, 0.60 and
/// 0.78.)
const RIVER_REFLECTION: f32 = 1.0;
const STREAM_REFLECTION: f32 = 1.0;
const SPRING_REFLECTION: f32 = 1.0;
const POND_REFLECTION: f32 = 1.0;
/// How much foam the water has: none, so that it is clear, not milky (plant_life's is 0.72, and
/// its springs' 0.16).
const FOAM: f32 = 0.0;
/// How many contacts a surface's shader takes, and how many times a second their rings go out.
const MOST_CONTACTS: usize = 8;
/// Which way along the water's flow coordinates its current runs, for the rings round what stands
/// in it: downstream. (How far they are drawn out is the shader's, by how fast the water runs
/// there: not at all on a pond or a spring, whose water does not run.)
const CONTACT_CURRENT: f32 = 1.0;
const CONTACT_RINGS: f32 = 1.15;
/// How many sides a basin's surface has round: as the tool's.
const BASIN_SEGMENTS: usize = 72;
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

/// How many of the scene's lights the water glints with: those nearest the eye.
const MOST_LAMPS: usize = 8;

/// A light the water glints with: where it is, in the shader's terms - its place and reach; its
/// colour times its brightness, and where its beam is whole (the cosine of the angle from its
/// middle); and which way it shines and where its beam is gone (for a light all round, any way
/// and -1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lamp {
    pub place: Vector4<f32>,
    pub color: Vector4<f32>,
    pub aim: Vector4<f32>,
}

/// Every point and spot light shown in `graph`, the [`MOST_LAMPS`] nearest `eye` (by how far
/// their light has to go to reach it), for the water to glint with: the flashlight and the
/// lantern, and whatever else gives light. (The sun and the moon are the sky's: see [`Lights`].)
pub fn lamps_near(graph: &fyrox::scene::graph::Graph, eye: Vector3<f32>) -> Vec<Lamp> {
    use fyrox::scene::light::{point::PointLight, spot::SpotLight};
    let linear = |c: Color| {
        let c = c.srgb_to_linear_f32();
        Vector3::new(c.x, c.y, c.z)
    };
    let mut lamps: Vec<(f32, Lamp)> = graph
        .linear_iter()
        .filter(|node| node.global_visibility())
        .filter_map(|node| {
            let (light, reach, aim) = if let Some(spot) = node.cast::<SpotLight>() {
                let light = spot.base_light_ref();
                // A spot light shines down its own -y; its angles are whole ones.
                let down = -light.up_vector().try_normalize(1e-6)?;
                let whole = (spot.hotspot_cone_angle() * 0.5).cos();
                let gone = ((spot.hotspot_cone_angle() + spot.falloff_angle_delta()) * 0.5).cos();
                (light, spot.distance(), (down, gone, whole))
            } else if let Some(point) = node.cast::<PointLight>() {
                (point.base_light_ref(), point.radius(), (Vector3::y(), -1.0, -1.0))
            } else {
                return None;
            };
            let color = linear(light.color()) * light.intensity();
            if color.max() <= 0.0 || reach <= 0.0 {
                return None;
            }
            let position = light.global_position();
            let (direction, gone, whole) = aim;
            Some((
                (position - eye).norm() - reach,
                Lamp {
                    place: Vector4::new(position.x, position.y, position.z, reach),
                    color: Vector4::new(color.x, color.y, color.z, whole),
                    aim: Vector4::new(direction.x, direction.y, direction.z, gone),
                },
            ))
        })
        .collect();
    lamps.sort_by(|a, b| a.0.total_cmp(&b.0));
    lamps.into_iter().take(MOST_LAMPS).map(|(_, lamp)| lamp).collect()
}

/// What lights the water now: the sun (unit vector toward it, even at night, as the shader dims
/// its reflection and foam by how far down it is); the sky's colours overhead and at the horizon
/// (sRGB), when days go by; the moon (toward it); how much it is night, 0 by day to 1, for the
/// moon and the stars; how far round the stars have turned, in radians; and the lamps.
#[derive(Debug, Clone, PartialEq)]
pub struct Lights {
    pub toward_sun: Vector3<f32>,
    pub sky: Option<(Color, Color)>,
    pub toward_moon: Vector3<f32>,
    pub night: f32,
    pub star_turn: f32,
    pub lamps: Vec<Lamp>,
}

/// One water surface in the scene.
#[derive(Debug, PartialEq)]
struct Ribbon {
    material: MaterialResource,
    /// How fast its ripples roll.
    roll: f32,
    /// Its triangles, and the least and most x and z of each, to find what stands in it.
    triangles: Vec<([Vector3<f32>; 3], [f32; 4])>,
    bounds: [f32; 4],
    /// What stands in it and stays: each one's middle, and its contact for the shader (its middle
    /// across, and its half widths).
    standing: Vec<(Vector3<f32>, Vector4<f32>)>,
}

impl Ribbon {
    fn new(surface: &Surface, material: MaterialResource, roll: f32) -> Self {
        {
            let mut material = material.data_ref();
            material.set_property("obstacleCurrent", CONTACT_CURRENT);
            // The ripples along the banks' shallows, as plant_life's; how strong by how fast the
            // water runs is the shader's, so none on still water.
            material.set_property("bankRippleStrength", 1.0f32);
        }
        let triangles: Vec<([Vector3<f32>; 3], [f32; 4])> = surface
            .triangles
            .iter()
            .map(|t| {
                let corners = t.map(|i| surface.vertices[i as usize].position);
                (corners, bounds(corners.iter().map(|p| Vector2::new(p.x, p.z))))
            })
            .collect();
        let bounds = bounds(triangles.iter().flat_map(|(c, _)| c.iter().map(|p| Vector2::new(p.x, p.z))));
        Self { material, roll, triangles, bounds, standing: Vec::new() }
    }

    /// The contact of what lies within `low` to `high` (least corner and most), if it stands in
    /// this water: plant_life's `update_obstacles`, which looks down five lines through it (its
    /// middle, and most of the way to each side) for the surface.
    fn contact(&self, low: Vector3<f32>, high: Vector3<f32>) -> Option<(Vector3<f32>, Vector4<f32>)> {
        let half = (high - low) * 0.5;
        let center = low + half;
        if !center.iter().chain(half.iter()).all(|v| v.is_finite()) || half.x <= 0.0 || half.z <= 0.0 || half.y <= 0.012 {
            return None;
        }
        let [west, south, east, north] = self.bounds;
        if high.x < west || low.x > east || high.z < south || low.z > north {
            return None;
        }
        let wet = [(0.0, 0.0), (-0.72, 0.0), (0.72, 0.0), (0.0, -0.72), (0.0, 0.72)].into_iter().any(|(x, z)| {
            let at = Vector2::new(center.x + x * half.x, center.z + z * half.z);
            self.triangles.iter().any(|(corners, [w, s, e, n])| {
                if at.x < *w || at.x > *e || at.y < *s || at.y > *n {
                    return false;
                }
                let [a, b, c] = corners.map(|p| Vector2::new(p.x, p.z));
                let area = signed_area(a, b, c);
                if area.abs() <= EPSILON {
                    return false;
                }
                let (u, v) = (signed_area(b, c, at) / area, signed_area(c, a, at) / area);
                let w = 1.0 - u - v;
                if u < 0.0 || v < 0.0 || w < 0.0 {
                    return false;
                }
                let y = corners[0].y * u + corners[1].y * v + corners[2].y * w;
                y > low.y + 0.012 && y < high.y - 0.012
            })
        });
        wet.then(|| (center, Vector4::new(center.x, center.z, half.x.max(0.035), half.z.max(0.035))))
    }
}

/// The water: each surface.
#[derive(Debug, Default, PartialEq)]
pub struct Water {
    ribbons: Vec<Ribbon>,
    elapsed: f32,
    rings: f32,
    /// Each channel's water surface, as laid and joined, triangle by triangle: its corners,
    /// and their flow coordinates (meters downstream, and across) - for the bed's gravel.
    pub surfaces: Vec<Vec<([Vector3<f32>; 3], [Vector2<f32>; 3])>>,
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
        let mut surfaces = Vec::new();
        let mut junctions = Vec::new();
        let river_material = water_material(&shader, RIVER_REFLECTION);
        let pools: Vec<(&Basin, Surface)> = terrain.basins.iter().map(|b| (b, basin(b))).collect();
        let mut pool_joins: Vec<Option<Vector4<f32>>> = vec![None; pools.len()];
        for (c, channel) in terrain.channels.iter().enumerate().skip(1) {
            // As the Birch Glade joins Fern Stream: to its spring first, then to the river.
            let mut stream = ribbon(channel);
            let mut confluences = Vec::new();
            if let Some(p) = pools.iter().position(|(b, _)| b.outlet.is_some_and(|(_, fed)| fed == c)) {
                match join(&stream, &pools[p].1) {
                    Some((joined, center)) => {
                        stream = joined;
                        let junction = Vector4::new(center.x, center.y, center.z, SPRING_CONFLUENCE_RADIUS);
                        pool_joins[p] = Some(junction);
                        confluences.push(junction);
                    }
                    None => Log::warn(format!("{} does not run out of {}", channel.name, pools[p].0.name)),
                }
            }
            let Some((stream, center)) = join(&stream, &river) else {
                Log::warn(format!("{} does not reach the river's water; it is left out", channel.name));
                continue;
            };
            let junction = Vector4::new(center.x, center.y, center.z, CONFLUENCE_RADIUS);
            confluences.insert(0, junction);
            let material = water_material(&shader, STREAM_REFLECTION);
            material.data_ref().set_property("confluences", confluences);
            junctions.push(junction);
            stream.build(scene, channel.name, &material);
            ribbons.push(Ribbon::new(&stream, material, STREAM_ROLL));
            surfaces.push(stream.corners());
        }
        for ((pool, surface), joined) in pools.iter().zip(&pool_joins) {
            let (roll, (waves, foam), mirror) = if pool.is_spring() {
                (SPRING_ROLL, SPRING_ACTIVITY, SPRING_REFLECTION)
            } else {
                (POND_ROLL, POND_ACTIVITY, POND_REFLECTION)
            };
            let material = water_material(&shader, mirror);
            {
                let mut material = material.data_ref();
                material.set_property("waveStrength", waves);
                material.set_property("foamStrength", foam);
                material.set_property("confluences", joined.iter().copied().collect::<Vec<_>>());
            }
            surface.build(scene, pool.name, &material);
            ribbons.push(Ribbon::new(surface, material, roll));
            surfaces.push(surface.corners());
        }
        Log::info(format!(
            "Water: {} streams join the river, at {}; {} run out of their springs, at {}; ponds at {}",
            junctions.len(),
            junctions.iter().map(|j| format!("({:.0}, {:.0})", j.x, j.z)).collect::<Vec<_>>().join(", "),
            pool_joins.iter().flatten().count(),
            pools.iter().filter(|(b, _)| b.is_spring()).map(|(b, _)| format!("({:.0}, {:.0})", b.center.x, b.center.y)).collect::<Vec<_>>().join(", "),
            pools.iter().filter(|(b, _)| !b.is_spring()).map(|(b, _)| format!("({:.0}, {:.0})", b.center.x, b.center.y)).collect::<Vec<_>>().join(", "),
        ));
        // The river is told of every junction (the shader takes eight).
        river_material.data_ref().set_property("confluences", junctions.iter().take(8).copied().collect::<Vec<_>>());
        river.build(scene, terrain.channels[0].name, &river_material);
        ribbons.push(Ribbon::new(&river, river_material, RIVER_ROLL));
        surfaces.insert(0, river.corners());
        Self { ribbons, elapsed: 0.0, rings: 0.0, surfaces }
    }

    /// Notes which of the things lying within `bounds` (each one's least corner and most) stand
    /// in the water and stay there - rocks - for it to ripple round. Returns how many.
    pub fn set_standing(&mut self, bounds: &[(Vector3<f32>, Vector3<f32>)]) -> usize {
        let mut count = 0;
        for ribbon in &mut self.ribbons {
            ribbon.standing = bounds.iter().filter_map(|&(low, high)| ribbon.contact(low, high)).collect();
            count += ribbon.standing.len();
        }
        count
    }

    /// Moves the water on by `dt` seconds, lit as `lights` say, hazed as `scene`'s fog is now;
    /// seen from `eye`, and the one moving thing that may stand in it lying within `wading` (least
    /// corner and most), rippling round it as round the rocks.
    pub fn update(&mut self, scene: &Scene, lights: &Lights, eye: Vector3<f32>, wading: (Vector3<f32>, Vector3<f32>), dt: f32) {
        self.elapsed = (self.elapsed + dt) % FLOW_CLOCK;
        self.rings = (self.rings + dt * CONTACT_RINGS).rem_euclid(1.0);
        let linear = |c: Color| {
            let c = c.srgb_to_linear_f32();
            Vector3::new(c.x, c.y, c.z)
        };
        let fog = scene.rendering_options.fog.as_ref();
        let shared = (self.elapsed * CONFLUENCE_ROLL).rem_euclid(1.0);
        for ribbon in &self.ribbons {
            // The contacts nearest the eye.
            let mut contacts: Vec<(f32, Vector4<f32>)> = ribbon
                .standing
                .iter()
                .chain(ribbon.contact(wading.0, wading.1).as_ref())
                .map(|(center, contact)| ((center - eye).norm_squared(), *contact))
                .collect();
            contacts.sort_by(|a, b| a.0.total_cmp(&b.0));
            contacts.truncate(MOST_CONTACTS);
            let mut material = ribbon.material.data_ref();
            material.set_property("obstaclePhase", self.rings);
            material.set_property("obstacleCount", contacts.len() as i32);
            material.set_property("obstacles", contacts.into_iter().map(|(_, c)| c).collect::<Vec<_>>());
            material.set_property("flowPhase", (self.elapsed * ribbon.roll).rem_euclid(1.0));
            material.set_property("flowTime", self.elapsed);
            material.set_property("confluencePhase", shared);
            material.set_property("sunDirection", lights.toward_sun);
            material.set_property("moonDirection", lights.toward_moon);
            material.set_property("night", lights.night);
            material.set_property("starTurn", lights.star_turn);
            material.set_property("lampCount", lights.lamps.len() as i32);
            material.set_property("lampPlaces", lights.lamps.iter().map(|l| l.place).collect::<Vec<_>>());
            material.set_property("lampColors", lights.lamps.iter().map(|l| l.color).collect::<Vec<_>>());
            material.set_property("lampAims", lights.lamps.iter().map(|l| l.aim).collect::<Vec<_>>());
            if let Some((zenith, horizon)) = lights.sky {
                material.set_property("skyZenith", linear(zenith));
                material.set_property("skyHorizon", linear(horizon));
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

/// A material for water, refracting, and mirroring the scenery near it by `mirror`: as
/// plant_life's `set_refraction_strength(1.0)` and `set_reflection_strength`, which have the
/// renderer copy the lit opaque scene before the forward pass for the water to bend the view of
/// its bed through, and to trace its reflections in.
fn water_material(shader: &ShaderResource, mirror: f32) -> MaterialResource {
    let mut material = Material::from_shader(shader.clone());
    material.set_property("sceneColorWeight", 1.0f32);
    material.set_property("reflectionWeight", mirror);
    material.set_property("foamStrength", FOAM);
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

    /// Its triangles' corners and their flow coordinates.
    fn corners(&self) -> Vec<([Vector3<f32>; 3], [Vector2<f32>; 3])> {
        self.triangles
            .iter()
            .map(|t| (t.map(|i| self.vertices[i as usize].position), t.map(|i| self.vertices[i as usize].uv)))
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

/// A basin's water, as the tool's `create_basin` lays it: level, a fan from its middle out to
/// [`BASIN_WATER`] of its shoreline, under its banks; its flow coordinates in meters across it
/// from its corner. Still, so its ripples are not carried along.
fn basin(basin: &Basin) -> Surface {
    let vertex = |at: Vector2<f32>| Vert {
        position: Vector3::new(at.x, basin.level, at.y),
        uv: at - basin.center + basin.radii,
        flow: Vector2::zeros(),
    };
    let mut vertices = vec![vertex(basin.center)];
    for i in 0..BASIN_SEGMENTS {
        vertices.push(vertex(basin.at(std::f32::consts::TAU * i as f32 / BASIN_SEGMENTS as f32, BASIN_WATER)));
    }
    let n = BASIN_SEGMENTS as u32;
    let triangles = (0..n).map(|i| [0, i + 1, (i + 1) % n + 1]).collect();
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
/// [`JOIN_BLEND`] of the receiver's edges, keeping its flow coordinates. The easing is done to the
/// tributary's own vertices before it is cut, so an edge split by the cut keeps the same height
/// either side; only the points along the receiver's edge are then set to its height. Returns the trimmed
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
    let mut conform = |vertex: Vert, seam_only: bool| {
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
        let Some(nearest) = nearest else {
            return result;
        };
        if seam_only {
            if distance <= ON_SEAM {
                result.position.y = nearest.y;
                seam.push(result.position);
            }
        } else {
            let weight = 1.0 - distance / JOIN_BLEND;
            result.position.y += (nearest.y - result.position.y) * weight * weight * (3.0 - 2.0 * weight);
        }
        result
    };
    let conformed: Vec<Vert> = tributary.vertices.iter().map(|&v| conform(v, false)).collect();

    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut indices: HashMap<[i64; 4], u32> = HashMap::new();
    for triangle in &tributary.triangles {
        let polygon: Vec<Vert> = triangle.iter().map(|&i| conformed[i as usize]).collect();
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
                        vertices.push(conform(vertex, true));
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

    #[test]
    fn ripples_round_what_stands_in_the_water_only() {
        let terrain = Terrain::new(1);
        let river = ribbon(&terrain.channels[0]);
        let shader = ShaderResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, Shader::from_string(SHADER).expect("the shader parses"));
        let water = Ribbon::new(&river, water_material(&shader, RIVER_REFLECTION), RIVER_ROLL);
        let i = 150;
        let (p, y) = (terrain.channels[0].points[i], terrain.channels[0].water(i));
        let rock = |at: Vector2<f32>, bottom: f32, top: f32| {
            (Vector3::new(at.x - 0.3, bottom, at.y - 0.3), Vector3::new(at.x + 0.3, top, at.y + 0.3))
        };
        // Through the surface, in the middle of the river: its contact there.
        let (low, high) = rock(p, y - 0.4, y + 0.3);
        let (_, contact) = water.contact(low, high).expect("a rock standing in the river");
        assert!((contact.x - p.x).abs() < 1e-4 && (contact.z - 0.3).abs() < 1e-4);
        // Under the surface, or above it, or on dry land: none.
        assert!(water.contact(rock(p, y - 0.6, y - 0.1).0, rock(p, y - 0.6, y - 0.1).1).is_none());
        assert!(water.contact(rock(p, y + 0.1, y + 0.6).0, rock(p, y + 0.1, y + 0.6).1).is_none());
        let dry = p + side_of(&terrain.channels[0].points, i) * 8.0;
        assert!(water.contact(rock(dry, y - 0.4, y + 0.3).0, rock(dry, y - 0.4, y + 0.3).1).is_none());
    }
}
