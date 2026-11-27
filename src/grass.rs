//! Where grass grows, for the engine's grass ([`fyrox_gfx::grass`]): a map over the whole land of
//! the ground's height and how thick the grass is, a sample to every [`SPACING`] meters.
//!
//! Thickest in the glades, thin under the dark canopy of a thicket, in uneven patches, as grass
//! grows; none in the water or on its very edge, and none in a trunk.

use crate::{
    forest::{self, Thicket, Tree},
    random::{Noise, smoothstep},
    terrain::Terrain,
};
use fyrox_gfx::GrassMap;

/// Meters between the map's samples.
const SPACING: f32 = 1.0;
/// How far from the water's edge grass keeps, in meters.
const OFF_WATER: f32 = 0.3;
/// How thick the grass is under the darkest canopy, against a glade's.
const IN_SHADE: f32 = 0.25;

/// The map of where grass grows on `terrain`, among `trees`, from `seed`.
pub fn map(seed: u64, terrain: &Terrain, trees: &[Tree]) -> GrassMap {
    let extent = forest::HALF_EXTENT + crate::game::BORDER;
    let samples = (2.0 * extent / SPACING).ceil() as u32 + 1;
    let origin = [-extent, -extent];
    let thicket = Thicket::new(seed);
    let patches = Noise(seed ^ 0x6a55_0f1e);
    let mut heights = Vec::with_capacity((samples * samples) as usize);
    let mut density = Vec::with_capacity((samples * samples) as usize);
    for row in 0..samples {
        for column in 0..samples {
            let (x, z) = (origin[0] + column as f32 * SPACING, origin[1] + row as f32 * SPACING);
            heights.push(terrain.height(x, z));
            let shade = thicket.at(x, z);
            let patch = 0.55 + 0.45 * smoothstep(0.3, 0.7, patches.at(x / 6.0, z / 6.0));
            let thick = if terrain.wet(x, z, OFF_WATER) { 0.0 } else { (1.0 - (1.0 - IN_SHADE) * shade) * patch };
            density.push((thick.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    // None in a trunk: the samples within a trunk's reach of a tree.
    for tree in trees {
        let reach = forest::SPECIES[tree.species].trunk * tree.scale + 0.2;
        let (cx, cz) = ((tree.position.x - origin[0]) / SPACING, (tree.position.z - origin[1]) / SPACING);
        let r = (reach / SPACING).ceil() as i32;
        for dz in -r..=r {
            for dx in -r..=r {
                let (column, row) = (cx.round() as i32 + dx, cz.round() as i32 + dz);
                if column < 0 || row < 0 || column >= samples as i32 || row >= samples as i32 {
                    continue;
                }
                let at = (origin[0] + column as f32 * SPACING, origin[1] + row as f32 * SPACING);
                if (at.0 - tree.position.x).hypot(at.1 - tree.position.z) <= reach {
                    density[(row as u32 * samples + column as u32) as usize] = 0;
                }
            }
        }
    }
    GrassMap { origin, spacing: SPACING, width: samples, depth: samples, heights, density }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_grows_on_land_thicker_in_the_open_and_never_in_water() {
        let terrain = Terrain::new(1);
        let trees = forest::lay_out(1, &terrain);
        let map = map(1, &terrain, &trees);
        assert_eq!(map.heights.len(), (map.width * map.depth) as usize);
        assert_eq!(map.density.len(), map.heights.len());
        let (mut wet, mut grown) = (0, 0);
        for row in 0..map.depth {
            for column in 0..map.width {
                let i = (row * map.width + column) as usize;
                let (x, z) = (map.origin[0] + column as f32 * map.spacing, map.origin[1] + row as f32 * map.spacing);
                assert!((map.heights[i] - terrain.height(x, z)).abs() < 1.0e-4);
                if terrain.wet(x, z, 0.0) {
                    wet += 1;
                    assert_eq!(map.density[i], 0, "grass in the water at {x}, {z}");
                }
                grown += (map.density[i] > 0) as usize;
            }
        }
        assert!(wet > 0, "the land has water");
        assert!(grown > map.density.len() / 2, "grass over most of the land: {grown} of {}", map.density.len());
    }
}
