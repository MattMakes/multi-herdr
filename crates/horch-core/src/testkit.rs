#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "SplitMix64::below(0)");
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next_u64();
            if x >= threshold {
                return x % n;
            }
        }
    }
}

pub fn property<F: FnMut(&mut SplitMix64)>(seed: u64, cases: u32, mut f: F) {
    let mut seeds = SplitMix64::new(seed);
    for case in 0..cases {
        let case_seed = seeds.next_u64();
        let mut rng = SplitMix64::new(case_seed);
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut rng)));
        if let Err(panic) = run {
            eprintln!(
                "property failed: case {case} of {cases}, seed {seed:#018x}, \
                 case seed {case_seed:#018x} (replay with SplitMix64::new({case_seed:#018x}))"
            );
            std::panic::resume_unwind(panic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first5(seed: u64) -> [u64; 5] {
        let mut g = SplitMix64::new(seed);
        std::array::from_fn(|_| g.next_u64())
    }

    #[test]
    fn nfr_11_splitmix64_matches_reference() {
        assert_eq!(
            first5(0),
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F,
                0xF88B_B8A8_724C_81EC,
                0x1B39_896A_51A8_749B,
            ]
        );
        assert_eq!(
            first5(0x9E37_79B9_7F4A_7C15),
            [
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F,
                0xF88B_B8A8_724C_81EC,
                0x1B39_896A_51A8_749B,
                0x53CB_9F0C_747E_A2EA,
            ]
        );
    }

    #[test]
    fn below_stays_in_range() {
        let mut g = SplitMix64::new(42);
        let mut seen = [0u32; 7];
        for _ in 0..10_000 {
            let x = g.below(7);
            assert!(x < 7);
            seen[x as usize] += 1;
        }
        assert!(seen.iter().all(|&c| c > 1_000), "{seen:?}");
        assert_eq!(g.below(1), 0);
        let big = u64::MAX / 2 + 2;
        for _ in 0..10_000 {
            assert!(g.below(big) < big);
        }
    }

    #[test]
    fn property_runs_every_case_and_reports_failures() {
        let mut firsts = Vec::new();
        property(1, 50, |g| firsts.push(g.next_u64()));
        assert_eq!(firsts.len(), 50);
        let mut again = Vec::new();
        property(1, 50, |g| again.push(g.next_u64()));
        assert_eq!(firsts, again, "the same seed replays the same cases");

        let failed = std::panic::catch_unwind(|| {
            property(1, 50, |g| assert!(g.below(10) != 3, "drew a 3"));
        });
        assert!(failed.is_err(), "a failing case propagates its panic");
    }
}
