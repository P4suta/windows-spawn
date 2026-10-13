//! How a program and its arguments become one command line that the Windows parsers split back into them.
//!
//! The C runtime and `CommandLineToArgvW` read a command line by the same rules for the arguments that follow the program name:
//! whitespace outside double quotes separates arguments, a double quote opens or closes a quoted region,
//! `2n` backslashes before a double quote stand for `n` backslashes, and `2n + 1` backslashes before one stand for `n` backslashes and a literal double quote.
//! The program name ends at the next double quote when it starts with one.

use super::UnitSink;

/// The double quote, which opens and closes a quoted region.
pub(crate) const QUOTE: u16 = 0x22;
/// The backslash, which escapes a double quote.
pub(crate) const BACKSLASH: u16 = 0x5c;
/// The space, which separates arguments outside a quoted region.
pub(crate) const SPACE: u16 = 0x20;
/// The horizontal tab, which also separates arguments outside a quoted region.
pub(crate) const TAB: u16 = 0x09;
/// The vertical bar, which a command shell reads as a pipe outside a quoted region.
pub(crate) const VERTICAL_BAR: u16 = 0x7c;
/// The NUL unit, which ends a command line.
pub(crate) const NUL: u16 = 0;

/// Returns true if `argument` goes inside double quotes: when it is empty, or holds a space, tab, double quote, or vertical bar.
#[must_use]
pub(crate) fn needs_quotes(argument: &[u16]) -> bool {
    argument.is_empty()
        || argument
            .iter()
            .any(|unit| matches!(*unit, SPACE | TAB | QUOTE | VERTICAL_BAR))
}

/// Appends the program name inside double quotes, which a program name never contains.
pub(crate) fn append_program(line: &mut impl UnitSink, program: &[u16]) {
    line.put(QUOTE);
    for &unit in program {
        line.put(unit);
    }
    line.put(QUOTE);
}

/// Appends `argument` so the Windows parsers read it back as exactly one argument.
///
/// A double quote gets one more backslash than the backslashes before it, which double.
/// Inside double quotes, the backslashes before the closing quote double as well.
pub(crate) fn append_argument(line: &mut impl UnitSink, argument: &[u16]) {
    let quoted = needs_quotes(argument);
    if quoted {
        line.put(QUOTE);
    }
    let mut backslashes = 0_usize;
    for &unit in argument {
        if unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
        } else {
            if unit == QUOTE {
                put_backslashes(line, backslashes.saturating_add(1));
            }
            backslashes = 0;
        }
        line.put(unit);
    }
    if quoted {
        put_backslashes(line, backslashes);
        line.put(QUOTE);
    }
}

fn put_backslashes(line: &mut impl UnitSink, count: usize) {
    for _ in 0..count {
        line.put(BACKSLASH);
    }
}

/// Executable models of the Windows command-line parsers, for proofs, tests, and the fuzzer.
#[cfg(any(test, kani, fuzzing))]
pub(crate) mod parsers {
    use super::{BACKSLASH, NUL, QUOTE, SPACE, TAB};

    /// The backslashes before a double quote that stand for one backslash.
    const BACKSLASH_PAIR: usize = 2;

    /// How a parser reads two double quotes inside a quoted region.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Dialect {
        /// The C runtime since Visual C++ 2008 and the Universal CRT: a literal double quote, and the region stays open.
        Ucrt,
        /// The C runtime before Visual C++ 2008: a literal double quote, and the region closes.
        Msvcrt,
    }

    /// Where the parser stands.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        /// Reading the program name, where double quotes toggle a quoted region and backslashes stay literal.
        Program,
        /// Skipping the whitespace before an argument.
        Between,
        /// Reading an argument.
        Argument,
    }

    /// What a parser read: the units of every argument back to back, and where each argument ends.
    ///
    /// The storage is fixed, because no argument holds more units than the line.
    #[derive(Debug)]
    pub(crate) struct Arguments<const N: usize> {
        units: [u16; N],
        length: usize,
        ends: [usize; N],
        count: usize,
    }

    impl<const N: usize> Arguments<N> {
        fn put(&mut self, unit: u16) {
            if let Some(slot) = self.units.get_mut(self.length) {
                *slot = unit;
                self.length = self.length.saturating_add(1);
            }
        }

        fn end(&mut self) {
            if let Some(slot) = self.ends.get_mut(self.count) {
                *slot = self.length;
                self.count = self.count.saturating_add(1);
            }
        }

        /// The number of arguments, the program name included.
        pub(crate) const fn len(&self) -> usize {
            self.count
        }

        /// The argument at `index`.
        pub(crate) fn get(&self, index: usize) -> Option<&[u16]> {
            if index >= self.count {
                return None;
            }
            let end = *self.ends.get(index)?;
            let start = match index.checked_sub(1) {
                Some(previous) => *self.ends.get(previous)?,
                None => 0,
            };
            self.units.get(start..end)
        }

        /// Every argument as its own vector.
        #[cfg(test)]
        pub(crate) fn to_vecs(&self) -> Vec<Vec<u16>> {
            (0..self.len())
                .filter_map(|index| self.get(index).map(<[u16]>::to_vec))
                .collect()
        }
    }

    /// Splits `line`, of at most `N` units, into the program name and the arguments, as the C runtime of `dialect` does.
    ///
    /// The parser reads each unit once: a backslash goes to the argument at once, and a double quote after `n` of them takes back the `n - n / 2` it no longer stands for.
    pub(crate) fn split<const N: usize>(line: &[u16], dialect: Dialect) -> Option<Arguments<N>> {
        if line.len() > N {
            return None;
        }
        let mut arguments = Arguments {
            units: [0; N],
            length: 0,
            ends: [0; N],
            count: 0,
        };
        let mut phase = Phase::Program;
        let mut quoted = false;
        let mut backslashes = 0_usize;
        let mut position = 0_usize;
        while let Some(&unit) = line.get(position) {
            if unit == NUL {
                break;
            }
            position = position.saturating_add(1);
            let blank = matches!(unit, SPACE | TAB);
            match phase {
                Phase::Program => {
                    if unit == QUOTE {
                        quoted = !quoted;
                    } else if blank && !quoted {
                        arguments.end();
                        phase = Phase::Between;
                        quoted = false;
                    } else {
                        arguments.put(unit);
                    }
                    continue;
                }
                Phase::Between if blank => continue,
                Phase::Between | Phase::Argument => phase = Phase::Argument,
            }
            if unit == BACKSLASH {
                arguments.put(unit);
                backslashes = backslashes.saturating_add(1);
                continue;
            }
            if unit == QUOTE {
                let escaped = backslashes % BACKSLASH_PAIR == 1;
                let dropped = backslashes.saturating_sub(backslashes / BACKSLASH_PAIR);
                arguments.length = arguments.length.saturating_sub(dropped);
                backslashes = 0;
                if escaped {
                    arguments.put(QUOTE);
                } else if quoted && line.get(position) == Some(&QUOTE) {
                    arguments.put(QUOTE);
                    position = position.saturating_add(1);
                    if dialect == Dialect::Msvcrt {
                        quoted = false;
                    }
                } else {
                    quoted = !quoted;
                }
                continue;
            }
            backslashes = 0;
            if blank && !quoted {
                arguments.end();
                phase = Phase::Between;
            } else {
                arguments.put(unit);
            }
        }
        if phase != Phase::Between {
            arguments.end();
        }
        Some(arguments)
    }
}

#[cfg(kani)]
mod proofs {
    use super::parsers::{split, Dialect};
    use super::*;
    use crate::domain::Buffer;

    /// The longest argument the round trip covers: Lean's `split_encodeLine` covers every length.
    const ARGUMENT: usize = 3;
    const PAIR: usize = 1;
    const PROGRAM: usize = 2;
    /// Room for any line the harnesses build.
    const LINE: usize = 16;

    fn dialect() -> Dialect {
        if kani::any() {
            Dialect::Ucrt
        } else {
            Dialect::Msvcrt
        }
    }

    /// Compares unit by unit, which keeps formatting and byte comparison out of the proof.
    fn same(left: Option<&[u16]>, right: &[u16]) -> bool {
        let Some(left) = left else {
            return false;
        };
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

    /// A symbolic text of at most `N` units, none of them NUL.
    fn text<const N: usize>() -> Buffer<N> {
        let units: [u16; N] = kani::any();
        let length: usize = kani::any();
        kani::assume(length <= N);
        let mut text = Buffer::new();
        for (index, unit) in units.into_iter().enumerate() {
            if index < length {
                kani::assume(unit != NUL);
                text.put(unit);
            }
        }
        text
    }

    #[kani::proof]
    #[kani::unwind(14)]
    fn one_argument_decodes_to_itself() {
        let program = text::<PROGRAM>();
        let program = program.as_slice().unwrap();
        kani::assume(!program.is_empty() && !program.contains(&QUOTE));
        let argument = text::<ARGUMENT>();
        let argument = argument.as_slice().unwrap();
        let dialect = dialect();
        let mut line = Buffer::<LINE>::new();
        append_program(&mut line, program);
        line.put(SPACE);
        append_argument(&mut line, argument);
        let decoded = split::<LINE>(line.as_slice().unwrap(), dialect).unwrap();
        assert!(decoded.len() == 2);
        assert!(same(decoded.get(0), program));
        assert!(same(decoded.get(1), argument));
        kani::cover!(argument.is_empty());
        kani::cover!(argument.contains(&QUOTE) && argument.contains(&BACKSLASH));
        kani::cover!(argument.last() == Some(&BACKSLASH) && needs_quotes(argument));
        kani::cover!(dialect == Dialect::Msvcrt && argument == [QUOTE, QUOTE]);
    }

    #[kani::proof]
    #[kani::unwind(14)]
    fn adjacent_arguments_stay_apart() {
        let first = text::<PAIR>();
        let first = first.as_slice().unwrap();
        let second = text::<PAIR>();
        let second = second.as_slice().unwrap();
        let program = [u16::from(b'p')];
        let mut line = Buffer::<LINE>::new();
        append_program(&mut line, &program);
        line.put(SPACE);
        append_argument(&mut line, first);
        line.put(TAB);
        append_argument(&mut line, second);
        let decoded = split::<LINE>(line.as_slice().unwrap(), dialect()).unwrap();
        assert!(decoded.len() == 3);
        assert!(same(decoded.get(0), &program));
        assert!(same(decoded.get(1), first));
        assert!(same(decoded.get(2), second));
        kani::cover!(first.is_empty() && second.is_empty());
        kani::cover!(first == [BACKSLASH] && second == [QUOTE]);
    }

    #[kani::proof]
    #[kani::unwind(6)]
    fn plain_arguments_stay_verbatim() {
        let argument = text::<ARGUMENT>();
        let argument = argument.as_slice().unwrap();
        let mut line = Buffer::<LINE>::new();
        append_argument(&mut line, argument);
        assert!(needs_quotes(argument) == !same(line.as_slice(), argument));
        kani::cover!(!needs_quotes(argument) && argument.contains(&BACKSLASH));
        kani::cover!(needs_quotes(argument));
    }

    /// Drops the doubling of backslashes before the closing quote, which the proofs must catch.
    fn append_argument_without_closing_escape(line: &mut impl UnitSink, argument: &[u16]) {
        let quoted = needs_quotes(argument);
        if quoted {
            line.put(QUOTE);
        }
        let mut backslashes = 0_usize;
        for &unit in argument {
            if unit == BACKSLASH {
                backslashes = backslashes.saturating_add(1);
            } else {
                if unit == QUOTE {
                    put_backslashes(line, backslashes.saturating_add(1));
                }
                backslashes = 0;
            }
            line.put(unit);
        }
        if quoted {
            line.put(QUOTE);
        }
    }

    /// Unwound as far as `one_argument_decodes_to_itself`, so the encoder's defect is the only failure.
    #[kani::proof]
    #[kani::unwind(14)]
    fn negative_control_unescaped_closing_quote() {
        let argument = text::<ARGUMENT>();
        let argument = argument.as_slice().unwrap();
        let mut line = Buffer::<LINE>::new();
        append_program(&mut line, &[u16::from(b'p')]);
        line.put(SPACE);
        append_argument_without_closing_escape(&mut line, argument);
        let decoded = split::<LINE>(line.as_slice().unwrap(), Dialect::Ucrt).unwrap();
        assert!(
            decoded.len() == 2 && same(decoded.get(1), argument),
            "NEGATIVE_COMMAND_LINE_CONTROL"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::parsers::{split, Dialect};
    use super::*;

    const LINE: usize = 64;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn encode(arguments: &[&str]) -> Vec<u16> {
        let mut line = Vec::new();
        append_program(&mut line, &wide("program.exe"));
        for argument in arguments {
            line.push(SPACE);
            append_argument(&mut line, &wide(argument));
        }
        line
    }

    fn arguments(line: &[u16], dialect: Dialect) -> Vec<Vec<u16>> {
        split::<LINE>(line, dialect).unwrap().to_vecs()
    }

    #[test]
    fn encodes_the_documented_examples() {
        assert_eq!(
            String::from_utf16_lossy(&encode(&["a b", "a\"b", "", r"C:\path with spaces\"])),
            r#""program.exe" "a b" "a\"b" "" "C:\path with spaces\\""#
        );
        assert_eq!(
            String::from_utf16_lossy(&encode(&[r"\\server\share", "x|y", "\t"])),
            "\"program.exe\" \\\\server\\share \"x|y\" \"\t\""
        );
    }

    #[test]
    fn every_short_argument_over_the_special_units_round_trips() {
        let alphabet = [u16::from(b'a'), SPACE, TAB, QUOTE, BACKSLASH, VERTICAL_BAR];
        let mut all: Vec<Vec<u16>> = vec![Vec::new()];
        let mut frontier = all.clone();
        for _ in 0..4 {
            let mut next = Vec::new();
            for prefix in &frontier {
                for unit in alphabet {
                    let mut argument = prefix.clone();
                    argument.push(unit);
                    next.push(argument);
                }
            }
            all.extend(next.iter().cloned());
            frontier = next;
        }
        for dialect in [Dialect::Ucrt, Dialect::Msvcrt] {
            for argument in &all {
                let mut line = Vec::new();
                append_program(&mut line, &wide("p"));
                line.push(SPACE);
                append_argument(&mut line, argument);
                line.push(SPACE);
                append_argument(&mut line, argument);
                assert_eq!(
                    arguments(&line, dialect),
                    vec![wide("p"), argument.clone(), argument.clone()],
                    "{dialect:?} {argument:?}"
                );
            }
        }
    }

    /// Reads hexadecimal units separated by spaces.
    #[cfg(windows_spawn_vectors)]
    fn units(hex: &str) -> Vec<u16> {
        hex.split(' ')
            .filter(|unit| !unit.is_empty())
            .map(|unit| u16::from_str_radix(unit, 16).unwrap())
            .collect()
    }

    /// Ties `append_argument` to the proved `encodeArg` of `proofs/lean`, row for row.
    #[cfg(windows_spawn_vectors)]
    #[test]
    fn the_encoder_writes_what_the_lean_model_writes() {
        let vectors = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proofs/lean/vectors/command_line_encoding.tsv"
        ));
        let mut rows = 0_usize;
        for row in vectors.lines() {
            let (argument, encoded) = row.split_once('\t').unwrap();
            let mut written = Vec::new();
            append_argument(&mut written, &units(argument));
            assert_eq!(written, units(encoded), "{row}");
            rows += 1;
        }
        assert_eq!(
            rows, 1555,
            "the vectors cover every argument up to four units"
        );
    }

    /// Ties the parser models of the proofs to the proved `split` of `proofs/lean`, row for row.
    #[cfg(windows_spawn_vectors)]
    #[test]
    fn the_parser_models_agree_with_the_lean_models() {
        let vectors = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proofs/lean/vectors/command_line_parsing.tsv"
        ));
        let mut rows = 0_usize;
        for row in vectors.lines() {
            let mut columns = row.split('\t');
            let line = units(columns.next().unwrap());
            for dialect in [Dialect::Ucrt, Dialect::Msvcrt] {
                let expected: Vec<Vec<u16>> =
                    columns.next().unwrap().split('|').map(units).collect();
                assert_eq!(arguments(&line, dialect), expected, "{dialect:?} {row}");
            }
            rows += 1;
        }
        assert_eq!(rows, 5461, "the vectors cover every line up to six units");
    }

    #[test]
    fn the_dialects_differ_only_on_doubled_quotes_inside_quotes() {
        let line = wide(r#"p "a""b" c"#);
        assert_eq!(
            arguments(&line, Dialect::Ucrt),
            vec![wide("p"), wide(r#"a"b"#), wide("c")]
        );
        assert_eq!(
            arguments(&line, Dialect::Msvcrt),
            vec![wide("p"), wide(r#"a"b c"#)]
        );
        assert!(split::<2>(&wide("abc"), Dialect::Ucrt).is_none());
    }
}
