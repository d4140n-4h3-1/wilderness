//! The player: a capsule-shaped rigid body that walks, runs and jumps, with a camera at eye
//! height that the mouse turns.

use fyrox::{
    core::{
        algebra::{Point3, UnitQuaternion, Vector3},
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

    /// Moves the player on by `dt` seconds, as the held keys say.
    pub fn update(&mut self, graph: &mut Graph, dt: f32) {
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

    /// Whether the player's feet are on something, or within a few centimeters of it.
    fn grounded(&self, graph: &Graph) -> bool {
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
