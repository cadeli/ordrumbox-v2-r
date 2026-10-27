//! Seedable random source (xoshiro256++, seeded through SplitMix64).
//!
//! The JS app calls `Math.random()` directly (13 call sites in `src/`), so a
//! generated pattern can never be reproduced. Here every random draw goes
//! through an `Rng` handed in by the caller: same seed, same pattern, and tests
//! can pin the outcome.

/// A small, fast, non-cryptographic generator.
#[derive(Debug, Clone)]
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    /// A generator fully determined by `seed`.
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let state = std::array::from_fn(|_| splitmix64(&mut sm));
        Self { state }
    }

    /// A generator seeded from the clock, for interactive use.
    pub fn from_time() -> Self {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Self::new(nanos)
    }

    pub fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = &mut self.state;
        let result = s0.wrapping_add(*s3).rotate_left(23).wrapping_add(*s0);
        let t = *s1 << 17;
        *s2 ^= *s0;
        *s3 ^= *s1;
        *s1 ^= *s2;
        *s0 ^= *s3;
        *s2 ^= t;
        *s3 = s3.rotate_left(45);
        result
    }

    /// Uniform in `[0, 1)`, the `Math.random()` contract.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform integer in `0..n` (unbiased, Lemire's method). `n` must be > 0.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0)");
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = u128::from(self.next_u64()) * u128::from(n);
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }

    /// True with probability `p`, clamped to `[0, 1]`. A certain event (`p >= 1`)
    /// draws nothing, like `isProbabilityTriggered()` in pattern_engine.js, so
    /// deterministic patterns consume no randomness.
    pub fn chance(&mut self, p: f64) -> bool {
        if p >= 1.0 {
            return true;
        }
        if p.is_nan() || p <= 0.0 {
            return false;
        }
        self.next_f64() < p
    }

    /// One item of `items`, uniformly (`pickRandom()` in core/random.js).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.below(items.len() as u64) as usize)
    }

    /// Unbiased in-place shuffle (Fisher-Yates). Replaces the biased
    /// `sort(() => Math.random() - 0.5)` of the JS arpeggiator.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_reference_values() {
        let mut s = 0;
        assert_eq!(splitmix64(&mut s), 0xE220_A839_7B1D_CDAF);
        assert_eq!(splitmix64(&mut s), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let mut c = Rng::new(43);
        let sa: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let sb: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let sc: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
    }

    #[test]
    fn next_f64_in_unit_interval() {
        let mut r = Rng::new(1);
        for _ in 0..10_000 {
            let x = r.next_f64();
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn below_is_in_range_and_covers_it() {
        let mut r = Rng::new(7);
        let mut seen = [0u32; 6];
        for _ in 0..6_000 {
            seen[r.below(6) as usize] += 1;
        }
        assert!(seen.iter().all(|&count| (800..1200).contains(&count)), "{seen:?}");
    }

    #[test]
    fn chance_edges_draw_nothing() {
        let mut r = Rng::new(3);
        let before = r.clone().next_u64();
        assert!(r.chance(1.0));
        assert!(r.chance(2.0));
        assert!(!r.chance(0.0));
        assert!(!r.chance(-1.0));
        assert!(!r.chance(f64::NAN));
        assert_eq!(r.next_u64(), before);
    }

    #[test]
    fn pick_and_shuffle() {
        let mut r = Rng::new(9);
        assert_eq!(r.pick::<u8>(&[]), None);
        assert_eq!(r.pick(&[5]), Some(&5));
        let mut items: Vec<u32> = (0..32).collect();
        r.shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..32).collect::<Vec<_>>());
        assert_ne!(items, sorted);
    }
}
