//! A tiny, dependency-free, fixed-seed pseudo-random number generator used to drive
//! differential test-case generation across this whole `tests/differential/` harness.
//!
//! This is SplitMix64 (public-domain construction, see
//! <http://xoshiro.di.unimi.it/splitmix64.c>). It is not cryptographic and does not need
//! to be: the only properties we need are (a) deterministic given a fixed seed, so a
//! failing case is always reproducible just by re-running `cargo test`, and (b) decent
//! bit-mixing, so nearby seeds/calls don't produce visibly-correlated sequences.
//!
//! Deliberately hand-rolled instead of pulling in `rand`+`rand_chacha` or similar: the task
//! this supports asked for a tiny generator with no new dependency, and SplitMix64 is ~10
//! lines.
//!
//! Shared by every function-specific generator under `tests/differential/` -- see
//! `tests/differential/README.md` for how a new generator should use it.

#![allow(dead_code)] // not every helper here is used by every generator that includes this module

pub struct Prng(u64);

impl Prng {
    pub fn new(seed: u64) -> Self {
        Prng(seed)
    }

    /// Raw SplitMix64 step.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }

    pub fn next_i16(&mut self) -> i16 {
        (self.next_u64() >> 48) as i16
    }

    /// Uniform over the inclusive range `[lo, hi]`.
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(hi >= lo);
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as i64
    }

    pub fn range_u16(&mut self, lo: u16, hi: u16) -> u16 {
        self.range_i64(lo as i64, hi as i64) as u16
    }

    /// True with probability `1/one_in`.
    pub fn chance(&mut self, one_in: u32) -> bool {
        (self.next_u32() % one_in) == 0
    }

    /// Pick one element of a fixed slice uniformly at random.
    pub fn pick<'a, T>(&mut self, choices: &'a [T]) -> &'a T {
        &choices[self.range_i64(0, choices.len() as i64 - 1) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::Prng;

    #[test]
    fn deterministic_for_fixed_seed() {
        let mut a = Prng::new(12345);
        let mut b = Prng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Prng::new(1);
        let mut b = Prng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
