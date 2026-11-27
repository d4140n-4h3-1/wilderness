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
//! shows Yoctite's clusters (=cpu chooses them on the CPU); the forest is drawn with Yoctite and
//! lit with Candelite unless WILDERNESS_CANDELITE=0 (Yoctite alone) or WILDERNESS_YOCTITE=0 (neither:
//! the renderer draws it) - or a graphics card that cannot run them, which turns them off by itself
//! and says why in the log; WILDERNESS_CANDELITE_VIEW=1 shows what Candelite's rays see, =lit with the
//! light on it;
//! WILDERNESS_GRASS=0 grows no grass; WILDERNESS_WIND=0 stills the wind; WILDERNESS_FLOOR=0 leaves the forest floor
//! bare, and WILDERNESS_COVER=0 only the ground cover out of it.

mod bed;
mod day;
mod flashlight;
mod floor;
mod forest;
mod game;
mod grass;
mod hud;
mod lantern;
mod litter;
mod models;
mod player;
mod random;
mod seasons;
mod sky;
mod snow;
mod sound;
mod terrain;
mod water;
mod weather;
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
    let effects = graphics_effects();
    // The field the grass grows on, for the game to say where once the land is made.
    let grass = effects.grass_field();
    executor.add_plugin(effects);
    executor.add_plugin(game::Wilderness::default().with_grass(grass));
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

/// Whether grass grows over the land (unless WILDERNESS_GRASS=0): the engine's, blades drawn round
/// the player in one draw (see [`grass`]).
pub fn grass_grows() -> bool {
    std::env::var("WILDERNESS_GRASS").as_deref() != Ok("0")
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
        // By default the forest is drawn with Yoctite - every tree's clusters at the level of
        // detail they need, chosen and drawn by the GPU, in place of the renderer - and lit with
        // Candelite's global illumination: skylight and sunlight bounced between surfaces, in place
        // of the flat ambient colour. Each only where the graphics card can run it (fyrox-gfx's
        // `support`, checked at start): without Yoctite the renderer draws the forest, and without
        // Candelite - or if it fails along the way - the ambient colour lights it.
        // WILDERNESS_CANDELITE=0 leaves Candelite out; WILDERNESS_YOCTITE=0 leaves both out, the
        // renderer drawing the forest itself.
        _ if std::env::var("WILDERNESS_YOCTITE").as_deref() == Ok("0") => {}
        _ if std::env::var("WILDERNESS_CANDELITE").as_deref() == Ok("0") => effects = effects.with_yoctite(),
        _ => effects = effects.with_candelite(),
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
    if grass_grows() {
        effects = effects.with_grass(fyrox_gfx::GrassSettings::default());
    }
    effects
}
