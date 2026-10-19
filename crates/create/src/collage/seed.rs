//! The collage's seeded generator: SplitMix64 (Steele, Lea and Flood, 2014), written from its
//! public description. Integer arithmetic only, so the same seed gives the same numbers on every
//! system and every build; no `rand` dependency. The layouts make their choices with integers
//! ([`Rng::below`], [`Rng::shuffle`]) and draw sizes and angles from [`Rng::unit`], whose 53 bits
//! are exact in an `f64`.

/// SplitMix64's increment (the golden ratio's fraction in 64 bits).
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// A small, fast, seeded generator. Not for anything secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// The generator for `seed`.
    pub const fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// An independent generator for one purpose of a seed (`stream` 0, 1, 2, … for the slots, the
    /// angles, …), so adding a draw to one purpose never moves another's numbers.
    pub fn stream(seed: u64, stream: u64) -> Rng {
        Rng::new(mix(seed ^ mix(stream.wrapping_add(1).wrapping_mul(GAMMA))))
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        mix(self.state)
    }

    /// A number in `0..n` without bias (Lemire's multiply-and-reject); 0 when `n` is 0.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        // The low part of the product is below `threshold` for exactly the draws that would bias
        // the result; at most a handful of redraws, almost always none.
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = u128::from(self.next_u64()) * u128::from(n);
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }

    /// A number in `[0, 1)` from the top 53 bits (exact in an `f64`).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A number in `[lo, hi)` (`lo` when the range is empty or not finite).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let v = lo + (hi - lo) * self.unit();
        if v.is_finite() && hi > lo { v } else { lo }
    }

    /// Fisher–Yates: every order equally likely.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
