//! The forest's sounds, from plant_life's Birch Glade (`environments/birch-glade/audio`): the
//! player's footsteps on the forest floor; the running water, heard from where it runs; and the
//! wind in the trees, all round, louder as it blows harder.
//!
//! The running water is the glade's loop mixed down to mono, so that the engine can place it:
//! played from a few points on each channel - the nearest to the player, and some way up and
//! down it from there - which keep with the player as they go, so that the water is heard from
//! its own direction, turning with the head, louder where it runs faster. The wind stays in
//! stereo, all round: it is the whole forest's. The listener is the camera.

use crate::{random::Random, terrain::Terrain};
use fyrox::graph::SceneGraph;
use fyrox::{
    asset::manager::ResourceManager,
    core::{
        algebra::{Vector2, Vector3},
        pool::Handle,
    },
    scene::{
        Scene,
        base::BaseBuilder,
        node::Node,
        sound::{Sound, SoundBuffer, SoundBufferResource, SoundBuilder, Status, listener::ListenerBuilder},
    },
};

const FOOTSTEPS: [&str; 6] = [
    "data/audio/footstep-forest-floor-01.ogg",
    "data/audio/footstep-forest-floor-02.ogg",
    "data/audio/footstep-forest-floor-03.ogg",
    "data/audio/footstep-forest-floor-04.ogg",
    "data/audio/footstep-forest-floor-05.ogg",
    "data/audio/footstep-forest-floor-06.ogg",
];
const STREAM: &str = "data/audio/stream-shallow-current-loop-mono.ogg";
/// How long the running water's loop is, in seconds: each source starts somewhere else in it,
/// so that they do not sound as one.
const STREAM_LENGTH: f32 = 20.0;
/// How far up and down a channel from its nearest point its other sources are, in meters.
const WATER_SPREAD: f32 = 10.0;
/// Within how far of a source the water is at its loudest, and how far off it is still heard
/// (inverse distance: half as loud at twice the first).
const WATER_NEAR: f32 = 2.0;
const WATER_FAR: f32 = 60.0;
const WIND: &str = "data/audio/trees-wind-loop.ogg";

/// How far a step carries the player, in meters, walking still and for each meter a second
/// faster: about 0.75 m at a walk, 1.2 m at a run.
const STRIDE: f32 = 0.47;
const STRIDE_PER_SPEED: f32 = 0.1;
/// Slower than this, in meters a second, the player is standing, not walking.
const STILL: f32 = 0.4;
/// How loud a footstep is, walking and running.
const STEP_GAIN: (f32, f32) = (0.45, 0.7);
/// How loud each of the running water's sources is at its loudest, and the wind in the trees in
/// still air and in the strongest wind.
const WATER_GAIN: f32 = 0.45;
const WIND_GAIN: (f32, f32) = (0.12, 0.65);
/// How quickly the loops' loudness follows where the player is and the wind, per second.
const FOLLOW: f32 = 2.0;

#[derive(Debug, Default, PartialEq)]
pub struct Sounds {
    footsteps: Vec<SoundBufferResource>,
    /// The running water's sources: three to a channel.
    water: Vec<Handle<Sound>>,
    wind: Handle<Sound>,
    /// How far the player has gone since the last step, in meters; the last footstep's sound;
    /// whether the player was off the ground.
    walked: f32,
    last: usize,
    airborne: bool,
    random: Random,
}

impl Sounds {
    /// Starts loading the sounds, and the loops playing, silent until the first update, for
    /// `terrain`'s water; the listener on `camera`.
    pub fn build(scene: &mut Scene, resources: &ResourceManager, terrain: &Terrain, camera: Handle<Node>, seed: u64) -> Self {
        let listener = ListenerBuilder::new(BaseBuilder::new().with_name("Listener")).build(&mut scene.graph);
        scene.graph.link_nodes(listener, camera);
        let footsteps = FOOTSTEPS.iter().map(|path| resources.request::<SoundBuffer>(*path)).collect();
        let stream = resources.request::<SoundBuffer>(STREAM);
        let water = (0..terrain.channels.len() * 3)
            .map(|k| {
                SoundBuilder::new(BaseBuilder::new().with_name("Running water"))
                    .with_buffer(Some(stream.clone()))
                    .with_looping(true)
                    .with_gain(0.0)
                    .with_spatial_blend_factor(1.0)
                    .with_radius(WATER_NEAR)
                    .with_rolloff_factor(1.0)
                    .with_max_distance(WATER_FAR)
                    // Each somewhere else in the loop (a step that is no whole fraction of it).
                    .with_playback_time(std::time::Duration::from_secs_f32((k as f32 * 7.31) % STREAM_LENGTH))
                    .with_status(Status::Playing)
                    .build(&mut scene.graph)
            })
            .collect();
        let wind = SoundBuilder::new(BaseBuilder::new().with_name("Wind in the trees"))
            .with_buffer(Some(resources.request::<SoundBuffer>(WIND)))
            .with_looping(true)
            .with_gain(0.0)
            .with_spatial_blend_factor(0.0)
            .with_status(Status::Playing)
            .build(&mut scene.graph);
        Self { footsteps, water, wind, walked: 0.0, last: usize::MAX, airborne: false, random: Random(seed ^ 0x50_0d5) }
    }

    /// Moves the sounds on by `dt` seconds: the player's feet at `feet`, moving at `velocity`,
    /// on the ground or not; the wind blowing with `wind` strength (0 to 1).
    pub fn update(&mut self, scene: &mut Scene, terrain: &Terrain, feet: Vector3<f32>, velocity: Vector3<f32>, grounded: bool, wind: f32, dt: f32) {
        let follow = 1.0 - (-FOLLOW * dt).exp();
        // Each loop's loudness eased toward where it should be; the water's sources moved on.
        let ease = |sound: &mut Sound, target: f32| {
            let gain = sound.gain();
            sound.set_gain(gain + (target - gain) * follow);
        };
        for (&handle, (at, loudness)) in self.water.iter().zip(water_sources(terrain, feet.xz())) {
            if let Ok(sound) = scene.graph.try_get_mut(handle) {
                ease(sound, WATER_GAIN * loudness);
                sound.local_transform_mut().set_position(at);
            }
        }
        if let Ok(sound) = scene.graph.try_get_mut(self.wind) {
            ease(sound, WIND_GAIN.0 + (WIND_GAIN.1 - WIND_GAIN.0) * wind.clamp(0.0, 1.0));
        }

        // A step each stride along the ground, and one on landing; none in the water, where
        // the forest floor's would be wrong.
        let speed = velocity.xz().norm();
        let landed = grounded && self.airborne;
        self.airborne = !grounded;
        if !grounded || terrain.wet(feet.x, feet.z, 0.0) {
            return;
        }
        if speed < STILL && !landed {
            // Starting off, the first step comes soon.
            self.walked = self.walked.max(0.6 * stride(WALKING));
            return;
        }
        self.walked += speed * dt;
        if self.walked >= stride(speed) || landed {
            self.walked = 0.0;
            let running = ((speed - WALKING) / (RUNNING - WALKING)).clamp(0.0, 1.0);
            self.step(scene, STEP_GAIN.0 + (STEP_GAIN.1 - STEP_GAIN.0) * running);
        }
    }

    /// One footstep, at `gain`: one of the six, never the last one again, a little higher or
    /// lower each time.
    fn step(&mut self, scene: &mut Scene, gain: f32) {
        let loaded: Vec<usize> = (0..self.footsteps.len()).filter(|&i| self.footsteps[i].is_ok() && i != self.last).collect();
        if loaded.is_empty() {
            return;
        }
        let pick = loaded[(self.random.unit() * loaded.len() as f32) as usize % loaded.len()];
        self.last = pick;
        SoundBuilder::new(BaseBuilder::new().with_name("Footstep"))
            .with_buffer(Some(self.footsteps[pick].clone()))
            .with_play_once(true)
            .with_gain(gain * self.random.between(0.85, 1.0))
            .with_pitch(self.random.between(0.94, 1.06) as f64)
            .with_spatial_blend_factor(0.0)
            .with_status(Status::Playing)
            .build(&mut scene.graph);
    }
}

/// The player's walking and running speeds, in meters a second.
const WALKING: f32 = 3.5;
const RUNNING: f32 = 7.0;

fn stride(speed: f32) -> f32 {
    STRIDE + STRIDE_PER_SPEED * speed.max(WALKING)
}

/// Where the running water is heard from, as the player stands at `at`, and how loud each
/// source is there (0 to 1): three to a channel, in order - at its nearest water, and
/// [`WATER_SPREAD`] meters up and down it from there (the ends kept) - each a little over the
/// water's surface; louder the faster the water runs there, the river, being wider, louder than a
/// stream.
pub fn water_sources(terrain: &Terrain, at: Vector2<f32>) -> Vec<(Vector3<f32>, f32)> {
    let mut sources = Vec::with_capacity(terrain.channels.len() * 3);
    for (c, channel) in terrain.channels.iter().enumerate() {
        let (mut nearest, mut index) = (f32::MAX, 0);
        for (i, point) in channel.points.iter().enumerate() {
            let [right, left] = channel.waterlines[i];
            let off = (point - at).norm() - 0.5 * (right + left);
            if off < nearest {
                (nearest, index) = (off, i);
            }
        }
        let width = if c == 0 { 1.0 } else { 0.75 };
        for spread in [0.0, -WATER_SPREAD, WATER_SPREAD] {
            let along = channel.distances[index] + spread;
            let i = channel.distances.partition_point(|&d| d < along).min(channel.points.len() - 1);
            let point = channel.points[i];
            let running = 0.35 + 0.65 * (channel.speed(i) / 2.0).clamp(0.0, 1.0);
            sources.push((Vector3::new(point.x, channel.water(i) + 0.2, point.y), width * running));
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_water_is_heard_from_where_it_runs() {
        let terrain = Terrain::new(crate::game::SEED);
        let river = &terrain.channels[0];
        let middle = river.points.len() / 2;
        let side = crate::terrain::side_of(&river.points, middle);
        let at = river.points[middle] + side * 6.0;
        let sources = water_sources(&terrain, at);
        assert_eq!(sources.len(), terrain.channels.len() * 3);
        // The river's first source is on the river, at its water, beside the player; the next
        // two up and down it.
        let (nearest, loudness) = sources[0];
        assert!((Vector2::new(nearest.x, nearest.z) - river.points[middle]).norm() < 1.5, "{nearest:?}");
        assert!((nearest.y - river.water(middle) - 0.2).abs() < 0.05);
        assert!((0.3..=1.0).contains(&loudness));
        for (up_or_down, _) in &sources[1..3] {
            let away = (Vector2::new(up_or_down.x, up_or_down.z) - Vector2::new(nearest.x, nearest.z)).norm();
            assert!((7.0..=11.0).contains(&away), "{away}");
        }
    }

    #[test]
    fn a_run_takes_longer_strides_than_a_walk() {
        assert!((stride(WALKING) - 0.82).abs() < 0.05);
        assert!(stride(RUNNING) > stride(WALKING) + 0.3);
    }
}
