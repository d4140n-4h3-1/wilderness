//! The water in the river and the streams: a ribbon along each channel at its water's height,
//! drawn with the Birch Glade's flowing-water shader (from plant_life's
//! `environments/birch-glade/hydroxus`), its ripples running downstream.
//!
//! The ribbon's first texture coordinates are in meters, as the shader wants them: along, how far
//! downstream; across, from one edge. The ripples move as `flowPhase` turns over, a river's
//! slower than a stream's; the water is dimmed with the light of the time of day, and hazed as the
//! renderer's fog would, since it is drawn after the fog.

use crate::terrain::{Channel, Terrain};
use fyrox::core::math::TriangleDefinition;
use fyrox::{
    asset::untyped::ResourceKind,
    graph::SceneGraph,
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

const SHADER: &str = include_str!("water.shader");
/// How many times a second the ripples' pattern turns over: the river's, a stream's.
const RIVER_FLOW: f32 = 0.23;
const STREAM_FLOW: f32 = 0.38;
/// The light in the shade in full daylight (`game.rs`'s AMBIENT), against which the water is dimmed.
const DAY_AMBIENT: Color = Color::opaque(128, 136, 146);
/// How far the ribbon reaches into the banks past where the bed meets the water, in meters, so
/// no gap shows along them.
const INTO_BANKS: f32 = 0.08;

/// The water: each channel's ribbon and its material.
#[derive(Debug, Default, PartialEq)]
pub struct Water {
    ribbons: Vec<(Handle<Node>, MaterialResource, f32)>,
    phase: f32,
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
        let mut ribbons = Vec::new();
        for (index, channel) in terrain.channels.iter().enumerate() {
            let material = MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, Material::from_shader(shader.clone()));
            let river = index == 0;
            // A stream's water ends where it meets the river's.
            let end = if river {
                channel.points.len()
            } else {
                let river = &terrain.channels[0];
                channel
                    .points
                    .iter()
                    .position(|p| terrain.nearest(p.x, p.y).is_some_and(|n| n.channel == 0 && n.distance < river.water_half_width()))
                    .map_or(channel.points.len(), |i| (i + 1).min(channel.points.len()))
            };
            if end < 2 {
                continue;
            }
            let node = ribbon(scene, channel, end, &material);
            ribbons.push((node, material, if river { RIVER_FLOW } else { STREAM_FLOW }));
        }
        Self { ribbons, phase: 0.0 }
    }

    /// Moves the ripples on by `dt` seconds, and lights and hazes the water as `scene` is now,
    /// lit by `sun`.
    pub fn update(&mut self, scene: &Scene, sun: Handle<DirectionalLight>, dt: f32) {
        self.phase += dt;
        let options = &scene.rendering_options;
        let linear = |c: Color| {
            let c = c.srgb_to_linear_f32();
            Vector3::new(c.x, c.y, c.z)
        };
        // The light: the shade's, against full daylight's, and the sun's (or moon's) as high as
        // it is.
        let ambient = linear(options.ambient_lighting_color);
        let day = linear(DAY_AMBIENT);
        let mut light = ambient.component_div(&day) * 0.45;
        if let Ok(sun) = scene.graph.try_get(sun) {
            let base = sun.base_light_ref();
            let up = sun.up_vector().try_normalize(f32::EPSILON).map_or(0.0, |v| v.y);
            light += linear(base.color()) * (base.intensity() * 0.55 * crate::random::smoothstep(-0.05, 0.3, up));
        }
        let fog = options.fog.as_ref();
        for (_, material, flow) in &self.ribbons {
            let mut material = material.data_ref();
            material.set_property("flowPhase", (self.phase * flow).rem_euclid(1.0));
            material.set_property("light", light);
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

/// The ribbon of water along `channel`'s first `end` points.
fn ribbon(scene: &mut Scene, channel: &Channel, end: usize, material: &MaterialResource) -> Handle<Node> {
    let points = &channel.points[..end];
    let half = channel.water_half_width() + INTO_BANKS;
    let mut vertices = Vec::with_capacity(points.len() * 2);
    let mut along = 0.0;
    for (i, &p) in points.iter().enumerate() {
        if i > 0 {
            along += (p - points[i - 1]).norm();
        }
        let tangent = points[(i + 1).min(points.len() - 1)] - points[i.saturating_sub(1)];
        let side = Vector2::new(-tangent.y, tangent.x).try_normalize(1e-6).unwrap_or(Vector2::x());
        let y = channel.water(i);
        for (offset, across) in [(-half, 0.0), (half, 2.0 * half)] {
            let at = p + side * offset;
            vertices.push(StaticVertex {
                position: Vector3::new(at.x, y, at.y),
                tex_coord: Vector2::new(along, across),
                normal: Vector3::y(),
                tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
            });
        }
    }
    let mut triangles = Vec::with_capacity((points.len() - 1) * 2);
    for i in 0..points.len() as u32 - 1 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        triangles.push(TriangleDefinition([a, b, c]));
        triangles.push(TriangleDefinition([b, d, c]));
    }
    let data = SurfaceData::new(
        VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"),
        TriangleBuffer::new(triangles),
    );
    MeshBuilder::new(BaseBuilder::new().with_name(channel.name).with_cast_shadows(false))
        .with_surfaces(vec![
            SurfaceBuilder::new(SurfaceResource::new_embedded(data))
                .with_material(material.clone())
                .build(),
        ])
        .with_render_path(RenderPath::Forward)
        .build(&mut scene.graph)
        .to_base()
}
