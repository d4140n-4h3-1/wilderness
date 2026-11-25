//! The wind, and every plant swaying in it.
//!
//! Each plant_life model carries nine morph targets, the shapes of its own sway, baked by the
//! collection's `bake_plant_wind.py`: a bend downwind and one across it (along a fixed direction
//! in the model's own space, [`DOWNWIND`]), the drop of a bent stem, a flutter of the outer
//! foliage (as a sine and a cosine), and the sway and rebound of its limbs (likewise). The models
//! also carry a twelve-second clip that plays them in a loop, always the same way. That clip is
//! left alone: the targets are driven from here instead, by a wind that is never the same twice.
//!
//! The wind turns and freshens slowly, over minutes; gusts, patches of stronger air some tens of
//! meters across, blow through the forest with it, so a gust is seen coming across a glade, the
//! grass bowing before the trees beside it do. Every plant answers the air where it stands as a
//! damped spring with its kind's stiffness, from the bake's own profiles, and with phases and
//! rates of its own, so no two plants move together.

use crate::random::{Noise, Random, smoothstep};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector2, Vector3},
        pool::Handle,
    },
    graph::SceneGraph,
    scene::{graph::Graph, mesh::Mesh, node::Node},
};
use std::f32::consts::TAU;

/// The morph targets, in the order the bake gives them; a model's own order is looked up by name.
pub const TARGETS: [&str; 9] = [
    "Downwind bend",
    "Crosswind bend",
    "Stem drop",
    "Flutter sine",
    "Flutter cosine",
    "Limb sway cosine",
    "Limb sway sine",
    "Limb rebound cosine",
    "Limb rebound sine",
];

/// The way a model's "Downwind bend" bends it, in its own space; "Crosswind bend" bends it
/// along [`ACROSS`].
const DOWNWIND: Vector3<f32> = Vector3::new(0.94, 0.0, 0.341_174_5);
const ACROSS: Vector3<f32> = Vector3::new(-0.341_174_5, 0.0, 0.94);

/// The bake's clip lasts this long, in seconds; the flutter and limb rates are whole numbers of
/// turns in it.
const CLIP: f32 = 12.0;
/// How fast the foliage flutters, and the limbs sway and rebound, in radians a second, as baked.
const FLUTTER_RATE: f32 = TAU / CLIP * 19.0;
const LIMB_RATES: [f32; 2] = [TAU / CLIP * 7.0, TAU / CLIP * 13.0];

/// How a kind of plant answers the wind: its sway's natural frequency, in hertz, and damping
/// ratio, as `bake_plant_wind.py` has them for its group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    pub frequency: f32,
    pub damping: f32,
}

pub const CONIFER: Profile = Profile { frequency: 0.65, damping: 0.40 };
pub const DECIDUOUS: Profile = Profile { frequency: 0.70, damping: 0.38 };
pub const SHRUB: Profile = Profile { frequency: 1.05, damping: 0.40 };
pub const FERN: Profile = Profile { frequency: 1.25, damping: 0.35 };
pub const GRASS: Profile = Profile { frequency: 1.55, damping: 0.34 };
/// Within the first distance from the player, in meters, plants sway in full detail; past the
/// second, they only bend (see [`Sway::update`]).
pub const DETAIL: (f32, f32) = (20.0, 35.0);

/// How much of a plant's fine sway is drawn, `distance` meters from the player.
pub fn detail_at(distance: f32) -> f32 {
    1.0 - smoothstep(DETAIL.0, DETAIL.1, distance)
}

pub const WILDFLOWER: Profile = Profile { frequency: 1.15, damping: 0.32 };

/// How wide a gust is, roughly, in meters.
const GUST_SIZE: f32 = 26.0;
/// How fast gusts cross the forest, in meters a second, in calm air and in the strongest.
const GUST_SPEED: (f32, f32) = (3.0, 9.0);

/// The wind over the whole forest.
#[derive(Debug, Clone, PartialEq)]
pub struct Wind {
    time: f32,
    /// How far the gusts have blown since the start, in meters along x and z.
    drift: Vector2<f32>,
    /// The way the wind blows now, as a unit vector along x and z, and its strength, about 0 to 1.
    direction: Vector2<f32>,
    strength: f32,
    /// Where it starts from and the phases of its slow changes, from the seed.
    bearing: f32,
    phases: [f32; 7],
    gusts: Noise,
    eddies: Noise,
    /// How many times it has moved on: plants far off take turns being bent by it (see
    /// [`every_at`]).
    tick: u32,
}

impl Wind {
    pub fn new(seed: u64) -> Self {
        let mut random = Random::for_place(seed, 0x77, 0x1d);
        let mut wind = Self {
            time: 0.0,
            drift: Vector2::zeros(),
            direction: Vector2::x(),
            strength: 0.0,
            bearing: random.between(0.0, TAU),
            phases: std::array::from_fn(|_| random.between(0.0, TAU)),
            gusts: Noise(random.next()),
            eddies: Noise(random.next()),
            tick: 0,
        };
        wind.update(0.0);
        wind
    }

    /// Moves the wind on by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.tick = self.tick.wrapping_add(1);
        let (t, p) = (self.time, self.phases);
        // A few slow waves, none a whole number of times another, so it never repeats: the
        // wind veers by up to about 50 degrees either way over some minutes, and dies down and
        // gets up again, now and then to almost nothing.
        let bearing = self.bearing
            + 0.50 * (0.023 * t + p[0]).sin()
            + 0.30 * (0.061 * t + p[1]).sin()
            + 0.12 * (0.170 * t + p[2]).sin();
        self.direction = Vector2::new(bearing.cos(), bearing.sin());
        self.strength = (0.52
            + 0.24 * (0.041 * t + p[3]).sin()
            + 0.14 * (0.097 * t + p[4]).sin()
            + 0.08 * (0.230 * t + p[5]).sin())
        .clamp(0.04, 1.0);
        let speed = GUST_SPEED.0 + (GUST_SPEED.1 - GUST_SPEED.0) * self.strength;
        self.drift += self.direction * speed * dt;
    }

    /// How strong the wind is now, about 0 (still) to 1.
    pub fn strength(&self) -> f32 {
        self.strength
    }

    /// The push of the wind at `x`, `z`, along x and z: its length is about 0 in still air, up
    /// to about 1.5 in the strongest gust, and averages near a half, the wind the models' bake
    /// was made for.
    pub fn at(&self, x: f32, z: f32) -> Vector2<f32> {
        let (gx, gz) = ((x - self.drift.x) / GUST_SIZE, (z - self.drift.y) / GUST_SIZE);
        let gust = smoothstep(0.22, 0.85, self.gusts.at(gx, gz));
        // Eddies turn the air a little this way and that, on a smaller scale than the gusts.
        let eddy = (self.eddies.at(gx * 2.5, gz * 2.5 + self.time * 0.05) - 0.5) * 0.9;
        let (sin, cos) = eddy.sin_cos();
        let direction = Vector2::new(
            self.direction.x * cos - self.direction.y * sin,
            self.direction.x * sin + self.direction.y * cos,
        );
        direction * self.strength * (0.3 + 1.2 * gust)
    }
}

/// One plant swaying: a spring, bent by the wind, that sets its model's morph targets.
#[derive(Debug, Clone, PartialEq)]
pub struct Sway {
    mesh: Handle<Node>,
    /// Where it stands, along x and z.
    at: Vector2<f32>,
    /// From the world into the model's own space.
    unturn: UnitQuaternion<f32>,
    /// Its natural angular frequency and damping ratio.
    omega: f32,
    zeta: f32,
    /// How much of the wind reaches it: less under a thick canopy.
    shelter: f32,
    /// How far it is bent, along x and z, in units of the bake's bends, and how fast it is
    /// bending.
    bend: Vector2<f32>,
    velocity: Vector2<f32>,
    flutter: (f32, f32),
    limbs: [(f32, f32); 2],
    /// Which of the mesh's blend shapes each of [`TARGETS`] is, if it has it.
    order: [Option<u8>; 9],
    /// Whether it was last left unbent, too far off to sway.
    still: bool,
    /// Time passed since it was last bent, and which of the updates it is bent at, far off (see
    /// [`every_at`]).
    waited: f32,
    phase: u32,
}

/// Every how many updates a plant `distance` meters from the player is bent: each one, near by;
/// further off, less often - its sway too small there for the steps to show, and thousands of
/// plants bent every update cost the game, the renderer and the shadows dearly. The time between
/// is made up for when it is bent.
pub fn every_at(distance: f32) -> u32 {
    if distance < 20.0 {
        1
    } else if distance < 45.0 {
        2
    } else {
        4
    }
}

impl Sway {
    /// A plant whose model's mesh is `mesh`, standing at `at` turned by `yaw` about the
    /// vertical; `None` if the mesh has no wind targets. `shelter` is the share of the wind that
    /// reaches it.
    pub fn new(
        graph: &Graph,
        mesh: Handle<Node>,
        at: Vector3<f32>,
        yaw: f32,
        profile: Profile,
        shelter: f32,
        random: &mut Random,
    ) -> Option<Self> {
        let shapes = graph.try_get_node(mesh).ok()?.cast::<Mesh>()?.blend_shapes();
        let order = TARGETS.map(|name| shapes.iter().position(|s| s.name == name).map(|i| i as u8));
        if order.iter().all(Option::is_none) {
            return None;
        }
        // No two alike: a little stiffer or looser, and the flutter and limbs at rates and
        // phases of their own.
        let mut jitter = |spread: f32| random.between(1.0 - spread, 1.0 + spread);
        let omega = TAU * profile.frequency * jitter(0.15);
        let zeta = profile.damping * jitter(0.1);
        let flutter_rate = FLUTTER_RATE * jitter(0.2);
        let limb_rates = LIMB_RATES.map(|rate| rate * jitter(0.2));
        Some(Self {
            mesh,
            at: at.xz(),
            unturn: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw).inverse(),
            omega,
            zeta,
            shelter,
            bend: Vector2::zeros(),
            velocity: Vector2::zeros(),
            flutter: (random.between(0.0, TAU), flutter_rate),
            limbs: limb_rates.map(|rate| (random.between(0.0, TAU), rate)),
            order,
            still: false,
            waited: 0.0,
            // From where it stands, not drawn at random: that would grow a different forest.
            phase: (at.x * 7.31 + at.z * 13.7).abs() as u32,
        })
    }

    /// Where it stands, along x and z.
    pub fn at(&self) -> Vector2<f32> {
        self.at
    }

    /// Bends it by the wind for `dt` seconds more, and poses its model so, its sway at `shown`
    /// of its strength: 0 leaves it unbent, and costs nothing once it is (the renderer skips
    /// shapes at no weight). Its stem's drop and its flutter and limbs, too fine to see far off,
    /// are at `detail` of theirs: at 0, only its two bends are drawn, a third of the cost.
    ///
    /// Bent only every `every`th time the wind moves on (its turn among them its own, so plants
    /// take turns: see [`every_at`]), by the time passed since.
    pub fn update(&mut self, wind: &Wind, dt: f32, graph: &mut Graph, shown: f32, detail: f32, every: u32) {
        if shown <= 0.0 {
            if !self.still {
                self.pose(graph, [0.0; 9]);
                self.still = true;
            }
            self.waited = 0.0;
            return;
        }
        self.still = false;
        self.waited += dt;
        if every > 1 && wind.tick.wrapping_add(self.phase) % every != 0 {
            return;
        }
        let dt = std::mem::take(&mut self.waited);
        let push = wind.at(self.at.x, self.at.y) * self.shelter;
        // x'' + 2 zeta omega x' + omega^2 x = omega^2 push, in steps short enough to keep the
        // stiffest grass steady whatever the frame time.
        let steps = (dt * 120.0).ceil().clamp(1.0, 8.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            let pull = (push - self.bend) * (self.omega * self.omega)
                - self.velocity * (2.0 * self.zeta * self.omega);
            self.velocity += pull * h;
            self.bend += self.velocity * h;
        }
        self.flutter.0 = (self.flutter.0 + self.flutter.1 * dt) % TAU;
        for limb in &mut self.limbs {
            limb.0 = (limb.0 + limb.1 * dt) % TAU;
        }

        let local = self.unturn * Vector3::new(self.bend.x, 0.0, self.bend.y);
        let (down, across) = (local.dot(&DOWNWIND), local.dot(&ACROSS));
        let bent = self.bend.norm();
        // Strong enough wind, as the bake made them; in still air, none.
        let flutter = (1.25 * bent).min(1.0);
        let limbs = (2.0 * bent).min(1.6);
        let (fs, fc) = self.flutter.0.sin_cos();
        let (ls, lc) = self.limbs[0].0.sin_cos();
        let (rs, rc) = self.limbs[1].0.sin_cos();
        let weights = [
            down,
            across,
            down * down + across * across,
            flutter * fc,
            flutter * fs,
            limbs * lc,
            limbs * ls,
            limbs * rc,
            limbs * rs,
        ];
        let mut weights = weights.map(|w| w * shown);
        for weight in &mut weights[2..] {
            *weight *= detail;
        }
        self.pose(graph, weights);
    }

    /// Sets its model's targets to `weights`, in the order of [`TARGETS`].
    fn pose(&self, graph: &mut Graph, weights: [f32; 9]) {
        let Some(mesh) = graph.try_get_node_mut(self.mesh).ok().and_then(|n| n.cast_mut::<Mesh>()) else {
            return;
        };
        let shapes = mesh.blend_shapes_mut();
        for (weight, index) in weights.iter().zip(self.order) {
            if let Some(shape) = index.and_then(|i| shapes.get_mut(i as usize)) {
                // The engine's weights are in percent.
                shape.weight = weight * 100.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_keeps_in_bounds() {
        let mut wind = Wind::new(1);
        let mut total = 0.0;
        let mut samples = 0;
        for step in 0..20_000 {
            wind.update(0.05);
            let push = wind.at((step % 97) as f32 * 3.1, (step % 89) as f32 * -2.7).norm();
            assert!(push.is_finite() && push <= 1.6, "push {push}");
            total += push;
            samples += 1;
        }
        let mean = total / samples as f32;
        assert!((0.25..0.9).contains(&mean), "mean push {mean}");
    }

    #[test]
    fn the_wind_changes() {
        let mut wind = Wind::new(4);
        let before = wind.at(0.0, 0.0);
        for _ in 0..600 {
            wind.update(0.05);
        }
        assert_ne!(before, wind.at(0.0, 0.0));
    }
}
