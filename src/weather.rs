//! Clear skies, rain or snow, as plant_life's Birch Glade weather presets make them (its
//! `environments/birch-glade/tools/weather.py`, 56461bc), here live, to be changed while walking:
//!
//! - The light (`apply_atmosphere`): under rain the sun at 32% and blue-grey, under snow at 49%
//!   and a cold white; the sky's light, 86%.
//! - The ground (`apply_ground`): rain darkens the loam (its colour times 0.69, 0.72, 0.76); snow
//!   builds up on the floor and the branches as the hours of snowfall go by (see `snow.rs`,
//!   plant_life 503da06). WILDERNESS_SNOW_HOURS (0 to 24, plant_life's 12 by default) is how
//!   much has fallen when it is snowing from the start.
//! - What falls (its two looping layers of particles before the camera): thin rain streaks or
//!   small feathered flakes, as many to a cubic meter, falling as fast; here in a box wrapped
//!   round the eye (see `precipitation.shader`).
//!
//! plant_life keeps weather and date apart, and so does this: WILDERNESS_WEATHER=clear, rain or
//! snow, and `R` turns from one to the next.
//!
//! The Wilderness's own: the falling is blown along by the wind here, as hard as plant_life's on
//! average; the sky drawn greyer and the sun's disk dimmed under cloud; more haze, nearer, in
//! rain and snow, since a forest seen clear to its edge through rain looks wrong; and the snow
//! melting once it stops - slowly under a clear sky, a day's fall in four days, and twice as fast
//! as it fell in the rain (plant_life's snow only falls).

use crate::{random::Random, snow::{Snow, FULL_HOURS}};
use fyrox::{
    asset::untyped::ResourceKind,
    core::{
        algebra::{Vector2, Vector3, Vector4},
        color::Color,
        log::Log,
        math::TriangleDefinition,
        pool::Handle,
        uuid::Uuid,
    },
    graph::SceneGraph,
    material::{shader::{Shader, ShaderResource}, Material, MaterialResource},
    scene::{
        base::BaseBuilder,
        graph::Graph,
        mesh::{
            buffer::{TriangleBuffer, VertexBuffer},
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            vertex::StaticVertex,
            MeshBuilder, RenderPath,
        },
        node::Node,
        Scene,
    },
};

const SHADER: &str = include_str!("precipitation.shader");

/// How many hours of snow lie when it is snowing from the start: WILDERNESS_SNOW_HOURS, or
/// plant_life's default.
fn starting_snow() -> f32 {
    std::env::var("WILDERNESS_SNOW_HOURS")
        .ok()
        .and_then(|h| h.parse::<f32>().ok())
        .map_or(crate::snow::DEFAULT_HOURS, |h| h.clamp(0.0, FULL_HOURS))
}

/// Which weather.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Clear,
    Rain,
    Snow,
}

impl Kind {
    /// WILDERNESS_WEATHER's, or clear.
    pub fn from_env() -> Self {
        match std::env::var("WILDERNESS_WEATHER").as_deref() {
            Ok("rain") => Self::Rain,
            Ok("snow") => Self::Snow,
            Ok("clear") | Err(_) => Self::Clear,
            Ok(other) => {
                Log::warn(format!("WILDERNESS_WEATHER={other}: no such weather (clear, rain or snow); clear"));
                Self::Clear
            }
        }
    }

    /// The weather after this one, round again.
    pub fn next(self) -> Self {
        match self {
            Self::Clear => Self::Rain,
            Self::Rain => Self::Snow,
            Self::Snow => Self::Clear,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Snow => "snow",
        }
    }
}

/// How the weather changes the light: the sun's strength, and its colour (times), as plant_life's;
/// the ambient light's, as its sky's strength; and how far the sky is greyed, the sun's disk
/// dimmed, and the haze thickened and brought nearer (the Wilderness's own).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overcast {
    pub sun: f32,
    pub sun_tint: [f32; 3],
    pub ambient: f32,
    pub grey: f32,
    pub disk: f32,
    pub haze: f32,
    pub haze_start: f32,
}

impl Kind {
    pub fn overcast(self) -> Overcast {
        match self {
            Self::Clear => Overcast { sun: 1.0, sun_tint: [1.0; 3], ambient: 1.0, grey: 0.0, disk: 1.0, haze: 1.0, haze_start: 1.0 },
            // plant_life's sun is (1, .94, .85) in the clear, (.73, .81, .93) in rain and
            // (.82, .88, 1) in snow: what turns the one into the other.
            Self::Rain => Overcast {
                sun: 0.32,
                sun_tint: [0.73, 0.81 / 0.94, 0.93 / 0.85],
                ambient: 0.86,
                grey: 0.7,
                disk: 0.15,
                haze: 1.6,
                haze_start: 0.55,
            },
            Self::Snow => Overcast {
                sun: 0.49,
                sun_tint: [0.82, 0.88 / 0.94, 1.0 / 0.85],
                ambient: 0.86,
                grey: 0.6,
                disk: 0.3,
                haze: 1.8,
                haze_start: 0.5,
            },
        }
    }
}

impl Overcast {
    /// `lit` under this sky.
    pub fn apply(&self, lit: &mut crate::day::Lighting) {
        lit.strength *= self.sun;
        lit.color = tinted(lit.color, self.sun_tint);
        lit.ambient = tinted(lit.ambient, [self.ambient; 3]);
        lit.sky = greyed(lit.sky, self.grey);
        lit.zenith = greyed(lit.zenith, self.grey);
        lit.haze = greyed(lit.haze, self.grey);
        lit.sun_color = tinted(lit.sun_color, [self.disk; 3]);
        lit.twilight *= 1.0 - self.grey;
    }

    /// `fog` under this sky, from its clear-weather `clear`.
    pub fn haze(&self, fog: &mut fyrox::scene::Fog, clear: &fyrox::scene::Fog) {
        fog.density = clear.density * self.haze;
        fog.start = clear.start * self.haze_start;
    }
}

/// `color` times `factors`, each channel, as it is stored.
pub fn tinted(color: Color, factors: [f32; 3]) -> Color {
    let c = |v: u8, f: f32| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    Color::from_rgba(c(color.r, factors[0]), c(color.g, factors[1]), c(color.b, factors[2]), color.a)
}

/// `color` greyed by `amount`, 0 to 1, keeping how bright it is.
pub fn greyed(color: Color, amount: f32) -> Color {
    let grey = 0.2126 * color.r as f32 + 0.7152 * color.g as f32 + 0.0722 * color.b as f32;
    let g = |v: u8| (v as f32 + (grey - v as f32) * amount).round().clamp(0.0, 255.0) as u8;
    Color::from_rgba(g(color.r), g(color.g), g(color.b), color.a)
}

/// How rain wets the loam: its colour times these, as plant_life's.
const WET: [f32; 3] = [0.69, 0.72, 0.76];

/// What falls, for each kind: how many to a cubic meter (plant_life's 1680 rain streaks and 1300
/// flakes in its 12 x 16 x 8 m volume), the longest half-length and the widest half-width, in
/// meters, and the least of each as a share of that, how fast they fall (plant_life's: 8 m in 27
/// frames, or 132, at 24 a second), how hard the wind blows them along, and how opaque, and how
/// lit by the sky, they are at most (its 60% mix, and its glow). Snowflakes twice plant_life's
/// size, big wet ones: as small as its, they hardly showed in a window the game's size.
struct Fall {
    per_cubic_meter: f32,
    half_length: (f32, f32),
    half_width: (f32, f32),
    speed: f32,
    blown: f32,
    opacity: f32,
    light: f32,
    tint: [f32; 3],
}

const RAIN: Fall = Fall {
    per_cubic_meter: 1680.0 / (12.0 * 16.0 * 8.0),
    half_length: (0.045, 0.15),
    half_width: (0.0007, 0.0016),
    speed: 8.0 / (27.0 / 24.0),
    blown: 2.2,
    opacity: 0.6,
    light: 1.7,
    tint: [0.54, 0.65, 0.78],
};
const SNOW: Fall = Fall {
    per_cubic_meter: 1300.0 / (12.0 * 16.0 * 8.0),
    half_length: (0.008, 0.022),
    half_width: (0.008, 0.02),
    speed: 8.0 / (132.0 / 24.0),
    blown: 0.6,
    opacity: 0.9,
    light: 1.9,
    tint: [0.85, 0.91, 1.0],
};

/// The box falling round the eye: meters across, up, and along.
const BOX: Vector3<f32> = Vector3::new(16.0, 10.0, 16.0);

/// How fast the snow melts once it stops, against how fast it fell: under a clear sky, and in
/// the rain (the Wilderness's own).
const MELT: (f32, f32) = (0.25, 2.0);

/// The weather now, and what it has made.
#[derive(Debug, Default)]
pub struct Weather {
    kind: Kind,
    /// The falling particles, hung from the camera, and their material.
    falling: Option<(Handle<Node>, MaterialResource)>,
    /// How far they have fallen, wrapped by the box; and the seconds gone by.
    fall: Vector3<f32>,
    time: f32,
    /// The snow lying, once laid out, and the hours of snowfall it lies as.
    snow: Option<Snow>,
    snow_hours: f32,
    /// The loam, and its own colour, to wet in the rain.
    loam: Option<(MaterialResource, Color)>,
}

impl PartialEq for Weather {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Weather {
    /// The weather `kind`, its particles hung from `camera`.
    pub fn build(scene: &mut Scene, camera: Handle<Node>, kind: Kind) -> Self {
        let falling = match Shader::from_string(SHADER) {
            Ok(shader) => {
                let shader = ShaderResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, shader);
                Some(build_falling(&mut scene.graph, camera, &shader))
            }
            Err(error) => {
                Log::err(format!("The rain and snow's shader would not build: {error:?}; nothing falls"));
                None
            }
        };
        let snow_hours = if kind == Kind::Snow { starting_snow() } else { 0.0 };
        let mut weather = Self { kind, falling, snow_hours, ..Default::default() };
        weather.show(&mut scene.graph);
        weather
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The loam: wet in the rain from now on.
    pub fn set_loam(&mut self, loam: MaterialResource) {
        let color = match loam.data_ref().property_group_ref("properties").and_then(|g| g.property_ref("diffuseColor")) {
            Some(fyrox::material::MaterialProperty::Color(color)) => *color,
            _ => Color::WHITE,
        };
        self.loam = Some((loam, color));
        self.wet_the_loam();
    }

    /// The snow's places: it lies as the hours of snowfall say from now on.
    pub fn set_snow(&mut self, graph: &mut Graph, mut snow: Snow) {
        snow.update(graph, self.snow_hours, true);
        self.snow = Some(snow);
    }

    /// Turns to `kind`.
    pub fn set(&mut self, graph: &mut Graph, kind: Kind) {
        self.kind = kind;
        self.show(graph);
        self.wet_the_loam();
        Log::info(format!("Weather: {}", kind.name()));
    }

    /// The snow building up or melting as `hours` of the game go by (none while the clock stands
    /// still: then plant_life's snapshot, so many hours' fall while it snows, none otherwise).
    pub fn pass(&mut self, graph: &mut Graph, hours: Option<f32>) {
        self.snow_hours = match hours {
            Some(hours) => {
                let rate = match self.kind {
                    Kind::Snow => 1.0,
                    Kind::Clear => -MELT.0,
                    Kind::Rain => -MELT.1,
                };
                (self.snow_hours + rate * hours).clamp(0.0, FULL_HOURS)
            }
            None if self.kind == Kind::Snow => {
                if self.snow_hours <= 0.0 {
                    starting_snow()
                } else {
                    self.snow_hours
                }
            }
            None => 0.0,
        };
        if let Some(snow) = &mut self.snow {
            snow.update(graph, self.snow_hours, false);
        }
    }

    /// The snow, once laid out.
    pub fn snow(&self) -> Option<&Snow> {
        self.snow.as_ref()
    }

    /// Whether the snow's places have been laid out.
    pub fn has_snow(&self) -> bool {
        self.snow.is_some()
    }

    /// What the status line says of the weather: what it is, and how much snow lies.
    pub fn text(&self) -> String {
        let snow = self.snow_hours;
        if snow <= 0.0 {
            return self.kind.name().to_string();
        }
        let depth = crate::snow::settled_depth(1.0, 0.45, snow) * 100.0;
        format!("{}, {:.0} h of snow fallen, up to {depth:.0} cm deep", self.kind.name(), snow)
    }

    fn wet_the_loam(&self) {
        if let Some((loam, color)) = &self.loam {
            let wet = if self.kind == Kind::Rain { tinted(*color, WET) } else { *color };
            loam.data_ref().set_property("diffuseColor", wet);
        }
    }

    /// Shows what this weather has falling, and hides it when nothing falls.
    fn show(&mut self, graph: &mut Graph) {
        if let Some((node, material)) = &self.falling {
            graph[*node].set_visibility(self.kind != Kind::Clear);
            let fall = match self.kind {
                Kind::Snow => &SNOW,
                _ => &RAIN,
            };
            let mut material = material.data_ref();
            material.set_property("shape", Vector4::new(fall.half_length.1, fall.half_width.1, if self.kind == Kind::Snow { 1.0 } else { 0.0 }, fall.opacity));
            material.set_property("tint", Vector3::from(fall.tint));
            material.set_property("amount", fall.per_cubic_meter / RAIN.per_cubic_meter.max(SNOW.per_cubic_meter));
        }
    }

    /// Moves what falls on by `dt` seconds, blown by `wind` (its push along x and z), lit by the
    /// sky's `light` (the haze's colour, as it is stored).
    pub fn update(&mut self, wind: Vector2<f32>, light: Color, dt: f32) {
        let Some((_, material)) = &self.falling else {
            return;
        };
        if self.kind == Kind::Clear {
            return;
        }
        let fall = if self.kind == Kind::Snow { &SNOW } else { &RAIN };
        let velocity = Vector3::new(wind.x * fall.blown, -fall.speed, wind.y * fall.blown);
        self.time += dt;
        self.fall += velocity * dt;
        self.fall = Vector3::new(
            self.fall.x.rem_euclid(BOX.x),
            self.fall.y.rem_euclid(BOX.y),
            self.fall.z.rem_euclid(BOX.z),
        );
        let lit = light.srgb_to_linear_f32();
        let mut material = material.data_ref();
        material.set_property("fall", self.fall);
        material.set_property("along", velocity.normalize());
        material.set_property("light", Vector3::new(lit.x, lit.y, lit.z) * fall.light);
        material.set_property("time", self.time);
    }
}

/// The falling particles: as many quads as the box holds at the thickest, each its corners at
/// one place in the box, hung from `camera` so the box is never culled.
fn build_falling(graph: &mut Graph, camera: Handle<Node>, shader: &ShaderResource) -> (Handle<Node>, MaterialResource) {
    let count = (RAIN.per_cubic_meter.max(SNOW.per_cubic_meter) * BOX.x * BOX.y * BOX.z).round() as usize;
    let mut random = Random::for_place(0x5e1a_3a1f, 0, 0);
    let mut vertices = Vec::with_capacity(count * 4);
    let mut triangles = Vec::with_capacity(count * 2);
    // The least length and width, as shares of the most: rain's and snow's are near enough alike.
    let least = (RAIN.half_length.0 / RAIN.half_length.1, RAIN.half_width.0 / RAIN.half_width.1);
    for _ in 0..count {
        let place = Vector3::new(
            random.between(-0.5, 0.5) * BOX.x,
            random.between(-0.5, 0.5) * BOX.y,
            random.between(-0.5, 0.5) * BOX.z,
        );
        let own = Vector3::new(random.between(least.0, 1.0), random.between(least.1, 1.0), random.unit());
        let first = vertices.len() as u32;
        for corner in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(StaticVertex {
                position: place,
                tex_coord: Vector2::new(corner.0, corner.1),
                normal: own,
                tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
            });
        }
        triangles.push(TriangleDefinition([first, first + 1, first + 2]));
        triangles.push(TriangleDefinition([first, first + 2, first + 3]));
    }
    let data = SurfaceData::new(VertexBuffer::new(vertices.len(), vertices).expect("a valid vertex layout"), TriangleBuffer::new(triangles));
    let material = MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, Material::from_shader(shader.clone()));
    let node = MeshBuilder::new(BaseBuilder::new().with_name("Rain and snow").with_cast_shadows(false))
        .with_surfaces(vec![SurfaceBuilder::new(SurfaceResource::new_embedded(data)).with_material(material.clone()).build()])
        .with_render_path(RenderPath::Forward)
        .build(graph)
        .to_base();
    graph.link_nodes(node, camera);
    (node, material)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weather_turns_round_and_dims_the_sun() {
        assert_eq!(Kind::Clear.next().next().next(), Kind::Clear);
        assert!(Kind::Rain.overcast().sun < Kind::Snow.overcast().sun);
        // plant_life's sun colours, from its clear one.
        let clear = Color::opaque(255, 240, 217);
        let rain = tinted(clear, Kind::Rain.overcast().sun_tint);
        assert!((rain.r as i32 - 186).abs() <= 2 && (rain.b as i32 - 237).abs() <= 2, "{rain:?}");
        assert_eq!(greyed(Color::opaque(10, 200, 30), 1.0).r, greyed(Color::opaque(10, 200, 30), 1.0).g);
    }
}
