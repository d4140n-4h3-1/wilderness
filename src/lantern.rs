//! A lit lantern in the player's hand, low at the right of the view; `L` takes it out or puts it
//! away (see `game.rs`: the hand holds it or the flashlight, not both).
//!
//! The model is plant_life's `player-lantern.glb`: a grip, and the lantern hanging from a hinge
//! below it. The grip goes where the hand would be, turning with the view; the lantern swings
//! from the hinge as a pendulum, so it hangs straight down whichever way the player looks, and
//! sways as they walk, turn and stop. The engine's importer reads neither the file's
//! `KHR_lights_punctual` flame light nor its alpha-blended glass: the light is added here, on the
//! flame's node, and the glass, which would come in opaque and hide the flame, is left out. (The
//! game's copy of the file lists the light extension as used, not required: the importer refuses
//! a file that requires one it does not know.) The lantern casts no shadow: its light is inside
//! the cage, and every shadow ray toward the light would end on the cage.

use fyrox::{
    asset::manager::ResourceManager,
    core::{
        algebra::{UnitQuaternion, Vector3},
        color::Color,
        log::Log,
        pool::Handle,
    },
    graph::SceneGraph,
    resource::model::{Model, ModelResource, ModelResourceExtension},
    scene::{
        Scene,
        animation::AnimationPlayer,
        base::BaseBuilder,
        graph::Graph,
        light::{BaseLightBuilder, point::PointLightBuilder},
        node::Node,
    },
};

const PATH: &str = "data/props/player-lantern.glb";
/// Where the hand holds the grip, from the eye, in the camera's own axes (+x is the left of the
/// screen, +z ahead), in meters.
const HAND: Vector3<f32> = Vector3::new(-0.27, -0.07, 0.55);
/// From the hinge to the middle of the lantern's weight, in meters.
const PENDULUM: f32 = 0.13;
/// How quickly a swing dies away, as a share of the speed per second.
const DAMPING: f32 = 2.5;
const GRAVITY: f32 = 9.81;
/// The flame's light: warm, how bright, and how far it reaches, in meters.
const FLAME: Color = Color::opaque(255, 170, 100);
const FLAME_INTENSITY: f32 = 2.0;
const FLAME_REACH: f32 = 10.0;

#[derive(Debug, PartialEq)]
pub struct Lantern {
    model: ModelResource,
    root: Handle<Node>,
    hinge: Handle<Node>,
    held: bool,
    /// Whether the model would not load, which is told once.
    failed: bool,
    /// The lantern's weight and the hinge, in the world, last update: the swing is worked out
    /// from how they move.
    weight: Option<(Vector3<f32>, Vector3<f32>)>,
}

impl Lantern {
    /// Starts loading the lantern, put away.
    pub fn load(resource_manager: &ResourceManager) -> Self {
        Self {
            model: resource_manager.request::<Model>(PATH),
            root: Handle::NONE,
            hinge: Handle::NONE,
            held: false,
            failed: false,
            weight: None,
        }
    }

    pub fn held(&self) -> bool {
        self.held
    }

    /// Takes the lantern out, lit, or puts it away.
    pub fn set_held(&mut self, graph: &mut Graph, held: bool) {
        self.held = held;
        // Swinging afresh from where the hand is when it comes out.
        self.weight = None;
        self.show(graph);
    }

    fn show(&self, graph: &mut Graph) {
        if self.root.is_none() {
            return;
        }
        graph[self.root].set_visibility(self.held);
        // Visibility is not among the changes the Yoctite pass is told of: each mesh is marked
        // changed, so it is drawn or left out from this frame.
        let nodes: Vec<Handle<Node>> = graph.traverse_handle_iter(self.root).collect();
        for handle in nodes {
            graph[handle].mark_content_changed();
        }
    }

    /// Holds the lantern up before `camera`, turned to `yaw` and tipped down by `pitch`, and
    /// swings it on by `dt` seconds.
    pub fn update(&mut self, scene: &mut Scene, camera: Handle<Node>, (yaw, pitch): (f32, f32), dt: f32) {
        if self.root.is_none() {
            if self.model.is_loading() {
                return;
            }
            if !self.model.is_ok() {
                if !self.failed {
                    Log::err(format!("Could not load {PATH}; there is no lantern"));
                    self.failed = true;
                }
                return;
            }
            self.root = self.model.instantiate(scene);
            if self.root.is_none() {
                return;
            }
            self.dress(&mut scene.graph, camera);
            self.show(&mut scene.graph);
        }
        if !self.held || self.hinge.is_none() || dt <= 0.0 {
            return;
        }
        let graph = &mut scene.graph;
        let turn = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
        let view = turn * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch);
        let hinge = graph[camera].global_position() + view * (HAND + **graph[self.hinge].local_transform().position());

        // The weight, moved on by its own speed and gravity (Verlet), then kept at the
        // pendulum's length from the hinge; its speed is what that leaves, less damping.
        let resting = hinge - Vector3::y() * PENDULUM;
        let (weight, last) = self.weight.unwrap_or((resting, resting));
        let speed = (weight - last) * (1.0 - DAMPING * dt).max(0.0);
        let moved = weight + speed - Vector3::y() * GRAVITY * dt * dt;
        let hanging = (moved - hinge).try_normalize(1e-6).unwrap_or(-Vector3::y());
        self.weight = Some((hinge + hanging * PENDULUM, weight));

        // The lantern hangs along that line, its face turned with the player.
        let world = UnitQuaternion::rotation_between(&-Vector3::y(), &hanging).unwrap_or_default() * turn;
        graph[self.hinge].local_transform_mut().set_rotation(view.inverse() * world);
    }

    /// Hangs the lantern from the camera, lights it, takes out its glass and its sway clip, and
    /// keeps it from casting shadows.
    fn dress(&mut self, graph: &mut Graph, camera: Handle<Node>) {
        let nodes: Vec<Handle<Node>> = graph.traverse_handle_iter(self.root).collect();
        let named = |graph: &Graph, name: &str| nodes.iter().copied().find(|&h| graph[h].name() == name);
        for &handle in &nodes {
            if graph[handle].cast::<AnimationPlayer>().is_some() {
                graph.remove_node(handle);
            }
        }
        if let Some(glass) = named(graph, "Glass panels") {
            graph.remove_node(glass);
        }
        for &handle in &nodes {
            if let Ok(node) = graph.try_get_node_mut(handle) {
                node.set_cast_shadows(false);
            }
        }
        self.hinge = named(graph, "LanternHinge").unwrap_or_default();
        match named(graph, "Carry Flame") {
            Some(flame) => {
                let light = PointLightBuilder::new(
                    BaseLightBuilder::new(BaseBuilder::new().with_name("Lantern light"))
                        .with_color(FLAME)
                        .with_intensity(FLAME_INTENSITY),
                )
                .with_radius(FLAME_REACH)
                .build(graph);
                graph.link_nodes(light.to_base::<Node>(), flame);
            }
            None => Log::warn("The lantern has no Carry Flame node; it is unlit"),
        }
        graph.link_nodes(self.root, camera);
        graph[self.root].local_transform_mut().set_position(HAND);
    }
}
