//! The player: a capsule-shaped rigid body that walks, runs and jumps, with a camera at eye
//! height that the mouse turns.

use crate::terrain::Terrain;
use fyrox::{
    core::{
        algebra::{Point3, UnitQuaternion, Vector3},
        log::Log,
        pool::Handle,
    },
    keyboard::KeyCode,
    scene::{
        base::BaseBuilder,
        camera::CameraBuilder,
        collider::{Collider, ColliderBuilder, ColliderShape},
        graph::{Graph, physics::RayCastOptions},
        node::Node,
        rigidbody::{RigidBody, RigidBodyBuilder},
        transform::TransformBuilder,
    },
};

/// The capsule's radius, and half the height of its straight middle, in meters: 1.8 m tall.
const RADIUS: f32 = 0.3;
const HALF_HEIGHT: f32 = 0.6;
/// From the body's middle to the soles of its feet.
const TO_FEET: f32 = HALF_HEIGHT + RADIUS;
/// From the body's middle up to the eyes: 1.65 m above the feet.
const TO_EYES: f32 = 0.75;
/// How fast the player walks and runs, in meters per second.
const WALK: f32 = 3.5;
const RUN: f32 = 7.0;
/// How fast the player leaves the ground when jumping, in meters per second.
const JUMP: f32 = 4.8;
/// How quickly the player can change direction in the air, as a share per second.
const AIR_CONTROL: f32 = 2.0;
/// Radians the view turns per mouse count.
const SENSITIVITY: f32 = 0.0025;
/// How far the view tips up or down, in radians: not quite straight.
const PITCH_LIMIT: f32 = 1.5;
/// How far the camera sees, in meters: the haze has all but hidden everything by then.
const VIEW: f32 = 400.0;
/// How far below the ground the feet can be before the player has fallen through it, in meters:
/// wading in the deepest water keeps them on the bed, above this.
const THROUGH_THE_GROUND: f32 = 1.0;
/// The furthest the player can be moved sideways in one update, in meters, before it counts as
/// thrown rather than walked or run: a run covers about a tenth of this in a frame at 60 Hz.
const THROWN: f32 = 1.5;

/// What the player is holding down.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Held {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    run: bool,
    jump: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Player {
    body: Handle<RigidBody>,
    collider: Handle<Collider>,
    camera: Handle<Node>,
    /// The view's turn about the vertical, and its tip up or down, in radians.
    yaw: f32,
    pitch: f32,
    held: Held,
    /// Where the body's middle was last update, and where it last stood on something: to put the
    /// player back if the physics throws them or lets them fall through the ground.
    last: Option<Vector3<f32>>,
    safe: Option<Vector3<f32>>,
    /// Where the body's middle started, and how many times running the player has been put back:
    /// a place that throws them again and again is given up for the start.
    start: Vector3<f32>,
    put_back: u32,
    calm: u32,
}

impl Player {
    /// Puts the player into `graph`, with its feet at `feet`.
    pub fn spawn(graph: &mut Graph, feet: Vector3<f32>) -> Self {
        let camera = CameraBuilder::new(
            BaseBuilder::new().with_local_transform(
                TransformBuilder::new()
                    .with_local_position(Vector3::new(0.0, TO_EYES, 0.0))
                    .build(),
            ),
        )
        .with_fov(75f32.to_radians())
        .with_z_near(0.05)
        .with_z_far(VIEW)
        .build(graph)
        .to_base();

        let collider = ColliderBuilder::new(BaseBuilder::new())
            .with_shape(ColliderShape::capsule_y(HALF_HEIGHT, RADIUS))
            // No friction, so the player slides along trunks rather than sticking to them.
            .with_friction(0.0)
            .build(graph);

        let body = RigidBodyBuilder::new(
            BaseBuilder::new()
                .with_name("Player")
                .with_local_transform(
                    TransformBuilder::new()
                        .with_local_position(feet + Vector3::new(0.0, TO_FEET, 0.0))
                        .build(),
                )
                .with_child(collider)
                .with_child(camera),
        )
        .with_locked_rotations(true)
        .with_can_sleep(false)
        .build(graph);

        Self {
            body,
            collider,
            camera,
            start: feet + Vector3::new(0.0, TO_FEET + 0.05, 0.0),
            ..Default::default()
        }
    }

    /// Where the player's feet are.
    pub fn feet(&self, graph: &Graph) -> Vector3<f32> {
        graph[self.body].global_position() - Vector3::new(0.0, TO_FEET, 0.0)
    }

    /// Which way the player looks, and which way is to their right, in the world. (The view is
    /// right-handed: the camera's own x runs to the left of the screen.)
    pub fn view_axes(&self, graph: &Graph) -> (Vector3<f32>, Vector3<f32>) {
        let camera = &graph[self.camera];
        (camera.look_vector(), -camera.side_vector())
    }

    /// The camera, which looks along its +z with +x to the left of the screen.
    pub fn camera(&self) -> Handle<Node> {
        self.camera
    }

    /// The view's turn about the vertical and its tip down, in radians.
    pub fn yaw_pitch(&self) -> (f32, f32) {
        (self.yaw, self.pitch)
    }

    /// Turns the view to `yaw` about the vertical and `pitch` down, in radians.
    pub fn set_view(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Turns the view by a mouse movement, in counts.
    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * SENSITIVITY;
        self.pitch = (self.pitch + dy * SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Notes a key going down or up. Whether it was one the player uses.
    pub fn key(&mut self, key: KeyCode, down: bool) -> bool {
        let held = match key {
            KeyCode::KeyW | KeyCode::ArrowUp => &mut self.held.forward,
            KeyCode::KeyS | KeyCode::ArrowDown => &mut self.held.back,
            KeyCode::KeyA | KeyCode::ArrowLeft => &mut self.held.left,
            KeyCode::KeyD | KeyCode::ArrowRight => &mut self.held.right,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => &mut self.held.run,
            KeyCode::Space => &mut self.held.jump,
            _ => return false,
        };
        *held = down;
        true
    }

    /// Lets go of every key: when the window loses focus, keys let go elsewhere never reach it.
    pub fn release(&mut self) {
        self.held = Held::default();
    }

    /// Moves the player on by `dt` seconds, as the held keys say; first putting them back where
    /// they last stood, should they have fallen through the `terrain` or been thrown.
    pub fn update(&mut self, graph: &mut Graph, terrain: Option<&Terrain>, dt: f32) {
        self.keep_in_the_world(graph, terrain);
        graph[self.camera].local_transform_mut().set_rotation(
            UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.yaw)
                * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.pitch),
        );

        let grounded = self.grounded(graph);
        // The camera looks along +z, and +x is to its left.
        let forward = Vector3::new(self.yaw.sin(), 0.0, self.yaw.cos());
        let right = Vector3::new(-self.yaw.cos(), 0.0, self.yaw.sin());
        let axis = |plus: bool, minus: bool| plus as i8 as f32 - minus as i8 as f32;
        let mut wish = forward * axis(self.held.forward || crate::walking(), self.held.back)
            + right * axis(self.held.right, self.held.left);
        if let Some(direction) = wish.try_normalize(f32::EPSILON) {
            wish = direction * if self.held.run { RUN } else { WALK };
        }

        let body = &mut graph[self.body];
        let mut velocity = body.lin_vel();
        if grounded {
            velocity.x = wish.x;
            velocity.z = wish.z;
            if self.held.jump {
                velocity.y = JUMP;
            }
        } else {
            let share = (AIR_CONTROL * dt).min(1.0);
            velocity.x += (wish.x - velocity.x) * share;
            velocity.z += (wish.z - velocity.z) * share;
        }
        body.set_lin_vel(velocity);
    }

    /// The fallback for what the physics gets wrong: a player under the ground, or moved further
    /// in a frame than they can walk or run - pushed out of a rock's box, say - is put back where
    /// they last stood, and stopped.
    fn keep_in_the_world(&mut self, graph: &mut Graph, terrain: Option<&Terrain>) {
        // The body hangs from the scene's root: its local position is its place in the world,
        // and is right from the first frame, before the global one is worked out.
        let middle = **graph[self.body].local_transform().position();
        let feet = middle - Vector3::new(0.0, TO_FEET, 0.0);
        let lost = !middle.iter().all(|v| v.is_finite());
        let through = !lost && terrain.is_some_and(|t| feet.y < t.height(feet.x, feet.z) - THROUGH_THE_GROUND);
        let thrown = !lost && self.last.is_some_and(|last| (middle - last).xz().norm() > THROWN);
        if !(lost || through || thrown) {
            // A second without being put back, and those before are behind them.
            self.calm += 1;
            if self.calm > 60 {
                self.put_back = 0;
            }
            self.last = Some(middle);
            if self.grounded(graph) {
                self.safe = Some(middle);
            }
            return;
        }
        // Where they last stood; or the start, if they have never stood anywhere, or that place
        // has thrown them three times running.
        self.put_back += 1;
        self.calm = 0;
        if self.put_back > 3 {
            self.safe = None;
            self.put_back = 0;
        }
        let back = self.safe.unwrap_or(self.start);
        Log::warn(format!(
            "Player {} at {feet:?}: put back at {:?}",
            if lost { "was lost" } else if through { "fell through the ground" } else { "was thrown" },
            back - Vector3::new(0.0, TO_FEET, 0.0)
        ));
        let body = &mut graph[self.body];
        body.local_transform_mut().set_position(back);
        body.set_lin_vel(Vector3::zeros());
        self.last = Some(back);
    }

    /// How fast the player is moving, in meters a second, along x, y and z.
    pub fn velocity(&self, graph: &Graph) -> Vector3<f32> {
        graph[self.body].lin_vel()
    }

    /// Whether the player's feet are on something, or within a few centimeters of it.
    pub fn grounded(&self, graph: &Graph) -> bool {
        // A rising player is jumping, not standing, even at the moment it leaves the ground.
        if graph[self.body].lin_vel().y > 0.5 {
            return false;
        }
        let middle = graph[self.body].global_position();
        let mut hits = Vec::new();
        graph.physics.cast_ray(
            RayCastOptions {
                ray_origin: Point3::from(middle),
                ray_direction: -Vector3::y(),
                max_len: TO_FEET + 0.1,
                groups: Default::default(),
                sort_results: false,
            },
            &mut hits,
        );
        hits.iter().any(|hit| hit.collider != self.collider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player under the ground, or thrown across it in a frame, is put back where they last
    /// stood (here, never having stood, at the start), and stopped.
    #[test]
    fn a_player_under_the_ground_or_thrown_is_put_back() {
        let terrain = Terrain::new(crate::game::SEED);
        let mut graph = Graph::new();
        let start = Vector3::new(0.0, terrain.height(0.0, 0.0), 0.0);
        let mut player = Player::spawn(&mut graph, start);
        let body = player.body;
        let middle = |graph: &Graph| **graph[body].local_transform().position();
        let put = |graph: &mut Graph, at: Vector3<f32>| {
            graph[body].local_transform_mut().set_position(at);
            graph[body].set_lin_vel(Vector3::new(0.0, -50.0, 0.0));
        };

        player.update(&mut graph, Some(&terrain), 1.0 / 60.0);
        assert!((middle(&graph) - (start + Vector3::new(0.0, TO_FEET, 0.0))).norm() < 0.1);

        // Fallen far below.
        put(&mut graph, Vector3::new(1.0, -5000.0, 1.0));
        player.update(&mut graph, Some(&terrain), 1.0 / 60.0);
        let back = middle(&graph) - Vector3::new(0.0, TO_FEET, 0.0);
        assert!((back - start).norm() < 0.2, "put back at {back:?}, not {start:?}");
        assert!(graph[body].lin_vel().y > -1.0, "and stopped");

        // Thrown 50 m in one frame.
        put(&mut graph, start + Vector3::new(50.0, 2.0, 0.0));
        player.update(&mut graph, Some(&terrain), 1.0 / 60.0);
        let back = middle(&graph) - Vector3::new(0.0, TO_FEET, 0.0);
        assert!((back - start).xz().norm() < 0.2, "put back at {back:?}");

        // A step's walk is left alone.
        let here = middle(&graph);
        put(&mut graph, here + Vector3::new(0.1, 0.0, 0.0));
        player.update(&mut graph, Some(&terrain), 1.0 / 60.0);
        assert!((middle(&graph) - here - Vector3::new(0.1, 0.0, 0.0)).norm() < 1e-4);
    }
}
