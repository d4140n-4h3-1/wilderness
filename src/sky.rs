//! The sky (see `sky.shader`): a box round the eye, hung from the camera so it goes wherever the
//! eye does, drawn at the far plane with a shader that works out the sky in each direction - the
//! gradient, the glow toward the rising or setting sun, the sun, the moon, and the stars - from the
//! time of day ([`crate::day::Lighting`]).

use crate::day::Lighting;
use fyrox::graph::SceneGraph;
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Matrix4, Vector3},
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
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
        },
        node::Node,
    },
};

const SHADER: &str = include_str!("sky.shader");
/// How big the box is, in meters: any size the camera's near plane does not cut into does, since
/// the shader puts it at the far plane.
const SIZE: f32 = 10.0;

#[derive(Debug, Default, PartialEq)]
pub struct Sky {
    material: Option<MaterialResource>,
    elapsed: f32,
}

impl Sky {
    /// Hangs the sky from `camera`.
    pub fn build(scene: &mut Scene, camera: Handle<Node>) -> Self {
        let shader = match Shader::from_string(SHADER) {
            Ok(shader) => ShaderResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, shader),
            Err(error) => {
                Log::err(format!("The sky's shader would not build: {error:?}; the sky is plain"));
                return Self::default();
            }
        };
        let material = MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, Material::from_shader(shader));
        let data = SurfaceData::make_cube(Matrix4::new_scaling(SIZE));
        let sky = MeshBuilder::new(BaseBuilder::new().with_name("Sky").with_cast_shadows(false))
            .with_surfaces(vec![
                SurfaceBuilder::new(SurfaceResource::new_embedded(data))
                    .with_material(material.clone())
                    .build(),
            ])
            .with_render_path(RenderPath::Forward)
            .build(&mut scene.graph)
            .to_base::<Node>();
        scene.graph.link_nodes(sky, camera);
        Self { material: Some(material), elapsed: 0.0 }
    }

    /// The sky as `lit`, `dt` seconds on.
    pub fn update(&mut self, lit: &Lighting, dt: f32) {
        let Some(material) = &self.material else {
            return;
        };
        self.elapsed = (self.elapsed + dt) % 1000.0;
        let linear = |c: Color| {
            let c = c.srgb_to_linear_f32();
            Vector3::new(c.x, c.y, c.z)
        };
        let mut material = material.data_ref();
        material.set_property("zenith", linear(lit.zenith));
        material.set_property("horizon", linear(lit.haze));
        material.set_property("glow", linear(lit.sky));
        material.set_property("towardSun", lit.toward_sun);
        material.set_property("towardMoon", lit.toward_moon);
        material.set_property("sunColor", linear(lit.sun_color));
        material.set_property("twilight", lit.twilight);
        material.set_property("stars", lit.stars);
        material.set_property("starTurn", lit.star_turn);
        material.set_property("time", self.elapsed);
    }
}
