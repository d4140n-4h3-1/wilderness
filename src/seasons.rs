//! The year going round in the plants: leaves coming out in spring, turning gold and falling in
//! autumn, flowers only while they bloom, and the grasses and ferns greening and browning - by the
//! date on the calendar (see `day.rs`), as plant_life's Birch Glade makes its seasonal snapshots.
//!
//! plant_life's palette (`data/seasonal-foliage.json`, a copy of its
//! `assets/seasonal-foliage.json`) gives each material, by its glTF name, a tint through the year:
//! dated linear RGB gains, eased between with `t*t*(3-2*t)`, clamped to [0, 1] once multiplied.
//! Its leaf fall (`seasonal_growth.py`) gives each deciduous crown the days its leaves come out
//! between and fall between; each spray turns in four steps to its peak colour before it drops.
//! And its flowers show only between their bloom's days.
//!
//! Here the copies of a model all share its materials, and so its textures: the seasons are drawn
//! into the textures. Each of a tree's leaf textures is one spray, its twig and its leaves; it is
//! cut into leaf-sized cells, each with its own timing as plant_life gives each spray its own, so
//! the leaves turn and fall a few at a time while the twigs stay. A season's texture is made anew,
//! from the original's texels, once a day of the calendar for each material whose look changes, a
//! material each frame so no frame does them all; a new texture, not the old one changed, so that
//! the traced shadows, which keep what each texture lets through, see the leaves gone too.
//!
//! Flowers and fruit out of season are not drawn at all: each of their surfaces, in every planted
//! copy, is swapped for a stand-in of one empty triangle, and back when they bloom again - so
//! nothing of them is drawn, casts a shadow or is lit, as if they were not planted. A copy
//! planted later, as the floor fills in round the player, is seen to by [`Seasons::despawn_in`].

use crate::random::{smoothstep, Random};
use fyrox::{
    asset::{untyped::ResourceKind, Resource},
    core::{log::Log, math::TriangleDefinition, pool::Handle, uuid::Uuid},
    fxhash::FxHashMap,
    graph::SceneGraph,
    material::MaterialResource,
    resource::{
        model::ModelResource,
        texture::{Texture, TextureKind, TexturePixelKind, TextureResource},
    },
    scene::{
        graph::Graph,
        mesh::{
            buffer::TriangleBuffer,
            surface::{SurfaceBuilder, SurfaceResource},
            Mesh,
        },
        node::Node,
    },
};

const PALETTE: &str = include_str!("../data/seasonal-foliage.json");

/// Each deciduous crown's material: the days its leaves start and finish coming out, then the
/// days they start and finish falling. As plant_life's `LEAF_FALL`.
const LEAF_FALL: [(&str, [f32; 4]); 5] = [
    ("Silver Birch Leaves", [99.0, 134.0, 263.0, 315.0]),
    ("English Oak Leaves", [114.0, 153.0, 288.0, 334.0]),
    ("European Larch Needles", [100.0, 136.0, 281.0, 325.0]),
    ("Common alder Leaves", [104.0, 144.0, 287.0, 329.0]),
    ("White willow Leaves", [88.0, 127.0, 288.0, 334.0]),
];
/// The day each crown's colour is at its peak: what a spray turns to before it drops.
const PEAK_COLOR_DAY: [(&str, f32); 5] = [
    ("Silver Birch Leaves", 300.0),
    ("English Oak Leaves", 301.0),
    ("European Larch Needles", 296.0),
    ("Common alder Leaves", 299.0),
    ("White willow Leaves", 299.0),
];
/// The flowers' materials, and the first and last days they bloom.
const FLOWERS: [(&str, (f32, f32)); 4] = [
    ("English Bluebell Bells", (101.0, 154.0)),
    ("Oxeye Daisy Petals", (145.0, 265.0)),
    ("Oxeye Daisy Pollen", (145.0, 265.0)),
    ("Common Heather Blossoms", (194.0, 285.0)),
];
/// The Wilderness's own, for the deciduous plants plant_life gives no seasons to - a rowan, the
/// shrubs - so that no crown stays green into winter beside bare birches: as [`LEAF_FALL`], and
/// the colour each turns to (as the palette's gain at [`PEAK_COLOR_DAY`]). Taken from when each
/// leafs out and falls in central Europe, not from plant_life.
const OWN_LEAF_FALL: [(&str, [f32; 4], [f32; 3]); 5] = [
    ("Rowan Leaves", [95.0, 130.0, 268.0, 310.0], [3.6, 0.55, 0.18]),
    ("Common Hawthorn Leaves", [85.0, 120.0, 275.0, 320.0], [2.6, 0.75, 0.25]),
    ("Dog Rose Leaves", [90.0, 125.0, 275.0, 320.0], [2.2, 0.85, 0.3]),
    ("Burnet Rose Leaves", [95.0, 130.0, 270.0, 315.0], [2.4, 0.6, 0.25]),
    ("Bilberry Leaves", [105.0, 140.0, 270.0, 310.0], [3.2, 0.5, 0.25]),
];
/// And the flowers and fruit plant_life gives no season to, as [`FLOWERS`]: the Wilderness's own.
const OWN_FLOWERS: [(&str, (f32, f32)); 13] = [
    ("Dog Rose Petals", (152.0, 200.0)),
    ("Dog Rose Pollen", (152.0, 200.0)),
    ("Burnet Rose Petals", (140.0, 190.0)),
    ("Burnet Rose Pollen", (140.0, 190.0)),
    ("Common Poppy Petals", (160.0, 240.0)),
    ("Common Poppy Pollen", (160.0, 240.0)),
    ("Foxglove Bells", (160.0, 235.0)),
    ("Wood Sorrel Petals", (95.0, 140.0)),
    ("Common Broom Flowers", (120.0, 175.0)),
    ("Common Hawthorn Fruit", (245.0, 340.0)),
    ("Rowan Berries", (230.0, 320.0)),
    ("Bilberry Berries", (190.0, 250.0)),
    ("Meadow Foxtail Seedheads", (130.0, 250.0)),
];
/// How wide a leaf's cell is, as a share of the texture's side: about a leaf of a spray.
const CELL: f32 = 0.045;
/// Below this alpha a texel is not drawn (the renderer's cutout).
const CUTOUT: u8 = 128;

/// The tints through the year, by material name.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    days_per_year: f32,
    profiles: FxHashMap<String, Vec<(f32, [f32; 3])>>,
}

impl Palette {
    pub fn load() -> Self {
        let json: serde_json::Value = serde_json::from_str(PALETTE).expect("the palette is JSON");
        let days_per_year = json["days_per_year"].as_f64().unwrap_or(365.0) as f32;
        let mut profiles = FxHashMap::default();
        for (name, keys) in json["profiles"].as_object().into_iter().flatten() {
            let keys = keys
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|key| {
                    let day = key[0].as_f64()? as f32;
                    let rgb = key[1].as_array()?;
                    let gain = |i: usize| rgb.get(i).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    Some((day, [gain(0), gain(1), gain(2)]))
                })
                .collect();
            profiles.insert(name.clone(), keys);
        }
        Self { days_per_year, profiles }
    }

    /// The linear RGB gain for `material` on `day` (1 on January 1, fractions too): plant_life's
    /// `foliage_tint`. Unlisted materials keep their colour.
    pub fn tint(&self, material: &str, day: f32) -> [f32; 3] {
        let Some(keys) = self.profiles.get(material).filter(|keys| !keys.is_empty()) else {
            return [1.0; 3];
        };
        let year = self.days_per_year;
        let day = (day - 1.0).rem_euclid(year) + 1.0;
        for (i, &(start, color)) in keys.iter().enumerate() {
            let (end, following) = keys.get(i + 1).copied().unwrap_or((keys[0].0 + year, keys[0].1));
            if day <= end {
                let t = ((day - start) / (end - start)).clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                return [0, 1, 2].map(|k| color[k] + (following[k] - color[k]) * t);
            }
        }
        [1.0; 3]
    }
}

fn smooth(value: f32) -> f32 {
    smoothstep(0.0, 1.0, value)
}

/// When a spray finishes turning and when it falls, between its crown's first and last days of
/// leaf fall: plant_life's `crown_timing`. Sheltered inner sprays (`outer` low) turn later; every
/// spray turns before it drops.
fn crown_timing(first: f32, last: f32, variation: f32, drop_chance: f32, outer: f32) -> (f32, f32) {
    let turn = first - 5.0 + (last - first - 9.0) * (0.66 * variation + 0.34 * (1.0 - outer));
    let candidate = first + (last - first) * (0.75 * drop_chance + 0.25 * (1.0 - outer));
    (turn, candidate.max(turn + 8.0))
}

/// The share of a crown's leaves (by its leaves' material) lying fallen on `day`: none before its
/// leaf fall starts, all once it ends, as plant_life lays its litter; then, the Wilderness's own,
/// rotting away from the new year until none are left when the crown starts leafing out again.
/// None for a material that does not fall.
pub fn fallen(material: &str, day: f32) -> f32 {
    let Some([spring_start, _, first, last]) = LEAF_FALL
        .iter()
        .find(|(n, _)| *n == material)
        .map(|(_, fall)| *fall)
        .or_else(|| OWN_LEAF_FALL.iter().find(|(n, ..)| *n == material).map(|(_, fall, _)| *fall))
    else {
        return 0.0;
    };
    let day = (day - 1.0).rem_euclid(365.0) + 1.0;
    if day >= first {
        smooth((day - first) / (last - first))
    } else if day < spring_start {
        1.0 - smooth(day / spring_start)
    } else {
        0.0
    }
}

/// How one leaf looks on a day: gone, or tinted so.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Leaf {
    Gone,
    Tinted([f32; 3]),
}

/// How a crown's leaf - its cell's numbers: `variation`, `drop_chance`, `outer` - looks on `day`,
/// by plant_life's `apply_growth` and `apply_blender`.
fn leaf(palette: &Palette, name: &str, fall: [f32; 4], peak: [f32; 3], cell: [f32; 3], day: f32) -> Leaf {
    let [spring_start, spring_end, first, last] = fall;
    let [variation, drop_chance, outer] = cell;
    let day = (day - 1.0).rem_euclid(palette.days_per_year) + 1.0;
    let (turn, falls) = crown_timing(first, last, variation, drop_chance, outer);
    if day < spring_end {
        let dormancy = 1.0 - smooth((day - spring_start) / (spring_end - spring_start));
        if dormancy >= 1.0 || drop_chance < dormancy {
            return Leaf::Gone;
        }
    } else if day >= falls {
        return Leaf::Gone;
    } else if day >= turn - 12.0 {
        let step = ((smooth((day - turn + 12.0) / 12.0) * 4.0) as u32).min(3);
        if step > 0 {
            let amount = step as f32 / 3.0;
            return Leaf::Tinted(peak.map(|gain| 1.0 + (gain - 1.0) * amount));
        }
    }
    // Green: as the palette has the crown, but neutral from a little before its leaves fall, the
    // sprays' own steps taking it on from there.
    if day >= first - 17.0 {
        Leaf::Tinted([1.0; 3])
    } else {
        Leaf::Tinted(palette.tint(name, day))
    }
}

/// What a material does through the year.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// Leaves that come out and fall, cell by cell; the twigs stay.
    Crown { fall: [f32; 4], peak: [f32; 3], cells: Vec<u16>, numbers: Vec<[f32; 3]> },
    /// Flowers, shown between these days.
    Flowers(f32, f32),
    /// Only tinted by the palette.
    Tinted,
}

/// One material that changes with the seasons, and its original texels.
#[derive(Debug)]
struct Seasonal {
    name: String,
    material: MaterialResource,
    original: Texture,
    width: usize,
    height: usize,
    /// Bytes a texel: 4, or 3 for a texture without alpha.
    stride: usize,
    kind: Kind,
    /// Flowers' surfaces, each with the stand-in that takes its place out of season.
    stand_ins: Vec<(SurfaceResource, SurfaceResource)>,
    /// Whether flowers are out: as the surfaces in the scene are now.
    out: bool,
    /// What its texture was last made for, so it is made again only when that changes.
    made: Option<u64>,
}

/// Every material that changes with the seasons, and the day they were last drawn for.
#[derive(Debug)]
pub struct Seasons {
    palette: Palette,
    materials: Vec<Seasonal>,
    /// The next material to look at.
    next: usize,
}

/// The glTF material names of the model at `path`, by primitive, in order: as the importer gives
/// a mesh its surfaces. Empty if the file cannot be read.
pub fn material_names(path: &str) -> Vec<String> {
    let json = std::fs::read(path).ok().and_then(|bytes| {
        let length = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
        serde_json::from_slice::<serde_json::Value>(bytes.get(20..20 + length)?).ok()
    });
    let Some(json) = json else {
        return Vec::new();
    };
    let materials = json["materials"].as_array().cloned().unwrap_or_default();
    json["meshes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|mesh| mesh["primitives"].as_array().cloned().unwrap_or_default())
        .map(|primitive| {
            primitive["material"]
                .as_u64()
                .and_then(|m| materials.get(m as usize))
                .and_then(|m| m["name"].as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// The same materials, drawn for the same days.
impl PartialEq for Seasons {
    fn eq(&self, other: &Self) -> bool {
        self.materials.len() == other.materials.len()
            && self.materials.iter().zip(&other.materials).all(|(a, b)| a.material.key() == b.material.key() && a.made == b.made)
    }
}

impl Seasons {
    /// Finds every material of `models` (each with its file's path) that the seasons change.
    pub fn new(models: &[(&str, &ModelResource)]) -> Self {
        let palette = Palette::load();
        let mut materials: Vec<Seasonal> = Vec::new();
        for &(path, model) in models {
            if !model.is_ok() {
                continue;
            }
            let names = material_names(path);
            let model = model.data_ref();
            let graph = &model.get_scene().graph;
            let mesh = graph.linear_iter().find_map(|node| node.cast::<Mesh>());
            let Some(mesh) = mesh else {
                continue;
            };
            for (surface, name) in mesh.surfaces().iter().zip(&names) {
                let fall = LEAF_FALL
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, fall)| {
                        let day = PEAK_COLOR_DAY.iter().find(|(n, _)| n == name).map_or(fall[2] + 20.0, |(_, day)| *day);
                        (*fall, palette.tint(name, day))
                    })
                    .or_else(|| OWN_LEAF_FALL.iter().find(|(n, ..)| n == name).map(|(_, fall, peak)| (*fall, *peak)));
                let flowers = FLOWERS.iter().chain(&OWN_FLOWERS).find(|(n, _)| n == name).map(|(_, days)| *days);
                if fall.is_none() && flowers.is_none() && !palette.profiles.contains_key(name) {
                    continue;
                }
                let material = surface.material().clone();
                let stand_in = flowers.map(|_| (surface.data(), stand_in(&surface.data())));
                if let Some(seasonal) = materials.iter_mut().find(|m| m.material.key() == material.key()) {
                    seasonal.stand_ins.extend(stand_in);
                    continue;
                }
                let Some(original) = diffuse_texels(&material) else {
                    Log::warn(format!("Seasons: the texture of {name} in {path} cannot be drawn into; it stays as it is"));
                    continue;
                };
                let TextureKind::Rectangle { width, height } = original.kind() else {
                    continue;
                };
                let (width, height) = (width as usize, height as usize);
                let stride = original.pixel_kind().size_in_bytes().unwrap_or(4);
                if fall.is_some() && stride != 4 {
                    Log::warn(format!("Seasons: the leaves of {name} in {path} have no alpha to fall by; they only turn"));
                }
                let kind = if let (Some((fall, peak)), 4) = (fall, stride) {
                    let (cells, numbers) = leaf_cells(&original, width, height, name);
                    Kind::Crown { fall, peak, cells, numbers }
                } else if let Some((from, to)) = flowers {
                    Kind::Flowers(from, to)
                } else {
                    Kind::Tinted
                };
                materials.push(Seasonal {
                    name: name.clone(),
                    material,
                    original,
                    width,
                    height,
                    stride,
                    kind,
                    stand_ins: stand_in.into_iter().collect(),
                    out: true,
                    made: None,
                });
            }
        }
        Log::info(format!(
            "Seasons: {} materials change through the year ({})",
            materials.len(),
            materials.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
        ));
        Self { palette, materials, next: 0 }
    }

    /// Draws the seasons for `day` into the next material whose look has changed since it was last
    /// drawn, if any; with `all`, into every one. Flowers coming out or over are swapped in or
    /// out of `graph`. Returns how many were drawn.
    pub fn update(&mut self, graph: &mut Graph, day: f32, all: bool) -> usize {
        let mut drawn = 0;
        let mut swaps: FxHashMap<u64, SurfaceResource> = FxHashMap::default();
        for _ in 0..self.materials.len() {
            let index = self.next;
            self.next = (self.next + 1) % self.materials.len().max(1);
            let seasonal = &mut self.materials[index];
            // A whole day's look: made again once a day at most.
            let state = look_key(&self.palette, seasonal, day.floor());
            if seasonal.made == Some(state) {
                continue;
            }
            seasonal.made = Some(state);
            seasonal.draw(&self.palette, day.floor());
            if let Kind::Flowers(from, to) = seasonal.kind {
                let out = in_bloom(&self.palette, from, to, day.floor());
                if out != seasonal.out {
                    seasonal.out = out;
                    for (original, stand_in) in &seasonal.stand_ins {
                        let (gone, coming) = if out { (stand_in, original) } else { (original, stand_in) };
                        swaps.insert(gone.key(), coming.clone());
                    }
                }
            }
            drawn += 1;
            if !all {
                break;
            }
        }
        if !swaps.is_empty() {
            swap(graph, graph.root(), &swaps);
        }
        drawn
    }

    /// Takes the flowers out of season out of a copy just planted under `root`.
    pub fn despawn_in(&self, graph: &mut Graph, root: Handle<Node>) {
        let swaps: FxHashMap<u64, SurfaceResource> = self
            .materials
            .iter()
            .filter(|m| !m.out)
            .flat_map(|m| &m.stand_ins)
            .map(|(original, stand_in)| (original.key(), stand_in.clone()))
            .collect();
        if !swaps.is_empty() {
            swap(graph, root, &swaps);
        }
    }

    /// How many flowers' materials are out of season, their surfaces swapped out.
    pub fn despawned(&self) -> usize {
        self.materials.iter().filter(|m| !m.out).count()
    }
}

/// Whether flowers blooming from day `from` to `to` are out on `day`.
fn in_bloom(palette: &Palette, from: f32, to: f32, day: f32) -> bool {
    let day = (day - 1.0).rem_euclid(palette.days_per_year) + 1.0;
    (from..=to).contains(&day)
}

/// A stand-in for `original`: its vertices but a single triangle with no area, drawing nothing
/// and casting no shadow; without the wind's shapes, so nothing bends it.
fn stand_in(original: &SurfaceResource) -> SurfaceResource {
    let mut data = original.data_ref().clone();
    data.geometry_buffer = TriangleBuffer::new(vec![TriangleDefinition([0, 0, 0])]);
    data.blend_shapes_container = None;
    SurfaceResource::new_embedded(data)
}

/// Every surface under `root` whose data is a key of `swaps` given that one's data instead, its
/// material kept. A new surface, not its data changed, so the renderer and the traced shadows,
/// which keep geometry by the data's key, take it afresh.
fn swap(graph: &mut Graph, root: Handle<Node>, swaps: &FxHashMap<u64, SurfaceResource>) {
    let meshes: Vec<Handle<Node>> = graph.traverse_handle_iter(root).filter(|&h| graph[h].cast::<Mesh>().is_some()).collect();
    for handle in meshes {
        let Some(mesh) = graph[handle].cast_mut::<Mesh>() else {
            continue;
        };
        for surface in mesh.surfaces_mut() {
            if let Some(data) = swaps.get(&surface.data_ref().key()) {
                *surface = SurfaceBuilder::new(data.clone()).with_material(surface.material().clone()).build();
            }
        }
    }
}

/// What decides how a material looks on `day`: its tint and which leaves or flowers show,
/// hashed. The same key, the same texture.
fn look_key(palette: &Palette, seasonal: &Seasonal, day: f32) -> u64 {
    let mut key: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: f32| key = (key ^ v.to_bits() as u64).wrapping_mul(0x100_0000_01b3);
    match &seasonal.kind {
        Kind::Crown { fall, peak, numbers, .. } => {
            for cell in numbers {
                match leaf(palette, &seasonal.name, *fall, *peak, *cell, day) {
                    Leaf::Gone => mix(-1.0),
                    Leaf::Tinted(t) => t.iter().for_each(|&v| mix(v)),
                }
            }
        }
        Kind::Flowers(from, to) => {
            let day = (day - 1.0).rem_euclid(palette.days_per_year) + 1.0;
            mix(if (*from..=*to).contains(&day) { 1.0 } else { 0.0 });
            palette.tint(&seasonal.name, day).iter().for_each(|&v| mix(v));
        }
        Kind::Tinted => palette.tint(&seasonal.name, day).iter().for_each(|&v| mix(v)),
    }
    key
}

impl Seasonal {
    /// Makes its texture for `day` from the original's texels, and gives it to its material.
    fn draw(&self, palette: &Palette, day: f32) {
        let mut texture = self.original.clone();
        let (width, height, stride) = (self.width, self.height, self.stride);
        let linear = srgb_to_linear_table();
        let tint_texel = |texel: &mut [u8], tint: [f32; 3]| {
            if tint == [1.0; 3] {
                return;
            }
            for k in 0..3 {
                texel[k] = linear_to_srgb((linear[texel[k] as usize] * tint[k]).clamp(0.0, 1.0));
            }
        };
        {
            let mut data = texture.modify();
            let bytes = data.data_mut();
            let base = &mut bytes[..width * height * stride];
            match &self.kind {
                Kind::Crown { fall, peak, cells, numbers } => {
                    let looks: Vec<Leaf> = numbers.iter().map(|cell| leaf(palette, &self.name, *fall, *peak, *cell, day)).collect();
                    for (texel, &cell) in base.chunks_exact_mut(stride).zip(cells) {
                        // The twig, and what is not drawn, stay as they are.
                        if cell == u16::MAX {
                            continue;
                        }
                        match looks[cell as usize] {
                            Leaf::Gone => texel[3] = 0,
                            Leaf::Tinted(tint) => tint_texel(texel, tint),
                        }
                    }
                }
                Kind::Flowers(from, to) => {
                    // Out of season they are not drawn at all (see `Seasons::update`).
                    if !in_bloom(palette, *from, *to, day) {
                        return;
                    }
                    let day = (day - 1.0).rem_euclid(palette.days_per_year) + 1.0;
                    let tint = palette.tint(&self.name, day);
                    for texel in base.chunks_exact_mut(stride) {
                        tint_texel(texel, tint);
                    }
                }
                Kind::Tinted => {
                    let tint = palette.tint(&self.name, day);
                    for texel in base.chunks_exact_mut(stride) {
                        tint_texel(texel, tint);
                    }
                }
            }
            if stride != 4 {
                downsample_opaque(bytes, width, height, stride);
            } else {
            // The smaller mip levels, each from the one above: the colour of what is drawn, how
            // much is drawn.
            let mut above = (0, width, height);
            let mut offset = width * height * 4;
            while offset < bytes.len() {
                let (start, w, h) = above;
                let (mw, mh) = ((w / 2).max(1), (h / 2).max(1));
                if offset + mw * mh * 4 > bytes.len() {
                    break;
                }
                for y in 0..mh {
                    for x in 0..mw {
                        let mut sum = [0u32; 4];
                        let mut weight = 0u32;
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                            let at = start + (sy * w + sx) * 4;
                            let a = bytes[at + 3] as u32;
                            for k in 0..3 {
                                sum[k] += bytes[at + k] as u32 * a;
                            }
                            sum[3] += a;
                            weight += a;
                        }
                        let at = offset + (y * mw + x) * 4;
                        for k in 0..3 {
                            bytes[at + k] = if weight > 0 { (sum[k] / weight) as u8 } else { bytes[at + k] };
                        }
                        bytes[at + 3] = (sum[3] / 4) as u8;
                    }
                }
                above = (offset, mw, mh);
                offset += mw * mh * 4;
            }
            }
        }
        let texture = Resource::new_ok(Uuid::new_v4(), ResourceKind::Embedded, texture);
        let mut material = self.material.data_ref();
        material.bind("diffuseTexture", Some(texture));
    }
}

/// The smaller mip levels of a texture without alpha, each the average of four texels of the one
/// above.
fn downsample_opaque(bytes: &mut [u8], width: usize, height: usize, stride: usize) {
    let (mut start, mut w, mut h) = (0, width, height);
    let mut offset = width * height * stride;
    while offset < bytes.len() {
        let (mw, mh) = ((w / 2).max(1), (h / 2).max(1));
        if offset + mw * mh * stride > bytes.len() {
            break;
        }
        for y in 0..mh {
            for x in 0..mw {
                for k in 0..stride {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        sum += bytes[start + (sy * w + sx) * stride + k] as u32;
                    }
                    bytes[offset + (y * mw + x) * stride + k] = (sum / 4) as u8;
                }
            }
        }
        (start, w, h) = (offset, mw, mh);
        offset += mw * mh * stride;
    }
}

/// A copy of `material`'s diffuse texture's texels, if they are 8-bit RGBA or RGB, as they can be
/// drawn into.
fn diffuse_texels(material: &MaterialResource) -> Option<Texture> {
    let material = material.data_ref();
    let texture: TextureResource = material.texture_ref("diffuseTexture")?.value.clone()?;
    let texture = texture.data_ref();
    let texels = (*texture).clone();
    let stride = match texels.pixel_kind() {
        TexturePixelKind::RGBA8 | TexturePixelKind::SRGBA8 => 4,
        TexturePixelKind::RGB8 | TexturePixelKind::SRGB8 => 3,
        other => {
            Log::warn(format!("Seasons: a texture of {other:?}"));
            return None;
        }
    };
    let TextureKind::Rectangle { width, height } = texels.kind() else {
        return None;
    };
    (texels.data().len() >= (width * height) as usize * stride).then_some(texels)
}

/// Each texel's leaf cell - [`u16::MAX`] for the twig and what is not drawn - and each cell's
/// numbers: how it varies, its chance of dropping early, and how far out in the crown it is.
fn leaf_cells(texture: &Texture, width: usize, height: usize, name: &str) -> (Vec<u16>, Vec<[f32; 3]>) {
    let texels = &texture.data()[..width * height * 4];
    let side = (CELL * width.max(height) as f32).max(4.0);
    let (across, down) = ((width as f32 / side).ceil() as usize + 1, (height as f32 / side).ceil() as usize + 1);
    // A seed point in each square, a little off its middle, each cell the texels nearest it.
    let seed = name.bytes().fold(0xce11_u64, |h, b| h.wrapping_mul(0x100_0000_01b3) ^ b as u64);
    let points: Vec<(f32, f32)> = (0..across * down)
        .map(|i| {
            let mut random = Random::for_place(seed, (i % across) as i64, (i / across) as i64);
            (((i % across) as f32 + random.between(0.15, 0.85)) * side, ((i / across) as f32 + random.between(0.15, 0.85)) * side)
        })
        .collect();
    let numbers = (0..points.len())
        .map(|i| {
            let mut random = Random::for_place(seed ^ 0x1eaf, i as i64, 0);
            // Further from the twig's middle line, the further out in the crown.
            let (x, _) = points[i];
            let outer = smooth((x / width as f32 - 0.5).abs() * 2.4 + random.between(-0.2, 0.2));
            [random.unit(), random.unit(), outer]
        })
        .collect();
    let mut cells = vec![u16::MAX; width * height];
    for y in 0..height {
        for x in 0..width {
            let texel = &texels[(y * width + x) * 4..][..4];
            // Not drawn, or the twig: brown, not green.
            if texel[3] < CUTOUT || (texel[1] as u32) * 20 < (texel[0] as u32) * 21 {
                continue;
            }
            let (cx, cy) = ((x as f32 / side) as usize, (y as f32 / side) as usize);
            let mut best = (f32::INFINITY, u16::MAX);
            for gy in cy.saturating_sub(1)..(cy + 2).min(down) {
                for gx in cx.saturating_sub(1)..(cx + 2).min(across) {
                    let (px, py) = points[gy * across + gx];
                    let d = (px - x as f32).powi(2) + (py - y as f32).powi(2);
                    if d < best.0 {
                        best = (d, (gy * across + gx) as u16);
                    }
                }
            }
            cells[y * width + x] = best.1;
        }
    }
    (cells, numbers)
}

fn srgb_to_linear_table() -> [f32; 256] {
    std::array::from_fn(|i| {
        let c = i as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    })
}

fn linear_to_srgb(linear: f32) -> u8 {
    let c = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_tints_as_plant_life_samples_it() {
        let palette = Palette::load();
        // On a key, its colour; in summer, none; between keys, eased.
        assert_eq!(palette.tint("Silver Birch Leaves", 274.0), [4.2, 0.94, 0.25]);
        assert_eq!(palette.tint("Silver Birch Leaves", 200.0), [1.0, 1.0, 1.0]);
        let between = palette.tint("Silver Birch Leaves", 287.0);
        assert!(between[0] > 4.2 && between[0] < 5.2, "{between:?}");
        // The year wraps: day 366 is day 1.
        assert_eq!(palette.tint("Silver Birch Leaves", 366.0), palette.tint("Silver Birch Leaves", 1.0));
        assert_eq!(palette.tint("Granite", 100.0), [1.0; 3]);
    }

    #[test]
    fn leaves_come_out_in_spring_turn_and_fall_in_autumn() {
        let palette = Palette::load();
        let name = "Silver Birch Leaves";
        let fall = LEAF_FALL[0].1;
        let peak = palette.tint(name, 300.0);
        let cells: Vec<[f32; 3]> = (0..400).map(|i| {
            let mut random = Random::for_place(9, i, 0);
            [random.unit(), random.unit(), random.unit()]
        }).collect();
        let share = |day: f32, test: &dyn Fn(Leaf) -> bool| {
            cells.iter().filter(|c| test(leaf(&palette, name, fall, peak, **c, day))).count() as f32 / cells.len() as f32
        };
        let gone = |l: Leaf| l == Leaf::Gone;
        let gold = |l: Leaf| matches!(l, Leaf::Tinted(t) if t[0] > 1.5);
        // Bare in winter, all out in summer, some out in the middle of spring.
        assert_eq!(share(30.0, &gone), 1.0);
        assert_eq!(share(200.0, &gone), 0.0);
        let spring = share(118.0, &gone);
        assert!(spring > 0.1 && spring < 0.9, "{spring}");
        // Green in summer, gold through the autumn, bare by December.
        assert_eq!(share(200.0, &gold), 0.0);
        assert!(share(295.0, &gold) > 0.3, "{}", share(295.0, &gold));
        assert_eq!(share(335.0, &gone), 1.0);
        // Every spray turns before it drops: none goes straight from green.
        for cell in &cells {
            let (turn, falls) = crown_timing(fall[2], fall[3], cell[0], cell[1], cell[2]);
            assert!(falls >= turn + 8.0);
        }
    }

    #[test]
    fn flowers_out_of_season_are_swapped_out_and_back() {
        use fyrox::scene::{base::BaseBuilder, mesh::MeshBuilder, mesh::surface::SurfaceData};
        let mut graph = Graph::new();
        let original = SurfaceResource::new_embedded(SurfaceData::make_quad(&fyrox::core::algebra::Matrix4::identity()));
        let other = SurfaceResource::new_embedded(SurfaceData::make_quad(&fyrox::core::algebra::Matrix4::identity()));
        let mesh: Handle<Node> = MeshBuilder::new(BaseBuilder::new())
            .with_surfaces(vec![SurfaceBuilder::new(other.clone()).build(), SurfaceBuilder::new(original.clone()).build()])
            .build(&mut graph)
            .to_base();
        let root = graph.root();
        let empty = stand_in(&original);
        assert_eq!(empty.data_ref().geometry_buffer.len(), 1);
        let keys = |graph: &Graph| graph[mesh].cast::<Mesh>().unwrap().surfaces().iter().map(|s: &fyrox::scene::mesh::surface::Surface| s.data_ref().key()).collect::<Vec<_>>();
        swap(&mut graph, root, &FxHashMap::from_iter([(original.key(), empty.clone())]));
        assert_eq!(keys(&graph), vec![other.key(), empty.key()]);
        swap(&mut graph, root, &FxHashMap::from_iter([(empty.key(), original.clone())]));
        assert_eq!(keys(&graph), vec![other.key(), original.key()]);
        let palette = Palette::load();
        assert!(in_bloom(&palette, 120.0, 175.0, 150.0) && !in_bloom(&palette, 120.0, 175.0, 300.0));
    }

    #[test]
    fn every_seasonal_material_is_in_the_models() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let names: Vec<String> = ["data/trees", "data/plants"]
            .iter()
            .flat_map(|d| std::fs::read_dir(dir.join(d)).unwrap())
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "glb"))
            .flat_map(|p| material_names(p.to_str().unwrap()))
            .collect();
        let own = OWN_LEAF_FALL.iter().map(|(n, ..)| *n).chain(OWN_FLOWERS.iter().map(|(n, _)| *n));
        for name in LEAF_FALL.iter().map(|(n, _)| *n).chain(FLOWERS.iter().map(|(n, _)| *n)).chain(own) {
            assert!(names.iter().any(|n| n == name), "no material named {name}");
        }
        let palette = Palette::load();
        for name in palette.profiles.keys() {
            assert!(names.iter().any(|n| n == name), "the palette's {name} is in no model");
        }
    }
}
