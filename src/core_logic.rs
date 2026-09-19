use std::collections::TryReserveError;

const QUOTE: u16 = 0x22;
const BACKSLASH: u16 = 0x5c;
const SPACE: u16 = 0x20;
const TAB: u16 = 0x09;
const PIPE: u16 = 0x7c;

pub(crate) const MAX_COMMAND_LINE_UNITS: usize = 32_767;

#[derive(Debug)]
pub(crate) enum CommandLineError {
    TooLong,
    Allocation(TryReserveError),
}

pub(crate) struct CommandLine {
    units: Vec<u16>,
}

impl CommandLine {
    pub(crate) fn new(program: &[u16]) -> Result<Self, CommandLineError> {
        let mut value = Self { units: Vec::new() };
        let additional = program.len().saturating_add(2);
        value.reserve(additional)?;
        value.units.push(QUOTE);
        value.units.extend_from_slice(program);
        value.units.push(QUOTE);
        Ok(value)
    }

    pub(crate) fn push_regular(&mut self, argument: &[u16]) -> Result<(), CommandLineError> {
        let encoded = quoted_argument_len(argument);
        let additional = encoded.saturating_add(1);
        self.reserve(additional)?;
        self.units.push(SPACE);
        append_regular(&mut self.units, argument);
        Ok(())
    }

    pub(crate) fn push_raw(&mut self, argument: &[u16]) -> Result<(), CommandLineError> {
        let additional = argument.len().saturating_add(1);
        self.reserve(additional)?;
        self.units.push(SPACE);
        self.units.extend_from_slice(argument);
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Vec<u16> {
        self.units.push(0);
        self.units
    }

    fn reserve(&mut self, additional: usize) -> Result<(), CommandLineError> {
        self.reserve_with(additional, Vec::try_reserve)
    }

    fn reserve_with(
        &mut self,
        additional: usize,
        reserve: impl FnOnce(&mut Vec<u16>, usize) -> Result<(), TryReserveError>,
    ) -> Result<(), CommandLineError> {
        let with_content = self.units.len().saturating_add(additional);
        let with_terminator = with_content.saturating_add(1);
        if with_terminator > MAX_COMMAND_LINE_UNITS {
            return Err(CommandLineError::TooLong);
        }
        let reservation = additional.saturating_add(1);
        reserve(&mut self.units, reservation).map_err(CommandLineError::Allocation)
    }
}

pub(crate) fn quoted_argument_len(argument: &[u16]) -> usize {
    let quoted = needs_quotes(argument);
    let mut length = argument.len();
    if quoted {
        length = length.saturating_add(2);
    }
    let mut backslashes = 0_usize;
    for unit in argument {
        if *unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
        } else {
            if *unit == QUOTE {
                length = length.saturating_add(backslashes.saturating_add(1));
            }
            backslashes = 0;
        }
    }
    if quoted {
        length = length.saturating_add(backslashes);
    }
    length
}

fn append_regular(command: &mut Vec<u16>, argument: &[u16]) {
    let quoted = needs_quotes(argument);
    if quoted {
        command.push(QUOTE);
    }
    let mut backslashes = 0_usize;
    for unit in argument {
        if *unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
        } else {
            if *unit == QUOTE {
                command.extend(std::iter::repeat(BACKSLASH).take(backslashes.saturating_add(1)));
            }
            backslashes = 0;
        }
        command.push(*unit);
    }
    if quoted {
        command.extend(std::iter::repeat(BACKSLASH).take(backslashes));
        command.push(QUOTE);
    }
}

fn needs_quotes(argument: &[u16]) -> bool {
    argument.is_empty()
        || argument
            .iter()
            .any(|unit| matches!(*unit, SPACE | TAB | QUOTE | PIPE))
}

pub(crate) const fn replace_two_bit_field(word: u64, shift: u32, value: u64) -> u64 {
    let Some(mask) = 3_u64.checked_shl(shift) else {
        return word;
    };
    let encoded = value << shift;
    (word & !mask).wrapping_add(encoded & mask)
}

pub(crate) const fn replace_one_bit_field(word: u64, shift: u32, value: u64) -> u64 {
    let Some(mask) = 1_u64.checked_shl(shift) else {
        return word;
    };
    let encoded = value << shift;
    (word & !mask).wrapping_add(encoded & mask)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn quoting_length_matches_emission() {
        for argument in [
            &[][..],
            &[u16::from(b'a')],
            &[SPACE],
            &[BACKSLASH, QUOTE],
            &[BACKSLASH, BACKSLASH, SPACE, BACKSLASH],
        ] {
            let mut emitted = Vec::new();
            append_regular(&mut emitted, argument);
            assert_eq!(emitted.len(), quoted_argument_len(argument));
        }
    }

    #[test]
    fn builder_includes_separators_and_terminator() {
        let mut line = CommandLine::new(&[u16::from(b'p')]).unwrap();
        line.push_regular(&[SPACE]).unwrap();
        line.push_raw(&[u16::from(b'r')]).unwrap();
        let line = line.finish();
        assert_eq!(line.last(), Some(&0));
        assert!(line.len() <= MAX_COMMAND_LINE_UNITS);
    }

    #[test]
    fn builder_rejects_the_windows_limit_and_invalid_bit_shifts_are_noops() {
        let largest_program = vec![u16::from(b'x'); MAX_COMMAND_LINE_UNITS - 3];
        let largest_line = CommandLine::new(&largest_program).unwrap().finish();
        assert_eq!(largest_line.len(), MAX_COMMAND_LINE_UNITS);
        let oversized = vec![u16::from(b'x'); MAX_COMMAND_LINE_UNITS];
        assert!(matches!(
            CommandLine::new(&oversized),
            Err(CommandLineError::TooLong)
        ));
        let mut regular = CommandLine::new(&[u16::from(b'p')]).unwrap();
        assert!(matches!(
            regular.push_regular(&oversized),
            Err(CommandLineError::TooLong)
        ));
        let mut raw = CommandLine::new(&[u16::from(b'p')]).unwrap();
        assert!(matches!(
            raw.push_raw(&oversized),
            Err(CommandLineError::TooLong)
        ));
        assert_eq!(replace_two_bit_field(7, 64, 0), 7);
        assert_eq!(replace_two_bit_field(7, 65, 0), 7);
        assert_eq!(replace_one_bit_field(7, 64, 0), 7);
        assert_eq!(replace_one_bit_field(7, 65, 0), 7);

        let allocation = Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err();
        let mut line = CommandLine::new(&[u16::from(b'p')]).unwrap();
        match line.reserve_with(0, |_, _| Err(allocation)) {
            Err(CommandLineError::Allocation(source)) => {
                assert!(!source.to_string().is_empty());
            }
            _ => panic!("allocation failure expected"),
        }
    }

    #[test]
    fn field_replacement_preserves_other_bits() {
        let word = u64::MAX;
        assert_eq!(replace_two_bit_field(word, 12, 0), word & !(3_u64 << 12));
        assert_eq!(replace_one_bit_field(word, 7, 0), word & !(1_u64 << 7));
    }

    #[test]
    fn generated_quoting_and_fields_preserve_properties() {
        let cases = std::env::var("WINDOWS_SPAWN_PROPERTY_CASES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10_000);
        let mut state = 0x8f3c_2d19_7a65_b401_u64;
        for case in 0..cases {
            let length = usize::try_from(next(&mut state) % 65).unwrap_or_default();
            let mut argument = Vec::with_capacity(length);
            for _ in 0..length {
                let [low, high, ..] = next(&mut state).to_le_bytes();
                argument.push(u16::from_le_bytes([low, high]));
            }
            let mut emitted = Vec::new();
            append_regular(&mut emitted, &argument);
            assert_eq!(emitted.len(), quoted_argument_len(&argument), "{case}");

            let word = next(&mut state);
            let value = next(&mut state);
            let shift = u32::try_from((next(&mut state) % 31) * 2).unwrap_or_default();
            let mask = 3_u64 << shift;
            let replaced = replace_two_bit_field(word, shift, value);
            assert_eq!(replaced & !mask, word & !mask, "{case}");
            assert_eq!(replaced & mask, (value << shift) & mask, "{case}");
        }
    }

    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state
    }
}

#[cfg(kani)]
mod proofs {
    use super::{quoted_argument_len, replace_one_bit_field, replace_two_bit_field};

    #[kani::proof]
    fn quoting_length_is_bounded() {
        let argument: [u16; 16] = kani::any();
        let length = quoted_argument_len(&argument);
        assert!(length >= argument.len());
        assert!(length <= argument.len() * 2 + 2);
    }

    #[kani::proof]
    fn two_bit_replacement_changes_only_its_field() {
        let word: u64 = kani::any();
        let value: u64 = kani::any();
        let result = replace_two_bit_field(word, 36, value);
        let mask = 3_u64 << 36;
        assert_eq!(result & !mask, word & !mask);
        assert_eq!(result & mask, (value << 36) & mask);
    }

    #[kani::proof]
    fn one_bit_replacement_changes_only_its_field() {
        let word: u64 = kani::any();
        let value: u64 = kani::any();
        let result = replace_one_bit_field(word, 2, value);
        let mask = 1_u64 << 2;
        assert_eq!(result & !mask, word & !mask);
        assert_eq!(result & mask, (value << 2) & mask);
    }
}
