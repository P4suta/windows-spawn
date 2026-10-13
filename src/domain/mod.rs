//! Decisions that depend only on their inputs, kept apart from Win32 so verifiers off Windows can reach them.
//!
//! Each `*_rules` module holds Kani harnesses beside the decisions they prove, compiled only under `cfg(kani)`.

pub(crate) mod attribute_rules;
pub(crate) mod command_line_rules;
pub(crate) mod creation_flag_rules;
pub(crate) mod environment_rules;
pub(crate) mod plan_rules;

/// Where an encoder writes the UTF-16 units it produces.
///
/// The spawn writes into a `Vec<u16>`; the proofs write into fixed buffers, which keeps allocation out of what they reason about.
pub(crate) trait UnitSink {
    /// Appends one unit.
    fn put(&mut self, unit: u16);

    /// Returns true if nothing was appended yet.
    fn is_empty(&self) -> bool;
}

impl UnitSink for Vec<u16> {
    fn put(&mut self, unit: u16) {
        self.push(unit);
    }

    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }
}

/// A fixed buffer for proofs and tests, which remembers a write past its end.
#[cfg(any(test, kani))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Buffer<const N: usize> {
    units: [u16; N],
    length: usize,
    overflowed: bool,
}

#[cfg(any(test, kani))]
impl<const N: usize> Buffer<N> {
    /// An empty buffer.
    pub(crate) const fn new() -> Self {
        Self {
            units: [0; N],
            length: 0,
            overflowed: false,
        }
    }

    /// The units appended so far, or `None` after a write past the end.
    pub(crate) fn as_slice(&self) -> Option<&[u16]> {
        if self.overflowed {
            None
        } else {
            self.units.get(..self.length)
        }
    }
}

#[cfg(any(test, kani))]
impl<const N: usize> UnitSink for Buffer<N> {
    fn put(&mut self, unit: u16) {
        match self.units.get_mut(self.length) {
            Some(slot) => {
                *slot = unit;
                self.length = self.length.saturating_add(1);
            }
            None => self.overflowed = true,
        }
    }

    fn is_empty(&self) -> bool {
        self.length == 0 && !self.overflowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proof_buffer_never_drops_a_write_silently() {
        let mut buffer = Buffer::<2>::new();
        assert!(UnitSink::is_empty(&buffer));
        buffer.put(1);
        buffer.put(2);
        assert_eq!(buffer.as_slice(), Some(&[1, 2][..]));
        buffer.put(3);
        assert_eq!(buffer.as_slice(), None);
        assert!(!UnitSink::is_empty(&buffer));

        let mut units = Vec::new();
        assert!(UnitSink::is_empty(&units));
        units.put(7);
        assert_eq!(units, [7]);
    }
}
