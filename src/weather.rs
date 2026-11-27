//! Clear skies, rain or snow, as plant_life's Birch Glade weather presets make them (its
//! `environments/birch-glade/tools/weather.py`, 56461bc), here live, to be changed while walking:
//!
//! - The light (`apply_atmosphere`): under rain the sun at 32% and blue-grey, under snow at 49%
//!   and a cold white; the sky's light, 86%.
//! - The ground (`apply_ground`): rain darkens the loam (its colour times 0.69, 0.72, 0.76); snow
//!   lies in patches on the dry, gentler slopes - plant_life's own pattern of them, the same sums
//!   of waves over x and z, thinned by how steep the ground is - never on the water, a centimetre
//!   and a half over the soil, its edge where the patches' cover passes a half (as plant_life's
//!   cutout of its "Snow coverage").
//! - What falls (its two looping layers of particles before the camera): thin rain streaks or
//!   small feathered flakes, as many to a cubic meter, falling as fast; here in a box wrapped
//!   round the eye (see `precipitation.shader`).
//!
//! plant_life keeps weather and date apart, and so does this: WILDERNESS_WEATHER=clear, rain or
//! snow, and `R` turns from one to the next.
//!
//! The Wilderness's own: the falling is blown along by the wind here, as hard as plant_life's on
//! average; the sky drawn greyer and the sun's disk dimmed under cloud; and more haze, nearer, in
//! rain and snow, since a forest seen clear to its edge through rain looks wrong.

use crate::{random::Random, terrain::Terrain};
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
    resource::texture::{
        Texture, TextureKind, TextureMagnificationFilter, TextureMinificationFilter, TexturePixelKind, TextureResource,
        TextureWrapMode,
    },
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

/// How high the snow lies over the soil, in meters: plant_life's.
const SNOW_LIFT: f32 = 0.015;
/// The snow's cover's grid, in meters: the ground's, so it lies even over it.
const SNOW_SPACING: f32 = 0.5;

/// The weather now, and what it has made.
#[derive(Debug, Default)]
pub struct Weather {
    kind: Kind,
    /// The falling particles, hung from the camera, and their material.
    falling: Option<(Handle<Node>, MaterialResource)>,
    /// How far they have fallen, wrapped by the box; and the seconds gone by.
    fall: Vector3<f32>,
    time: f32,
    /// The snow lying, once made.
    snow: Vec<Handle<Node>>,
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
        let mut weather = Self { kind, falling, ..Default::default() };
        weather.show(&mut scene.graph, None);
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

    /// Turns to `kind`: the snow lying made on `terrain` the first time it is wanted.
    pub fn set(&mut self, graph: &mut Graph, kind: Kind, terrain: Option<&Terrain>) {
        self.kind = kind;
        self.show(graph, terrain);
        self.wet_the_loam();
        Log::info(format!("Weather: {}", kind.name()));
    }

    fn wet_the_loam(&self) {
        if let Some((loam, color)) = &self.loam {
            let wet = if self.kind == Kind::Rain { tinted(*color, WET) } else { *color };
            loam.data_ref().set_property("diffuseColor", wet);
        }
    }

    /// Shows what this weather has, and hides what it has not.
    fn show(&mut self, graph: &mut Graph, terrain: Option<&Terrain>) {
        if self.kind == Kind::Snow && self.snow.is_empty() {
            if let Some(terrain) = terrain {
                let started = std::time::Instant::now();
                let (snow, triangles) = lay_snow(graph, terrain);
                self.snow = snow;
                Log::info(format!("Weather: the snow laid, {triangles} triangles, in {:.2} s", started.elapsed().as_secs_f32()));
            }
        }
        for &patch in &self.snow {
            graph[patch].set_visibility(self.kind == Kind::Snow);
        }
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

/// How much snow lies at `x`, `z`, 0 to 1, where the ground's normal is `normal`: plant_life's
/// patches (`apply_ground`), thinned on steep ground, none in the water.
pub fn snow_cover(terrain: &Terrain, x: f32, z: f32, normal: Vector3<f32>) -> f32 {
    if terrain.wet(x, z, 0.0) {
        return 0.0;
    }
    // plant_life's y across the land is z here.
    let y = z;
    let mut patches = 0.64 + 0.22 * (1.6 * x + 0.6 * y + 0.7 * y.sin()).sin();
    patches += 0.15 * (2.9 * y - 1.9 * x).sin() + 0.09 * (5.2 * x + 4.4 * y).cos();
    let slope = ((normal.y - 0.62) / 0.35).clamp(0.0, 1.0);
    (patches * slope).clamp(0.0, 1.0)
}

/// Lays the snow over `terrain`, in the ground's squares: each cell with a corner covered more
/// than half, its corners told how much (the texture's u), which the material's cutout cuts at a
/// half. Returns the squares and how many triangles they have.
fn lay_snow(graph: &mut Graph, terrain: &Terrain) -> (Vec<Handle<Node>>, usize) {
    let half = crate::terrain::half_size();
    let chunk = crate::terrain::CHUNK;
    let chunks = (2.0 * half / chunk).ceil() as i32;
    let cells = (chunk / SNOW_SPACING).round() as usize;
    let material = snow_material();
    let mut nodes = Vec::new();
    let mut total = 0;
    for cx in 0..chunks {
        for cz in 0..chunks {
            let origin = Vector2::new(-half + cx as f32 * chunk, -half + cz as f32 * chunk);
            let across = cells + 1;
            let mut vertices = Vec::with_capacity(across * across);
            let mut cover = Vec::with_capacity(across * across);
            for j in 0..across {
                for i in 0..across {
                    let x = origin.x + i as f32 * SNOW_SPACING;
                    let z = origin.y + j as f32 * SNOW_SPACING;
                    let e = SNOW_SPACING;
                    let dx = terrain.height(x + e, z) - terrain.height(x - e, z);
                    let dz = terrain.height(x, z + e) - terrain.height(x, z - e);
                    let normal = Vector3::new(-dx, 2.0 * e, -dz).normalize();
                    let c = snow_cover(terrain, x, z, normal);
                    cover.push(c);
                    vertices.push(StaticVertex {
                        position: Vector3::new(x, terrain.height(x, z) + SNOW_LIFT, z),
                        tex_coord: Vector2::new(c, 0.5),
                        normal,
                        tangent: Vector4::new(1.0, 0.0, 0.0, 1.0),
                    });
                }
            }
            let mut triangles = Vec::new();
            for j in 0..cells {
                for i in 0..cells {
                    let a = j * across + i;
                    let (b, c, d) = (a + 1, a + across, a + across + 1);
                    if [a, b, c, d].iter().all(|&k| cover[k] <= 0.5) {
                        continue;
                    }
                    let (a, b, c, d) = (a as u32, b as u32, c as u32, d as u32);
                    triangles.push(TriangleDefinition([a, c, b]));
                    triangles.push(TriangleDefinition([b, c, d]));
                }
            }
            if triangles.is_empty() {
                continue;
            }
            total += triangles.len();
            // Only the vertices used, so the mesh's bounds are the snow's.
            let mut used = vec![u32::MAX; vertices.len()];
            let mut kept = Vec::new();
            for triangle in &mut triangles {
                for index in &mut triangle.0 {
                    let k = *index as usize;
                    if used[k] == u32::MAX {
                        used[k] = kept.len() as u32;
                        kept.push(vertices[k]);
                    }
                    *index = used[k];
                }
            }
            let data = SurfaceData::new(VertexBuffer::new(kept.len(), kept).expect("a valid vertex layout"), TriangleBuffer::new(triangles));
            let node = MeshBuilder::new(BaseBuilder::new().with_name("Snow cover").with_cast_shadows(false))
                .with_surfaces(vec![SurfaceBuilder::new(SurfaceResource::new_embedded(data)).with_material(material.clone()).build()])
                .build(graph)
                .to_base();
            nodes.push(node);
        }
    }
    (nodes, total)
}

/// The snow's material: its colour and cover along a strip, u from none to whole - plant_life's
/// shade, (.83, .90, 1) times .77 to .98, and its cover as the alpha the cutout cuts at a half.
fn snow_material() -> MaterialResource {
    const WIDE: usize = 64;
    let to_srgb = |c: f32| {
        let c = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let bytes: Vec<u8> = (0..WIDE)
        .flat_map(|i| {
            let cover = i as f32 / (WIDE - 1) as f32;
            let shade = 0.77 + 0.21 * cover;
            [to_srgb(0.83 * shade), to_srgb(0.90 * shade), to_srgb(shade), (cover * 255.0).round() as u8]
        })
        .collect();
    let mut texture = Texture::from_bytes(TextureKind::Rectangle { width: WIDE as u32, height: 1 }, TexturePixelKind::SRGBA8, bytes)
        .expect("the strip's size matches its bytes");
    texture.set_magnification_filter(TextureMagnificationFilter::Linear);
    texture.set_minification_filter(TextureMinificationFilter::Linear);
    texture.set_s_wrap_mode(TextureWrapMode::ClampToEdge);
    texture.set_t_wrap_mode(TextureWrapMode::ClampToEdge);
    let mut material = Material::standard();
    material.bind("diffuseTexture", TextureResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, texture));
    MaterialResource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, material)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snow_lies_in_patches_on_gentle_dry_ground() {
        let terrain = Terrain::new(crate::game::SEED);
        let flat = Vector3::y();
        let steep = Vector3::new(0.8, 0.6, 0.0).normalize();
        let (mut covered, mut bare, mut steep_covered) = (0, 0, 0);
        for i in -60..60 {
            for j in -60..60 {
                let (x, z) = (i as f32 * 1.3, j as f32 * 1.3);
                if terrain.wet(x, z, 0.0) {
                    assert_eq!(snow_cover(&terrain, x, z, flat), 0.0, "snow in the water at {x}, {z}");
                    continue;
                }
                if snow_cover(&terrain, x, z, flat) > 0.5 {
                    covered += 1;
                } else {
                    bare += 1;
                }
                if snow_cover(&terrain, x, z, steep) > 0.5 {
                    steep_covered += 1;
                }
            }
        }
        // Patches: much of the flat ground covered, but not all of it; none on the steep.
        assert!(covered > bare / 2 && bare > covered / 10, "{covered} covered, {bare} bare");
        assert_eq!(steep_covered, 0);
    }

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
