//! A seeded random generator for the differential tests, which needs no dependency and repeats for its seed.

/// The shifts and the multiplier of Marsaglia's xorshift64*.
const SHIFTS: [u32; 3] = [12, 25, 27];
const MULTIPLIER: u64 = 0x2545_f491_4f6c_dd1d;

/// The units after NUL, which no argument or variable may hold.
const NONZERO_UNITS: u64 = 0xffff;

/// The variable that multiplies the number of random cases, which the nightly workflow raises.
const SCALE: &str = "WINDOWS_SPAWN_RANDOM_SCALE";

/// Returns `cases` times `WINDOWS_SPAWN_RANDOM_SCALE`, or `cases` when the variable is unset.
///
/// The seed stays the same, so a larger run checks the cases of a smaller one first.
pub(crate) fn scaled(cases: usize) -> std::io::Result<usize> {
    let invalid = |error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error);
    let scale = match std::env::var(SCALE) {
        Ok(text) => text
            .parse::<usize>()
            .map_err(|error| invalid(error.to_string()))?,
        Err(std::env::VarError::NotPresent) => 1,
        Err(error) => return Err(invalid(error.to_string())),
    };
    cases
        .checked_mul(scale)
        .ok_or_else(|| invalid(format!("{SCALE} times {cases} overflows")))
}

pub(crate) struct Generator(u64);

impl Generator {
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) const fn next(&mut self) -> u64 {
        let [first, second, third] = SHIFTS;
        let Self(state) = self;
        *state ^= state.wrapping_shr(first);
        *state ^= state.wrapping_shl(second);
        *state ^= state.wrapping_shr(third);
        state.wrapping_mul(MULTIPLIER)
    }

    /// A number below `bound`, or zero when `bound` is zero.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next().checked_rem(bound).unwrap_or_default()
    }

    /// Half the time a unit of `interesting`, otherwise any unit but NUL.
    pub(crate) fn unit(&mut self, interesting: &[u16]) -> u16 {
        if self.below(2) == 0 {
            let choices = u64::try_from(interesting.len()).unwrap_or(1);
            if let Some(unit) = usize::try_from(self.below(choices))
                .ok()
                .and_then(|index| interesting.get(index))
            {
                return *unit;
            }
        }
        u16::try_from(self.below(NONZERO_UNITS)).map_or(1, |unit| unit.saturating_add(1))
    }

    /// Up to `longest` units, each from [`Self::unit`].
    pub(crate) fn text(&mut self, longest: u64, interesting: &[u16]) -> Vec<u16> {
        let length = self.below(longest.saturating_add(1));
        (0..length).map(|_| self.unit(interesting)).collect()
    }
}
