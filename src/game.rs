//! The game: a forest in afternoon sun, with a breeze through it, and a player walking through
//! it.

use crate::{
    day::Day,
    floor::{self, Floor},
    flashlight::Flashlight,
    lantern::Lantern,
    hud::WindGauge,
    forest::{self, SPECIES},
    models::{self, Prepared},
    player::Player,
    terrain::Terrain,
    water::Water,
    wind::{Sway, Wind},
};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector2, Vector3},
        color::Color,
        log::Log,
        pool::Handle,
        reflect::prelude::*,
        visitor::prelude::*,
    },
    engine::GraphicsContext,
    event::{ElementState, Event, KeyEvent, MouseButton, WindowEvent},
    gui::{
        HorizontalAlignment, Thickness, UserInterface, VerticalAlignment,
        brush::Brush,
        text::{Text, TextBuilder, TextMessage},
        widget::WidgetBuilder,
    },
    keyboard::{KeyCode, PhysicalKey},
    material::{Material, MaterialResource},
    plugin::{Plugin, PluginContext, error::GameResult},
    resource::model::{Model, ModelResource, ModelResourceExtension},
    scene::{
        EnvironmentLightingSource, Fog, Scene,
        base::BaseBuilder,
        collider::{ColliderBuilder, ColliderShape},
        graph::Graph,
        node::Node,
        light::{
            BaseLightBuilder,
            directional::{CsmOptions, DirectionalLight, DirectionalLightBuilder, FrustumSplitOptions},
        },
        mesh::Mesh,
        rigidbody::{RigidBodyBuilder, RigidBodyType},
        skybox::SkyBoxKind,
        transform::TransformBuilder,
    },
    window::CursorGrabMode,
};
use fyrox::{graph::SceneGraph, material::MaterialResourceExtension};

/// Which forest grows, unless WILDERNESS_SEED says otherwise.
const SEED: u64 = 1;
/// Light from the sky on the side of things the sun is not on.
const AMBIENT: Color = Color::opaque(128, 136, 146);
/// The sun: warm, in the afternoon.
const SUN: Color = Color::opaque(255, 244, 222);
/// The sun's height over the horizon, and its bearing, in degrees.
const SUN_ELEVATION: f32 = 38.0;
const SUN_BEARING: f32 = 35.0;
/// Where each of the sun's three shadow maps ends, in meters from the eye, where shadows are not
/// traced (see `main.rs`). The engine's own (5, 25, 64 m) spread the last map so thin that its
/// edges step; further than 80 m costs more draws than the haze lets a shadow show.
const SHADOW_CASCADES: [f32; 3] = [8.0, 30.0, 80.0];
/// A light blue haze, which hides the edge of the forest.
const HAZE: Fog = Fog {
    color: Color::opaque(178, 192, 204),
    density: 0.012,
    start: 20.0,
    base_height: 0.0,
    height_falloff: 0.02,
    sky_opacity: 0.0,
};
/// Bare earth, should the forest loam not load.
const GROUND: Color = Color::opaque(104, 88, 64);
/// The forest floor's soil: a tile a meter across, repeated.
const LOAM: &str = "data/ground/forest-loam.glb";
/// How far past the trees the ground goes, and where the invisible walls that keep the player in
/// stand, in meters.
pub const BORDER: f32 = 12.0;
/// Within the first distance from the player, in meters, trees sway fully; past the second, not
/// at all, between, less and less.
const TREES_SWAY: (f32, f32) = (60.0, 85.0);
/// What the help line says once the forest has grown.
const HELP: &str = "WASD walk  ·  Shift run  ·  Space jump  ·  F flashlight  ·  L lantern  ·  Click to look around  ·  Esc free the mouse";

#[derive(Default, Debug, PartialEq, Visit, Reflect)]
#[reflect(non_cloneable, type_uuid = "6f1c2b7e-4a5d-4e8b-9c3a-1d2e7f80a915")]
pub struct Wilderness {
    #[visit(skip)]
    #[reflect(hidden)]
    scene: Handle<Scene>,
    /// The models of the [`SPECIES`], in that order, then of the floor's [`floor::kinds`], then
    /// the [`LOAM`], loading until the forest is planted.
    #[visit(skip)]
    #[reflect(hidden)]
    models: Vec<ModelResource>,
    planted: bool,
    /// The land, and its ground's squares, bare earth until the loam takes its place.
    #[visit(skip)]
    #[reflect(hidden)]
    terrain: Option<Terrain>,
    #[visit(skip)]
    #[reflect(hidden)]
    ground: Vec<Handle<Node>>,
    /// The water in the river and the streams.
    #[visit(skip)]
    #[reflect(hidden)]
    water: Water,
    #[visit(skip)]
    #[reflect(hidden)]
    floor: Option<Floor>,
    #[visit(skip)]
    #[reflect(hidden)]
    prepared: Prepared,
    #[visit(skip)]
    #[reflect(hidden)]
    wind: Option<Wind>,
    #[visit(skip)]
    #[reflect(hidden)]
    gauge: WindGauge,
    /// Every tree, swaying.
    #[visit(skip)]
    #[reflect(hidden)]
    trees: Vec<Sway>,
    #[visit(skip)]
    #[reflect(hidden)]
    player: Player,
    /// Whether the mouse is held by the window, turning the view.
    mouse_captured: bool,
    focused: bool,
    /// The line of text at the top of the screen.
    #[visit(skip)]
    #[reflect(hidden)]
    status: Handle<Text>,
    /// The time of day going by (see `day.rs`); none when WILDERNESS_DAY=0, the afternoon staying.
    #[visit(skip)]
    #[reflect(hidden)]
    day: Option<Day>,
    #[visit(skip)]
    #[reflect(hidden)]
    sun: Handle<DirectionalLight>,
    /// What the player can light the way with: one at a time.
    #[visit(skip)]
    #[reflect(hidden)]
    flashlight: Flashlight,
    #[visit(skip)]
    #[reflect(hidden)]
    lantern: Option<Lantern>,
    /// What the status line says, the clock included.
    #[visit(skip)]
    #[reflect(hidden)]
    shown_status: String,
}

impl Wilderness {
    fn build_scene(&mut self, ctx: &mut PluginContext) {
        let mut scene = Scene::new();
        scene.rendering_options.ambient_lighting_color = AMBIENT;
        scene.rendering_options.environment_lighting_source = EnvironmentLightingSource::AmbientColor;
        scene.rendering_options.fog = Some(HAZE);
        scene.set_skybox(Some(SkyBoxKind::built_in_skybox().clone()));

        // A directional light shines down its own -y; tipped over by the sun's angle from
        // straight up, then turned to its bearing.
        self.sun = DirectionalLightBuilder::new(
            BaseLightBuilder::new(
                BaseBuilder::new().with_name("Sun").with_local_transform(
                    TransformBuilder::new()
                        .with_local_rotation(
                            UnitQuaternion::from_axis_angle(&Vector3::y_axis(), SUN_BEARING.to_radians())
                                * UnitQuaternion::from_axis_angle(
                                    &Vector3::x_axis(),
                                    (90.0 - SUN_ELEVATION).to_radians(),
                                ),
                        )
                        .build(),
                ),
            )
            .with_color(SUN),
        )
        .with_csm_options({
            let mut options = CsmOptions::default();
            options.split_options = FrustumSplitOptions::Absolute {
                far_planes: SHADOW_CASCADES,
            };
            options
        })
        .build(&mut scene.graph);

        self.day = Day::from_env();
        if self.day.is_some() {
            // The built-in sky is a picture of day; a plain colour, changing, stands for the sky.
            scene.set_skybox(None);
        }

        // The land, its ground bare earth until the loam has loaded, and its water.
        let started = std::time::Instant::now();
        let terrain = Terrain::new(seed());
        let mut bare = Material::standard();
        bare.set_property("diffuseColor", GROUND);
        self.ground = terrain.build_ground(&mut scene.graph, &MaterialResource::new_embedded(bare));
        build_walls(&mut scene.graph);
        self.water = Water::build(&mut scene, &terrain);
        Log::info(format!(
            "Shaped the land: a river (by the start at x {:.1}) and {} streams, in {:.2} s",
            terrain.river_x(0.0),
            terrain.channels.len() - 1,
            started.elapsed().as_secs_f32()
        ));
        // WILDERNESS_SPAWN=<x>,<z> starts the player there, in meters, rather than in the
        // clearing in the middle: a glade in the sun, say, to see shadows.
        let (x, z) = std::env::var("WILDERNESS_SPAWN")
            .ok()
            .and_then(|at| {
                let (x, z) = at.split_once(',')?;
                Some((x.trim().parse().ok()?, z.trim().parse().ok()?))
            })
            .unwrap_or((0.0, 0.0));
        let spawn = Vector3::new(x, terrain.height(x, z), z);
        self.terrain = Some(terrain);
        self.player = Player::spawn(&mut scene.graph, spawn);
        self.flashlight = Flashlight::build(&mut scene.graph, self.player.camera());
        // WILDERNESS_VIEW=<yaw>,<pitch> starts the view turned so, in degrees (pitch down), for
        // screenshots taken the same way each time.
        if let Some((yaw, pitch)) = std::env::var("WILDERNESS_VIEW").ok().and_then(|view| {
            let (yaw, pitch) = view.split_once(',')?;
            Some((yaw.trim().parse::<f32>().ok()?, pitch.trim().parse::<f32>().ok()?))
        }) {
            self.player.set_view(yaw.to_radians(), pitch.to_radians());
        }
        self.scene = ctx.scenes.add(scene);

        self.models = model_paths()
            .map(|path| ctx.resource_manager.request::<Model>(path))
            .collect();
        self.lantern = Some(Lantern::load(&ctx.resource_manager));
    }

    fn build_status(&mut self, ctx: &mut PluginContext) {
        // The engine starts with no user interface; this one becomes the first, and the engine
        // keeps it the size of the window.
        ctx.user_interfaces.add(UserInterface::new(Vector2::new(1280.0, 720.0)));
        let ui = ctx.user_interfaces.first_mut();
        self.status = TextBuilder::new(
            WidgetBuilder::new()
                .with_margin(Thickness::uniform(12.0))
                .with_horizontal_alignment(HorizontalAlignment::Center)
                .with_vertical_alignment(VerticalAlignment::Top)
                .with_foreground(Brush::Solid(Color::WHITE).into()),
        )
        .with_font_size(18.0.into())
        .with_text("The forest is growing...")
        .build(&mut ui.build_ctx());
        self.gauge = WindGauge::build(ui);
    }

    /// Switches the flashlight on or off (`flashlight`), or takes out or puts away the lantern:
    /// the player holds one or the other, so taking out one puts the other away.
    fn light_the_way(&mut self, graph: &mut Graph, flashlight: bool) {
        let lantern_held = self.lantern.as_ref().is_some_and(Lantern::held);
        let (flashlight_on, lantern_on) = if flashlight {
            (!self.flashlight.on(graph), false)
        } else {
            (false, !lantern_held)
        };
        self.flashlight.set_on(graph, flashlight_on);
        if let Some(lantern) = &mut self.lantern {
            if lantern.held() != lantern_on {
                lantern.set_held(graph, lantern_on);
            }
        }
    }

    /// Moves the sun on, and lights the scene for the time of day, while days go by.
    fn pass_time(&mut self, ctx: &mut PluginContext) {
        let Some(day) = &mut self.day else {
            return;
        };
        day.advance(ctx.dt);
        let lit = day.lighting();
        let scene = &mut ctx.scenes[self.scene];
        scene.rendering_options.ambient_lighting_color = lit.ambient;
        scene.rendering_options.clear_color = Some(lit.sky);
        if let Some(fog) = &mut scene.rendering_options.fog {
            fog.color = lit.haze;
        }
        let sun = &mut scene.graph[self.sun];
        sun.local_transform_mut().set_rotation(lit.rotation);
        let light = sun.base_light_mut();
        light.set_color(lit.color);
        light.set_intensity(lit.strength);
        if self.planted {
            let status = format!("{HELP}\n{}  ·  [ ] step an hour  ·  P pause", self.day.as_ref().unwrap().clock());
            if status != self.shown_status {
                self.set_status(ctx, &status);
                self.shown_status = status;
            }
        }
    }

    fn set_status(&self, ctx: &mut PluginContext, text: &str) {
        ctx.user_interfaces
            .first_mut()
            .send(self.status, TextMessage::Text(text.to_owned()));
    }

    /// Plants the forest once every model has loaded or failed to. Whether it has been planted.
    fn plant_when_loaded(&mut self, ctx: &mut PluginContext) -> bool {
        if self.planted {
            return true;
        }
        if self.models.iter().any(|model| model.is_loading()) {
            return false;
        }
        let seed = seed();
        let Some(terrain) = &self.terrain else {
            return false;
        };
        for (path, model) in model_paths().zip(&self.models) {
            if !model.is_ok() {
                Log::err(format!("Could not load {path}; it is left out"));
            }
        }
        let started = std::time::Instant::now();
        let wind = wind_blows();
        let double_sided: Vec<Vec<bool>> = model_paths().map(models::double_sided).collect();
        let mut trees = forest::lay_out(seed, terrain);
        trees.retain(|tree| self.models[tree.species].is_ok());
        let scene = &mut ctx.scenes[self.scene];
        let (_, sways) = forest::plant(
            scene,
            &self.models[..SPECIES.len()],
            &double_sided[..SPECIES.len()],
            &trees,
            &mut self.prepared,
            wind,
            seed,
        );
        self.trees = sways;

        // The floor, where its models loaded; one that did not is left bare.
        let floor_models = &self.models[SPECIES.len()..SPECIES.len() + floor::kinds().count()];
        let mut floor = Floor::lay_out(
            seed,
            terrain,
            &trees,
            floor_models.to_vec(),
            double_sided[SPECIES.len()..SPECIES.len() + floor::kinds().count()].to_vec(),
            wind,
        );
        floor.retain_loaded();
        // WILDERNESS_FLOOR=0 leaves the forest floor bare, to tell what it costs.
        if std::env::var("WILDERNESS_FLOOR").as_deref() == Ok("0") {
            floor.retain(|_| false);
        }
        floor.build_rocks(&mut scene.graph);
        let wind_now = Wind::new(seed);
        let player = self.player.feet(&scene.graph);
        floor.update(scene, &mut self.prepared, player, &wind_now, 0.0, true);
        if let Some(loam) = self.models.last().filter(|m| m.is_ok()) {
            lay_loam(scene, loam, &self.ground);
        }
        Log::info(format!(
            "Planted {} trees and {} plants and rocks from seed {seed} in {:.1} s; wind {}",
            trees.len(),
            floor.len(),
            started.elapsed().as_secs_f32(),
            if wind { "on" } else { "off" },
        ));
        self.floor = Some(floor);
        self.wind = Some(wind_now);
        self.planted = true;
        self.set_status(ctx, HELP);
        true
    }

    fn set_mouse_captured(&mut self, ctx: &mut PluginContext, captured: bool) {
        let GraphicsContext::Initialized(graphics_context) = &*ctx.graphics_context else {
            return;
        };
        let window = &graphics_context.window;
        if captured {
            // Not every platform can lock the cursor in place; confining it will do.
            if window.set_cursor_grab(CursorGrabMode::Locked).is_err()
                && window.set_cursor_grab(CursorGrabMode::Confined).is_err()
            {
                return;
            }
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
        }
        window.set_cursor_visible(!captured);
        self.mouse_captured = captured;
    }
}

/// Every model the game loads: the trees, the floor's plants and rocks, and the loam.
fn model_paths() -> impl Iterator<Item = &'static str> {
    SPECIES
        .iter()
        .map(|s| s.path)
        .chain(floor::kinds().map(|k| k.path))
        .chain(std::iter::once(LOAM))
}

/// Whether the plants sway: unless WILDERNESS_WIND=0.
fn wind_blows() -> bool {
    std::env::var("WILDERNESS_WIND").as_deref() != Ok("0")
}

/// Lays the loam over the `ground`'s squares: the material of its one-meter tile, whose texture
/// the squares repeat a meter apart.
fn lay_loam(scene: &mut Scene, loam: &ModelResource, ground: &[Handle<Node>]) {
    let root = loam.instantiate(scene);
    let graph = &mut scene.graph;
    let material = graph
        .traverse_handle_iter(root)
        .find_map(|handle| graph[handle].cast::<Mesh>()?.surfaces().first().map(|s| s.material().deep_copy()));
    graph.remove_node(root);
    let Some(material) = material else {
        Log::warn(format!("{LOAM} has no surface; the ground stays bare"));
        return;
    };
    material.data_ref().set_property("texCoordScale", Vector2::new(1.0, 1.0));
    for &square in ground {
        if let Some(mesh) = graph[square].cast_mut::<Mesh>() {
            for surface in mesh.surfaces_mut() {
                surface.set_material(material.clone());
            }
        }
    }
}

/// Which forest grows: WILDERNESS_SEED, or [`SEED`].
fn seed() -> u64 {
    std::env::var("WILDERNESS_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or(SEED)
}

/// Invisible walls round the ground's edge, which keep the player in.
fn build_walls(graph: &mut Graph) {
    let half = crate::terrain::half_size();
    let wall_height = 20.0;
    let mut body = BaseBuilder::new().with_name("Walls");
    let pieces = [
        (Vector3::new(0.0, 0.0, half), Vector3::new(half, wall_height, 0.5)),
        (Vector3::new(0.0, 0.0, -half), Vector3::new(half, wall_height, 0.5)),
        (Vector3::new(half, 0.0, 0.0), Vector3::new(0.5, wall_height, half)),
        (Vector3::new(-half, 0.0, 0.0), Vector3::new(0.5, wall_height, half)),
    ];
    for (position, extents) in pieces {
        let collider = ColliderBuilder::new(
            BaseBuilder::new()
                .with_local_transform(TransformBuilder::new().with_local_position(position).build()),
        )
        .with_shape(ColliderShape::cuboid(extents.x, extents.y, extents.z))
        .build(graph);
        body = body.with_child(collider);
    }
    RigidBodyBuilder::new(body)
        .with_body_type(RigidBodyType::Static)
        .build(graph);
}

/// With WILDERNESS_STATS=1 or WILDERNESS_BENCH=1, logs what the renderer did, once a second.
fn log_stats(ctx: &PluginContext) {
    if !crate::stats() {
        return;
    }
    // The longest wait between two updates this second: a pause shows here, where the average
    // hides it.
    thread_local! {
        static LONGEST: std::cell::Cell<(Option<std::time::Instant>, f32)> = const { std::cell::Cell::new((None, 0.0)) };
    }
    let now = std::time::Instant::now();
    let longest = LONGEST.with(|cell| {
        let (last, longest) = cell.get();
        let longest = last.map_or(longest, |last| longest.max((now - last).as_secs_f32()));
        cell.set((Some(now), longest));
        longest
    });
    if (ctx.elapsed_time - ctx.dt).floor() == ctx.elapsed_time.floor() {
        return;
    }
    LONGEST.with(|cell| cell.set((Some(now), 0.0)));
    if let GraphicsContext::Initialized(graphics_context) = &*ctx.graphics_context {
        let stats = graphics_context.renderer.get_statistics();
        Log::info(format!(
            "Stats: {} FPS, frame {:.2} ms, longest {:.1} ms, {} draw calls, {} triangles",
            stats.frames_per_second,
            stats.pure_frame_time * 1000.0,
            longest * 1000.0,
            stats.geometry.draw_calls,
            stats.geometry.triangles_rendered,
        ));
        // Where the GPU's time went, section by section, averaged over the second.
        let timings = graphics_context.renderer.gpu_timings();
        if !timings.is_empty() {
            let total: f32 = timings.iter().map(|(_, ms)| ms).sum();
            let sections: Vec<String> = timings.iter().map(|(label, ms)| format!("{label} {ms:.2}")).collect();
            Log::info(format!("GPU: {total:.2} ms - {}", sections.join(", ")));
        }
    }
}

impl Plugin for Wilderness {
    fn init(&mut self, _scene_path: Option<&str>, mut ctx: PluginContext) -> GameResult {
        self.focused = true;
        self.build_scene(&mut ctx);
        self.build_status(&mut ctx);
        Ok(())
    }

    fn update(&mut self, ctx: &mut PluginContext) -> GameResult {
        self.pass_time(ctx);
        if !self.plant_when_loaded(ctx) {
            return Ok(());
        }
        let scene = &mut ctx.scenes[self.scene];
        self.player.update(&mut scene.graph, ctx.dt);
        self.water.update(scene, self.sun, ctx.dt);
        self.flashlight.update(&mut scene.graph, self.player.yaw_pitch(), ctx.dt);
        if let Some(lantern) = &mut self.lantern {
            lantern.update(scene, self.player.camera(), self.player.yaw_pitch(), ctx.dt);
        }
        let started = std::time::Instant::now();
        if let Some(wind) = &mut self.wind {
            wind.update(ctx.dt);
            // Far off, a tree's sway is too small to see: faded out, and then left still, which
            // costs nothing to draw.
            let player = self.player.feet(&scene.graph).xz();
            for tree in &mut self.trees {
                let distance = (tree.at() - player).norm();
                let shown = 1.0 - crate::random::smoothstep(TREES_SWAY.0, TREES_SWAY.1, distance);
                tree.update(wind, ctx.dt, &mut scene.graph, shown, crate::wind::detail_at(distance), crate::wind::every_at(distance));
            }
            if let Some(floor) = &mut self.floor {
                let player = self.player.feet(&scene.graph);
                floor.update(scene, &mut self.prepared, player, wind, ctx.dt, false);
            }
        }
        if let Some(wind) = &self.wind {
            let at = self.player.feet(&scene.graph);
            let (look, side) = self.player.view_axes(&scene.graph);
            let blowing = !self.trees.is_empty();
            self.gauge.update(ctx.user_interfaces.first_mut(), wind, blowing, at, look, side, ctx.dt);
        }
        let scene = &mut ctx.scenes[self.scene];
        let graph = &mut scene.graph;
        if crate::stats()
            && (ctx.elapsed_time - ctx.dt).floor() != ctx.elapsed_time.floor()
        {
            Log::info(format!(
                "Player at {:?}; {} of the floor's plants and rocks in the scene; wind and floor {:.2} ms{}",
                self.player.feet(graph),
                self.floor.as_ref().map_or(0, Floor::spawned),
                started.elapsed().as_secs_f32() * 1000.0,
                self.day.as_ref().map_or(String::new(), |day| format!(
                    "; {} sun {:.1}°",
                    day.clock(),
                    day.lighting().elevation
                )),
            ));
        }
        log_stats(ctx);
        Ok(())
    }

    fn on_os_event(&mut self, event: &Event<()>, mut ctx: PluginContext) -> GameResult {
        // Measuring, the view stays put whatever is pressed.
        if crate::benchmarking() {
            return Ok(());
        }
        match event {
            Event::DeviceEvent {
                event: fyrox::event::DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if self.mouse_captured && self.focused {
                    self.player.look(delta.0 as f32, delta.1 as f32);
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key: PhysicalKey::Code(key),
                            state,
                            ..
                        },
                    ..
                } => {
                    let down = *state == ElementState::Pressed;
                    if *key == KeyCode::Escape && down {
                        self.set_mouse_captured(&mut ctx, false);
                    } else if matches!(key, KeyCode::KeyF | KeyCode::KeyL) && down {
                        self.light_the_way(&mut ctx.scenes[self.scene].graph, *key == KeyCode::KeyF);
                    } else if let (Some(day), true, KeyCode::BracketLeft | KeyCode::BracketRight | KeyCode::KeyP) =
                        (&mut self.day, down, key)
                    {
                        match key {
                            KeyCode::BracketLeft => day.step(-1.0),
                            KeyCode::BracketRight => day.step(1.0),
                            _ => day.toggle_pause(),
                        }
                    } else {
                        self.player.key(*key, down);
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if !self.mouse_captured {
                        self.set_mouse_captured(&mut ctx, true);
                    }
                }
                WindowEvent::Focused(focused) => {
                    self.focused = *focused;
                    if !focused {
                        self.player.release();
                        self.set_mouse_captured(&mut ctx, false);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn on_graphics_context_initialized(&mut self, ctx: PluginContext) -> GameResult {
        if let GraphicsContext::Initialized(graphics_context) = &*ctx.graphics_context {
            graphics_context.window.set_title("Wilderness");
        }
        Ok(())
    }
}
