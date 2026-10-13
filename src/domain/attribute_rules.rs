//! The `PROC_THREAD_ATTRIBUTE_LIST` of a spawn: how many attributes it holds and what storage backs it.

use core::mem::size_of;

/// The number of attributes the list holds, one for each requested attribute; a spawn that requests none needs no list.
#[must_use]
pub(crate) const fn count(requested: &[bool]) -> u32 {
    let mut count = 0_u32;
    let mut rest = requested;
    while let [attribute, tail @ ..] = rest {
        if *attribute {
            count = count.saturating_add(1);
        }
        rest = tail;
    }
    count
}

/// The bytes in one word of storage, the alignment the list needs.
const WORD: usize = size_of::<usize>();

/// The most words one allocation holds, which keeps its size within `isize::MAX` bytes.
const MAXIMUM_WORDS: usize = (usize::MAX >> 1) / WORD;

/// What the null-list size probe of `InitializeProcThreadAttributeList` reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeProbe {
    /// The probe succeeded, which it never does for a null list.
    Succeeded,
    /// The probe failed with `ERROR_INSUFFICIENT_BUFFER` and wrote the size in bytes.
    Sized(usize),
    /// The probe failed some other way.
    Failed,
}

/// Why a probe yields no storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeFault {
    /// The probe succeeded against its contract.
    UnexpectedSuccess,
    /// The probe failed, or reported no size; the operating-system error explains it.
    Failed,
    /// The size exceeds what one allocation holds.
    TooLarge,
}

/// Classifies a size probe from whether it succeeded, whether its error was `ERROR_INSUFFICIENT_BUFFER`, and the size it wrote.
#[must_use]
pub(crate) const fn probe(succeeded: bool, insufficient_buffer: bool, bytes: usize) -> SizeProbe {
    if succeeded {
        SizeProbe::Succeeded
    } else if insufficient_buffer {
        SizeProbe::Sized(bytes)
    } else {
        SizeProbe::Failed
    }
}

/// The words of storage that back a list of the probed size.
///
/// # Errors
///
/// Returns why the probe gives no usable size.
pub(crate) const fn storage_words(probe: SizeProbe) -> Result<usize, SizeFault> {
    match probe {
        SizeProbe::Succeeded => Err(SizeFault::UnexpectedSuccess),
        SizeProbe::Failed | SizeProbe::Sized(0) => Err(SizeFault::Failed),
        SizeProbe::Sized(bytes) => {
            let words = bytes.div_ceil(WORD);
            if words > MAXIMUM_WORDS {
                Err(SizeFault::TooLarge)
            } else {
                Ok(words)
            }
        }
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn storage_covers_the_probed_size_within_one_allocation() {
        let bytes: usize = kani::any();
        match storage_words(SizeProbe::Sized(bytes)) {
            Ok(words) => {
                let size = words.checked_mul(WORD).expect("storage size fits a usize");
                assert!(size >= bytes && size - bytes < WORD);
                assert!(size <= isize::MAX as usize);
                assert!(bytes != 0);
            }
            Err(SizeFault::TooLarge) => {
                assert!(bytes > isize::MAX as usize - (WORD - 1));
            }
            Err(SizeFault::Failed) => {
                assert_eq!(bytes, 0);
            }
            Err(SizeFault::UnexpectedSuccess) => unreachable!(),
        }
        kani::cover!(bytes == isize::MAX as usize - (WORD - 1));
        kani::cover!(bytes == isize::MAX as usize);
        kani::cover!(bytes % WORD == 1);
    }

    #[kani::proof]
    fn only_a_sized_failure_yields_storage() {
        let succeeded: bool = kani::any();
        let insufficient_buffer: bool = kani::any();
        let bytes: usize = kani::any();
        match storage_words(probe(succeeded, insufficient_buffer, bytes)) {
            Ok(_) => {
                assert!(!succeeded && insufficient_buffer && bytes != 0);
            }
            Err(SizeFault::UnexpectedSuccess) => {
                assert!(succeeded);
            }
            Err(SizeFault::Failed) => {
                assert!(!succeeded && (!insufficient_buffer || bytes == 0));
            }
            Err(SizeFault::TooLarge) => {
                assert!(!succeeded && insufficient_buffer);
            }
        }
        kani::cover!(!succeeded && insufficient_buffer && bytes == 64);
    }

    #[kani::proof]
    #[kani::unwind(7)]
    fn count_matches_the_requested_attributes() {
        let requested: [bool; 5] = kani::any();
        let expected = requested.iter().filter(|attribute| **attribute).count();
        assert_eq!(count(&requested) as usize, expected);
        kani::cover!(count(&requested) == 5);
        kani::cover!(count(&requested) == 0);
    }

    #[kani::proof]
    fn negative_control_unbounded_storage() {
        let bytes: usize = kani::any();
        let words = bytes.div_ceil(WORD);
        assert!(
            words
                .checked_mul(WORD)
                .is_some_and(|size| size <= isize::MAX as usize),
            "NEGATIVE_ATTRIBUTE_CONTROL"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_results_classify_as_documented() {
        assert_eq!(storage_words(SizeProbe::Sized(48)), Ok(48 / WORD));
        assert_eq!(storage_words(SizeProbe::Sized(1)), Ok(1));
        assert_eq!(storage_words(SizeProbe::Sized(WORD + 1)), Ok(2));
        assert_eq!(storage_words(SizeProbe::Sized(0)), Err(SizeFault::Failed));
        assert_eq!(storage_words(SizeProbe::Failed), Err(SizeFault::Failed));
        assert_eq!(
            storage_words(SizeProbe::Succeeded),
            Err(SizeFault::UnexpectedSuccess)
        );
        assert_eq!(
            storage_words(SizeProbe::Sized(usize::MAX)),
            Err(SizeFault::TooLarge)
        );
        let largest = MAXIMUM_WORDS * WORD;
        assert_eq!(storage_words(SizeProbe::Sized(largest)), Ok(MAXIMUM_WORDS));
        assert_eq!(
            storage_words(SizeProbe::Sized(largest + 1)),
            Err(SizeFault::TooLarge)
        );
    }

    #[test]
    fn probes_classify_by_result_then_error() {
        assert_eq!(probe(true, true, 64), SizeProbe::Succeeded);
        assert_eq!(probe(true, false, 0), SizeProbe::Succeeded);
        assert_eq!(probe(false, true, 64), SizeProbe::Sized(64));
        assert_eq!(probe(false, false, 64), SizeProbe::Failed);
    }

    #[test]
    fn count_is_the_number_of_requested_attributes() {
        assert_eq!(count(&[false; 5]), 0);
        assert_eq!(count(&[false, true, false, true, false]), 2);
    }
}
