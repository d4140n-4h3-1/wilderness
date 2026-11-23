//! Wilderness: a first-person walk through a wild forest, on the Hydroxus engine.
//!
//! WILDERNESS_SEED=<n> grows a different forest; WILDERNESS_FULLSCREEN=1 fills the screen;
//! WILDERNESS_STATS=1 also logs where the GPU's time goes, where it can time itself, and
//! WILDERNESS_VSYNC=0 draws without waiting for the screen, for steadier GPU times;
//! WILDERNESS_BENCH=1 is for measuring: stats on, no vsync, and the keyboard and mouse ignored,
//! so the view stays where WILDERNESS_VIEW puts it; WILDERNESS_BENCH=walk the same, walking straight
//! on, to measure what moving costs (the stats give each second's longest frame);
//! WILDERNESS_RT=0 draws shadows from shadow maps rather than tracing them, and
//! WILDERNESS_HARD_SHADOWS=1 traces them with sharp edges; WILDERNESS_STATS=1 logs frame times;
//! WILDERNESS_VIEW=<yaw>,<pitch> starts the view turned so, in degrees; WILDERNESS_YOCTITE_VIEW=1
//! shows Yoctite's clusters (=cpu chooses them on the CPU); WILDERNESS_YOCTITE=1 draws the forest
//! with Yoctite; WILDERNESS_CANDELITE_VIEW=1 shows what Candelite's rays see, =lit with the light on it; WILDERNESS_CANDELITE=1 lights the frame with it (see below);
//! WILDERNESS_WIND=0 stills the wind; WILDERNESS_FLOOR=0 leaves the forest floor
//! bare.

mod floor;
mod forest;
mod game;
mod hud;
mod models;
mod player;
mod random;
mod wind;

use fyrox::{
    engine::{GraphicsContextParams, executor::Executor},
    event_loop::EventLoop,
    window::{Fullscreen, WindowAttributes},
};

fn main() {
    // Assets are looked up next to this crate, wherever it is started from.
    let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));

    let mut window_attributes = WindowAttributes::default()
        .with_title("Wilderness")
        .with_resizable(true);
    if std::env::var("WILDERNESS_FULLSCREEN").as_deref() == Ok("1") {
        window_attributes = window_attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    let mut executor = Executor::from_params(
        Some(EventLoop::new().unwrap()),
        GraphicsContextParams {
            window_attributes,
            // WILDERNESS_VSYNC=0 draws as fast as it can, for measuring: waiting for the screen,
            // the GPU idles between frames, slows its clock, and times every frame slower.
            vsync: std::env::var("WILDERNESS_VSYNC").as_deref() != Ok("0") && !benchmarking(),
            msaa_sample_count: None,
            // With OpenGL compiled out, the default constructor is the wgpu one.
            graphics_server_constructor: Default::default(),
            named_objects: false,
        },
    );
    executor.add_plugin(graphics_effects());
    executor.add_plugin(game::Wilderness::default());
    executor.run()
}

/// Whether WILDERNESS_BENCH=1 (or =walk) asks for a run to measure by: see the crate's
/// description.
pub fn benchmarking() -> bool {
    matches!(std::env::var("WILDERNESS_BENCH").as_deref(), Ok("1") | Ok("walk"))
}

/// Whether WILDERNESS_BENCH=walk asks the player to walk straight on by themselves, for measuring
/// what moving costs: the floor streamed in, shadows bent anew, Candelite's grids moved.
pub fn walking() -> bool {
    std::env::var("WILDERNESS_BENCH").as_deref() == Ok("walk")
}

/// Whether to log what the renderer did: WILDERNESS_STATS=1, or benchmarking.
pub fn stats() -> bool {
    std::env::var("WILDERNESS_STATS").as_deref() == Ok("1") || benchmarking()
}

/// Whether the sun's shadows are traced (unless WILDERNESS_RT=0), rather than drawn from shadow
/// maps. A graphics card that cannot trace falls back to maps anyway.
pub fn traced_shadows() -> bool {
    std::env::var("WILDERNESS_RT").as_deref() != Ok("0")
}

/// The sun's shadows traced against the trees, with soft edges - their leaves and needles
/// letting light through their gaps - in place of shadow maps; and anti-aliasing, which the
/// fine edges of needles need. A graphics card without ray tracing falls back to shadow maps, as
/// does WILDERNESS_RT=0.
fn graphics_effects() -> fyrox_gfx::GraphicsEffects {
    let mut effects = fyrox_gfx::GraphicsEffects::default().with_anti_aliasing(fyrox_gfx::AntiAliasing {
        fxaa: true,
        temporal: true,
    });
    // WILDERNESS_YOCTITE_VIEW=1 draws Yoctite's cluster view over the frame: every mesh as its
    // clusters, each in a colour of its own, at the level of detail it would be drawn at, chosen
    // on the GPU; =cpu chooses them on the CPU instead, as the GPU's choice is checked against.
    match std::env::var("WILDERNESS_YOCTITE_VIEW").as_deref() {
        Ok("1") => effects = effects.with_yoctite_view(),
        Ok("cpu") => effects = effects.with_yoctite_view_on_cpu(),
        // WILDERNESS_CANDELITE_VIEW=1 draws with Yoctite, and lays over the frame what a ray
        // from the eye through each pixel finds on Candelite's cards: the surface cache the global
        // illumination is to gather from. Magenta where no card saw what a ray hit.
        _ if std::env::var("WILDERNESS_CANDELITE_VIEW").as_deref() == Ok("1") => {
            effects = effects.with_candelite_view()
        }
        // WILDERNESS_CANDELITE_VIEW=lit lights the cards too - the sun with traced shadows, and
        // light bounced between surfaces - and shows the light leaving what each ray hit.
        _ if std::env::var("WILDERNESS_CANDELITE_VIEW").as_deref() == Ok("lit") => {
            effects = effects.with_candelite_light_view()
        }
        // WILDERNESS_CANDELITE=1 draws with Yoctite and lights the forest with Candelite's global
        // illumination: skylight and sunlight bounced between surfaces, in place of the flat
        // ambient colour.
        _ if std::env::var("WILDERNESS_CANDELITE").as_deref() == Ok("1") => effects = effects.with_candelite(),
        // WILDERNESS_YOCTITE=1 draws the forest with Yoctite in place of the renderer: every
        // tree's clusters at the level of detail they need, chosen and drawn by the GPU.
        _ if std::env::var("WILDERNESS_YOCTITE").as_deref() == Ok("1") => effects = effects.with_yoctite(),
        _ => {}
    }
    if traced_shadows() {
        // WILDERNESS_HARD_SHADOWS=1 traces them with sharp edges, from a sun with no size.
        let shadows = if std::env::var("WILDERNESS_HARD_SHADOWS").as_deref() == Ok("1") {
            fyrox_gfx::RayTracedShadows::hard()
        } else {
            fyrox_gfx::RayTracedShadows {
                // Near the real sun's quarter degree: a bigger sun blurs the shadows of needles
                // and leaves, a few meters below them, into smudges.
                sun_angular_radius: 0.35,
                ..Default::default()
            }
        };
        effects = effects.with_ray_traced_shadows(fyrox_gfx::RayTracedShadows {
            // Past this, the haze has all but hidden a shadow.
            reach: 150.0,
            ..shadows
        });
    }
    effects
}
