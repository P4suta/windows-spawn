//! Conversions between Win32 widths and Rust integers that `From` does not offer, written without `as`.
//!
//! A `DWORD` widens to a `usize`, and a compile-time size narrows to a `DWORD`, through their little-endian bytes.

use std::mem::size_of;

const _: () = assert!(
    size_of::<usize>() >= size_of::<u32>(),
    "a usize holds every DWORD"
);

/// Widens a `DWORD` through its bytes, which the low bytes of a `usize` hold.
pub(crate) fn widen(value: u32) -> usize {
    let mut bytes = [0_u8; size_of::<usize>()];
    for (slot, byte) in bytes.iter_mut().zip(value.to_le_bytes()) {
        *slot = byte;
    }
    usize::from_le_bytes(bytes)
}

/// Returns true if `value` fits a `DWORD`.
///
/// A shift by 32 bits finds the high bits of a 64-bit `usize`, and a 32-bit `usize` has none.
const fn fits_dword(value: usize) -> bool {
    match value.checked_shr(u32::BITS) {
        Some(high) => high == 0,
        None => true,
    }
}

/// Converts a compile-time size to a Win32 `DWORD`; a size that does not fit fails constant evaluation.
pub(crate) const fn dword_const(value: usize) -> u32 {
    assert!(fits_dword(value), "size does not fit a DWORD");
    let [first, second, third, fourth, ..] = value.to_le_bytes();
    u32::from_le_bytes([first, second, third, fourth])
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn every_dword_widens_to_its_value() {
        let value: u32 = kani::any();
        assert_eq!(widen(value) as u64, u64::from(value));
        kani::cover!(value == u32::MAX);
    }

    #[kani::proof]
    fn a_size_fits_a_dword_exactly_up_to_its_maximum() {
        let value: usize = kani::any();
        assert_eq!(fits_dword(value), value <= u32::MAX as usize);
        kani::cover!(value == u32::MAX as usize);
        kani::cover!(value == u32::MAX as usize + 1);
    }

    #[kani::proof]
    fn every_size_within_a_dword_narrows_to_its_value() {
        let value: usize = kani::any();
        kani::assume(value <= u32::MAX as usize);
        assert_eq!(u64::from(dword_const(value)), value as u64);
        kani::cover!(value == u32::MAX as usize);
    }

    #[kani::proof]
    fn negative_control_bytes_in_big_endian_order() {
        let value: u32 = kani::any();
        let mut bytes = [0_u8; size_of::<usize>()];
        for (slot, byte) in bytes.iter_mut().zip(value.to_be_bytes()) {
            *slot = byte;
        }
        assert!(
            usize::from_le_bytes(bytes) as u64 == u64::from(value),
            "NEGATIVE_CONVERSION_CONTROL"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_keep_their_values() {
        assert_eq!(widen(0), 0);
        assert_eq!(widen(0x0102_0304), 0x0102_0304);
        assert_eq!(widen(u32::MAX), 0xFFFF_FFFF);
        assert_eq!(dword_const(0), 0);
        assert_eq!(dword_const(0x0102_0304), 0x0102_0304);
        assert_eq!(dword_const(widen(u32::MAX)), u32::MAX);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    #[should_panic(expected = "size does not fit a DWORD")]
    fn a_size_past_a_dword_panics() {
        dword_const(widen(u32::MAX) + 1);
    }
}
