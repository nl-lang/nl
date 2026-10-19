//! A small deterministic random number generator: SplitMix64.
//!
//! The fuzzer needs the same program for the same seed on every machine
//! and every Rust version (a finding is reproduced from its seed), and
//! nothing cryptographic. SplitMix64 is a few lines,
//! has no bad seeds, and saves a dependency.

/// SplitMix64 (Steele, Lea and Flood, 2014), the seeder of xoshiro.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`; 0 when `n` is 0. The modulo bias is far below
    /// anything a fuzzer notices.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = hi.abs_diff(lo) + 1;
        lo.wrapping_add((self.next_u64() % span) as i64)
    }

    /// True with probability `percent` in 100.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.next_u64() % 100 < u64::from(percent)
    }

    /// An index chosen with the given weights; 0 when they are all zero.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u64 = weights.iter().map(|&w| u64::from(w)).sum();
        if total == 0 {
            return 0;
        }
        let mut r = self.next_u64() % total;
        for (i, &w) in weights.iter().enumerate() {
            if r < u64::from(w) {
                return i;
            }
            r -= u64::from(w);
        }
        0
    }

    /// One item of a slice, or `None` when it is empty.
    pub fn pick_ref<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        items.get(self.below(items.len())).cloned()
    }

    /// One item of a slice; the default value when it is empty.
    pub fn pick<T: Clone + Default>(&mut self, items: &[T]) -> T {
        items.get(self.below(items.len())).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..4 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        // The reference value of SplitMix64 for seed 0.
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn ranges() {
        let mut r = Rng::new(1);
        for _ in 0..1000 {
            let v = r.range(-3, 3);
            assert!((-3..=3).contains(&v));
            assert!(r.below(5) < 5);
        }
    }
}
