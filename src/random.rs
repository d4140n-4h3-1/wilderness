//! Seeded randomness: the forest, its floor and its wind need the same numbers for the same seed,
//! on every machine.

/// A small seeded random number generator (splitmix64).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Random(pub u64);

impl Random {
    /// A generator for one of many places or things, `a` and `b` saying which, from one seed.
    pub fn for_place(seed: u64, a: i64, b: i64) -> Self {
        let mut random = Random(
            seed ^ (a as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (b as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f),
        );
        random.next();
        random
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from 0 up to 1.
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// An index into `weights`, each picked as often as its weight against the rest; `None` if
    /// they are all nothing.
    pub fn pick(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let mut pick = self.unit() * total;
        for (index, weight) in weights.iter().enumerate() {
            pick -= weight;
            if pick <= 0.0 && *weight > 0.0 {
                return Some(index);
            }
        }
        weights.iter().rposition(|w| *w > 0.0)
    }
}

/// Smooth value noise, from 0 to 1: two octaves of random values at whole-number points,
/// blended between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noise(pub u64);

impl Noise {
    pub fn at(&self, x: f32, z: f32) -> f32 {
        (self.octave(x, z, 0) * 2.0 + self.octave(x * 2.3, z * 2.3, 1)) / 3.0
    }

    fn octave(&self, x: f32, z: f32, octave: u64) -> f32 {
        let (x0, z0) = (x.floor(), z.floor());
        let (fx, fz) = (smoothstep(0.0, 1.0, x - x0), smoothstep(0.0, 1.0, z - z0));
        let corner = |dx: i64, dz: i64| self.value(x0 as i64 + dx, z0 as i64 + dz, octave);
        let near = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * fx;
        let far = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * fx;
        near + (far - near) * fz
    }

    fn value(&self, x: i64, z: i64, octave: u64) -> f32 {
        let mut random = Random(
            self.0 ^ (x as u64).wrapping_mul(0x9e37_79b9) ^ (z as u64).wrapping_mul(0x85eb_ca6b) ^ (octave << 56),
        );
        random.unit()
    }
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
