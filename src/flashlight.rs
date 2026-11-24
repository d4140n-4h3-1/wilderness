//! A flashlight in the player's right hand, aimed where they look; `F` switches it on and off.
//! The beam follows the view a little behind, as a hand would, so it trails a quick turn.

use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector3},
        color::Color,
        pool::Handle,
    },
    graph::SceneGraph,
    scene::{
        base::BaseBuilder,
        graph::Graph,
        light::{BaseLightBuilder, spot::{SpotLight, SpotLightBuilder}},
        node::Node,
        transform::TransformBuilder,
    },
};

/// Where the hand holds it, from the eye, in the camera's own axes (+x is the left of the screen,
/// +z ahead), in meters.
const HAND: Vector3<f32> = Vector3::new(-0.2, -0.3, 0.15);
/// The beam: cool white, how bright, its bright middle and the soft edge round that, in degrees,
/// and how far it reaches, in meters.
const BEAM: Color = Color::opaque(235, 242, 255);
const BRIGHTNESS: f32 = 3.0;
const HOTSPOT: f32 = 20.0;
const FALLOFF: f32 = 14.0;
const REACH: f32 = 35.0;
/// How quickly the beam catches up with the view, per second.
const FOLLOW: f32 = 18.0;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Flashlight {
    light: Handle<SpotLight>,
    /// Where the beam points, in the world.
    aim: Option<Vector3<f32>>,
}

impl Flashlight {
    /// Puts the flashlight, switched off, in the hand below `camera`.
    pub fn build(graph: &mut Graph, camera: Handle<Node>) -> Self {
        let light = SpotLightBuilder::new(
            BaseLightBuilder::new(
                BaseBuilder::new()
                    .with_name("Flashlight")
                    .with_visibility(false)
                    .with_local_transform(TransformBuilder::new().with_local_position(HAND).build()),
            )
            .with_color(BEAM)
            .with_intensity(BRIGHTNESS)
            .with_scatter_enabled(false),
        )
        .with_hotspot_cone_angle(HOTSPOT.to_radians())
        .with_falloff_angle_delta(FALLOFF.to_radians())
        .with_distance(REACH)
        .build(graph);
        graph.link_nodes(light.to_base::<Node>(), camera);
        Self { light, aim: None }
    }

    pub fn on(&self, graph: &Graph) -> bool {
        graph.try_get(self.light).is_ok_and(|light| light.visibility())
    }

    pub fn set_on(&self, graph: &mut Graph, on: bool) {
        if let Ok(light) = graph.try_get_mut(self.light) {
            light.set_visibility(on);
        }
    }

    /// Swings the beam on toward where the view, turned to `yaw` and tipped down by `pitch`,
    /// looks, by `dt` seconds.
    pub fn update(&mut self, graph: &mut Graph, (yaw, pitch): (f32, f32), dt: f32) {
        let view = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch);
        let look = view * Vector3::z();
        let aim = match self.aim {
            Some(aim) => (aim + (look - aim) * (1.0 - (-FOLLOW * dt).exp())).try_normalize(1e-6).unwrap_or(look),
            None => look,
        };
        self.aim = Some(aim);
        // A spot light shines down its own -y: turned so that runs along the aim, in the camera's
        // frame.
        let along = view.inverse() * aim;
        let rotation = UnitQuaternion::rotation_between(&-Vector3::y(), &along).unwrap_or_default();
        if let Ok(light) = graph.try_get_mut(self.light) {
            light.local_transform_mut().set_rotation(rotation);
        }
    }
}
