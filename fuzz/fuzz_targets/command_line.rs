//! Encodes a program name and arguments the fuzzer chooses, and requires both C runtime parser models to read every one back.
//!
//! It compiles `src/domain/command_line_rules.rs` itself, so it fuzzes the encoder the crate runs.
//! NUL ends a command line, so it separates the texts the input holds, and the first text is the program name.
//! Validation keeps a double quote out of a program name, so an input with one tests nothing.

#![no_main]

#[path = "../../src/domain/command_line_rules.rs"]
mod command_line_rules;

use command_line_rules::parsers::{split, Dialect};
use command_line_rules::{append_argument, append_program, NUL, QUOTE, SPACE};
use libfuzzer_sys::fuzz_target;

/// The part of the `UnitSink` of `src/domain/mod.rs` that the encoder calls.
trait UnitSink {
    fn put(&mut self, unit: u16);
}

impl UnitSink for Vec<u16> {
    fn put(&mut self, unit: u16) {
        self.push(unit);
    }
}

/// The longest command line `CreateProcessW` accepts, its terminator included, which sizes the models' storage.
const LINE: usize = 32_767;

fuzz_target!(|data: &[u8]| {
    let units: Vec<u16> = data
        .chunks_exact(size_of::<u16>())
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let mut texts = units.split(|unit| *unit == NUL);
    let Some(program) = texts.next() else {
        return;
    };
    if program.contains(&QUOTE) {
        return;
    }
    let arguments: Vec<&[u16]> = texts.collect();

    let mut line = Vec::new();
    append_program(&mut line, program);
    for argument in &arguments {
        line.put(SPACE);
        append_argument(&mut line, argument);
    }

    for dialect in [Dialect::Ucrt, Dialect::Msvcrt] {
        let Some(read) = split::<LINE>(&line, dialect) else {
            return;
        };
        let expected = std::iter::once(program).chain(arguments.iter().copied());
        assert_eq!(read.len(), arguments.len() + 1, "{dialect:?} split {line:?}");
        for (index, text) in expected.enumerate() {
            assert_eq!(read.get(index), Some(text), "{dialect:?} split {line:?}");
        }
    }
});
