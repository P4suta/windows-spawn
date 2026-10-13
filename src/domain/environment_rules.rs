//! The layout of a Unicode environment block: `key=value` entries that each end with NUL, and one more NUL after the last.

use super::UnitSink;

/// The NUL unit that ends each entry and the block.
pub(crate) const NUL: u16 = 0;
/// The `=` that separates a key from its value.
pub(crate) const SEPARATOR: u16 = 0x3d;

/// Splits `key=value` at the first `=` after the first unit, so the hidden `=C:` entries keep their key.
#[must_use]
pub(crate) fn split_entry(entry: &[u16]) -> Option<(&[u16], &[u16])> {
    let (separator, _) = entry
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, unit)| **unit == SEPARATOR)?;
    let (key, rest) = entry.split_at(separator);
    rest.split_first().map(|(_, value)| (key, value))
}

/// Returns true if `unit` ends the block: a NUL at the start of the block or right after another NUL.
#[must_use]
pub(crate) const fn ends_block(previous: Option<u16>, unit: u16) -> bool {
    unit == NUL && matches!(previous, None | Some(NUL))
}

/// The `key=value` entries of a block that stops before its final NUL; an entry without a separator yields nothing.
pub(crate) fn entries(block: &[u16]) -> impl Iterator<Item = (&[u16], &[u16])> {
    block.split(|unit| *unit == NUL).filter_map(split_entry)
}

/// Appends one entry and its NUL.
pub(crate) fn append_entry(block: &mut impl UnitSink, key: &[u16], value: &[u16]) {
    for &unit in key {
        block.put(unit);
    }
    block.put(SEPARATOR);
    for &unit in value {
        block.put(unit);
    }
    block.put(NUL);
}

/// Ends a block with its final NUL, after the empty entry an empty block needs.
pub(crate) fn finish(block: &mut impl UnitSink) {
    if block.is_empty() {
        block.put(NUL);
    }
    block.put(NUL);
}

/// Returns how many units of `block` precede the NUL that ends it, reading no unit past that NUL.
#[cfg(any(test, kani, fuzzing))]
pub(crate) fn scan(block: &[u16]) -> Option<usize> {
    let mut previous = None;
    for (index, unit) in block.iter().enumerate() {
        if ends_block(previous, *unit) {
            return Some(index);
        }
        previous = Some(*unit);
    }
    None
}

#[cfg(kani)]
mod proofs {
    //! The round trip of a whole block composes three proofs, because one harness over two parsed entries outgrows CBMC:
    //! a built block lays out each entry as `key=value` and its NUL before the terminator `scan` finds;
    //! `entries` reads each text between two NULs with `split_entry`;
    //! and `split_entry` splits at the first separator after the key's first unit, which a key never holds.
    //! `parsed_back_blocks_of_one_entry` checks the composition end to end for blocks of at most one entry,
    //! and the unit test `every_small_block_parses_back_to_its_entries` checks blocks of two over a small alphabet.

    use super::*;
    use crate::domain::Buffer;

    const KEY: usize = 2;
    const VALUE: usize = 1;
    const ENTRY: usize = KEY + 1 + VALUE;
    /// Two entries of the longest key and value, each with its NUL, and the terminator.
    const BLOCK: usize = 2 * (ENTRY + 1) + 1;
    /// The longest text between two NULs in the iteration proof, which already holds texts with and without an entry.
    const SEGMENT: usize = 2;

    /// Compares unit by unit, which keeps formatting and byte comparison out of the proof.
    fn same(left: &[u16], right: &[u16]) -> bool {
        if left.len() != right.len() {
            return false;
        }
        let mut index = 0;
        while index < right.len() {
            if left[index] != right[index] {
                return false;
            }
            index += 1;
        }
        true
    }

    fn text<const N: usize>(minimum: usize) -> Buffer<N> {
        let units: [u16; N] = kani::any();
        let length: usize = kani::any();
        kani::assume(minimum <= length && length <= N);
        let mut text = Buffer::new();
        for (index, unit) in units.into_iter().enumerate() {
            if index < length {
                kani::assume(unit != NUL);
                text.put(unit);
            }
        }
        text
    }

    fn key() -> Buffer<KEY> {
        let key = text::<KEY>(1);
        kani::assume(!key.as_slice().unwrap()[1..].contains(&SEPARATOR));
        key
    }

    /// The loops over the entries run a constant number of times, so CBMC unrolls each one only that often.
    #[kani::proof]
    #[kani::unwind(12)]
    fn built_blocks_lay_out_each_entry_before_the_terminator() {
        let count: usize = kani::any();
        kani::assume(count <= 2);
        let keys = [key(), key()];
        let values = [text::<VALUE>(0), text::<VALUE>(0)];
        let mut block = Buffer::<BLOCK>::new();
        for index in 0..2 {
            if index < count {
                append_entry(
                    &mut block,
                    keys[index].as_slice().unwrap(),
                    values[index].as_slice().unwrap(),
                );
            }
        }
        finish(&mut block);
        let block = block.as_slice().unwrap();
        let mut offset = 0;
        for index in 0..2 {
            if index < count {
                let key = keys[index].as_slice().unwrap();
                let value = values[index].as_slice().unwrap();
                let separator = offset + key.len();
                let end = separator + 1 + value.len();
                assert!(same(&block[offset..separator], key));
                assert!(block[separator] == SEPARATOR);
                assert!(same(&block[separator + 1..end], value));
                assert!(block[end] == NUL);
                offset = end + 1;
            }
        }
        let length = scan(block).expect("a finished block ends");
        assert!(length == offset);
        assert!(block.len() == if count == 0 { 2 } else { offset + 1 });
        assert!(block[block.len() - 1] == NUL);
        kani::cover!(count == 0);
        kani::cover!(count == 2 && keys[0].as_slice().unwrap() == [SEPARATOR]);
    }

    #[kani::proof]
    #[kani::unwind(7)]
    fn entries_read_each_text_between_nuls() {
        let first = text::<SEGMENT>(0);
        let second = text::<SEGMENT>(0);
        let first = first.as_slice().unwrap();
        let second = second.as_slice().unwrap();
        let mut block = Buffer::<{ 2 * (SEGMENT + 1) }>::new();
        for &unit in first {
            block.put(unit);
        }
        block.put(NUL);
        for &unit in second {
            block.put(unit);
        }
        block.put(NUL);
        let mut parsed = entries(block.as_slice().unwrap());
        for text in [first, second] {
            if let Some((key, value)) = split_entry(text) {
                let (parsed_key, parsed_value) = parsed.next().expect("every entry parses");
                assert!(same(parsed_key, key));
                assert!(same(parsed_value, value));
            }
        }
        assert!(parsed.next().is_none());
        kani::cover!(split_entry(first).is_none() && split_entry(second).is_some());
        kani::cover!(split_entry(first).is_some() && split_entry(second).is_some());
    }

    #[kani::proof]
    #[kani::unwind(7)]
    fn parsed_back_blocks_of_one_entry() {
        let present: bool = kani::any();
        let key = key();
        let value = text::<VALUE>(0);
        let key = key.as_slice().unwrap();
        let value = value.as_slice().unwrap();
        let mut block = Buffer::<{ ENTRY + 2 }>::new();
        if present {
            append_entry(&mut block, key, value);
        }
        finish(&mut block);
        let block = block.as_slice().unwrap();
        let length = scan(block).expect("a finished block ends");
        let mut parsed = entries(&block[..length]);
        if present {
            let (parsed_key, parsed_value) = parsed.next().expect("the entry parses");
            assert!(same(parsed_key, key));
            assert!(same(parsed_value, value));
        }
        assert!(parsed.next().is_none());
        kani::cover!(present && key.len() == KEY && key[0] == SEPARATOR && value == [SEPARATOR]);
    }

    #[kani::proof]
    #[kani::unwind(6)]
    fn entries_split_at_the_first_separator_after_the_key_start() {
        let entry = text::<ENTRY>(0);
        let entry = entry.as_slice().unwrap();
        match split_entry(entry) {
            Some((key, value)) => {
                assert!(!key.is_empty() && !key[1..].contains(&SEPARATOR));
                assert!(key.len() + 1 + value.len() == entry.len());
                assert!(same(&entry[..key.len()], key));
                assert!(entry[key.len()] == SEPARATOR);
                assert!(same(&entry[key.len() + 1..], value));
            }
            None => {
                assert!(entry.len() < 2 || !entry[1..].contains(&SEPARATOR));
            }
        }
        kani::cover!(entry.first() == Some(&SEPARATOR) && split_entry(entry).is_some());
    }

    #[kani::proof]
    #[kani::unwind(6)]
    fn negative_control_first_separator_anywhere() {
        let entry = text::<ENTRY>(1);
        let entry = entry.as_slice().unwrap();
        let first = entry.iter().position(|unit| *unit == SEPARATOR);
        let key_length = split_entry(entry).map(|(key, _)| key.len());
        assert!(key_length == first, "NEGATIVE_ENVIRONMENT_CONTROL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn entries_split_at_the_first_separator_after_the_key() {
        let split = |text: &str| {
            let entry = wide(text);
            split_entry(&entry).map(|(key, value)| (key.to_vec(), value.to_vec()))
        };
        assert_eq!(split("KEY=a=b"), Some((wide("KEY"), wide("a=b"))));
        assert_eq!(split("=C:=C:\\work"), Some((wide("=C:"), wide("C:\\work"))));
        assert_eq!(split("EMPTY="), Some((wide("EMPTY"), Vec::new())));
        assert_eq!(split("NOSEPARATOR"), None);
        assert_eq!(split("="), None);
    }

    #[test]
    fn blocks_end_with_a_doubled_nul() {
        let mut empty = Vec::new();
        finish(&mut empty);
        assert_eq!(empty, [NUL, NUL]);
        assert_eq!(scan(&empty), Some(0));

        let mut block = Vec::new();
        append_entry(&mut block, &wide("A"), &wide("1"));
        append_entry(&mut block, &wide("=C:"), &wide("C:\\"));
        finish(&mut block);
        assert_eq!(String::from_utf16_lossy(&block), "A=1\0=C:=C:\\\0\0");
        let length = scan(&block).unwrap();
        assert_eq!(length + 1, block.len());
        let parsed: Vec<_> = entries(&block[..length]).collect();
        assert_eq!(
            parsed,
            [
                (&wide("A")[..], &wide("1")[..]),
                (&wide("=C:")[..], &wide("C:\\")[..])
            ]
        );
        assert_eq!(scan(&wide("A=1")), None);
        assert!(ends_block(None, NUL) && ends_block(Some(NUL), NUL));
        assert!(!ends_block(Some(SEPARATOR), NUL) && !ends_block(None, SEPARATOR));
    }

    #[test]
    fn every_small_block_parses_back_to_its_entries() {
        let alphabet = [SEPARATOR, u16::from(b'a'), 0x00e9];
        let mut texts: Vec<Vec<u16>> = vec![Vec::new()];
        for first in alphabet {
            texts.push(vec![first]);
            for second in alphabet {
                texts.push(vec![first, second]);
            }
        }
        let keys = texts
            .iter()
            .filter(|text| !text.is_empty() && !text[1..].contains(&SEPARATOR));
        let pairs: Vec<(&[u16], &[u16])> = keys
            .flat_map(|key| texts.iter().map(move |value| (&key[..], &value[..])))
            .collect();
        let mut blocks: Vec<Vec<(&[u16], &[u16])>> = vec![Vec::new()];
        blocks.extend(pairs.iter().map(|pair| vec![*pair]));
        for first in &pairs {
            blocks.extend(pairs.iter().map(|second| vec![*first, *second]));
        }
        for expected in &blocks {
            let mut block = Vec::new();
            for (key, value) in expected {
                append_entry(&mut block, key, value);
            }
            finish(&mut block);
            let length = scan(&block).unwrap();
            assert_eq!(entries(&block[..length]).collect::<Vec<_>>(), *expected);
        }
        assert_eq!(blocks.len(), 1 + 117 + 117 * 117);
    }
}
