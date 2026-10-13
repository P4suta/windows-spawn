//! Builds an environment block from variables the fuzzer chooses, and requires the block to end where `scan` finds its end and to read back as those variables, in order.
//!
//! It compiles `src/domain/environment_rules.rs` itself, so it fuzzes the builder the crate runs.
//! NUL ends an entry, so it separates the texts the input holds, which alternate between a key and its value.
//! Validation keeps a key nonempty and keeps `=` out of it after its first unit, where a hidden key such as `=C:` holds one, so other keys test nothing.

#![no_main]

#[path = "../../src/domain/environment_rules.rs"]
mod environment_rules;

use environment_rules::{append_entry, entries, finish, scan, NUL, SEPARATOR};
use libfuzzer_sys::fuzz_target;

/// The `UnitSink` of `src/domain/mod.rs`, which the builder writes into.
trait UnitSink {
    fn put(&mut self, unit: u16);

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

/// The texts of one variable: its key and its value.
const KEY_AND_VALUE: usize = 2;

fuzz_target!(|data: &[u8]| {
    let units: Vec<u16> = data
        .chunks_exact(size_of::<u16>())
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let texts: Vec<&[u16]> = units.split(|unit| *unit == NUL).collect();
    let variables: Vec<(&[u16], &[u16])> = texts
        .chunks_exact(KEY_AND_VALUE)
        .map(|pair| (pair[0], pair[1]))
        .filter(|(key, _)| key.split_first().is_some_and(|(_, rest)| !rest.contains(&SEPARATOR)))
        .collect();

    let mut block = Vec::new();
    for (key, value) in &variables {
        append_entry(&mut block, key, value);
    }
    finish(&mut block);

    let end = scan(&block).expect("a built block ends");
    let last = block.len() - 1;
    assert_eq!(end, if variables.is_empty() { 0 } else { last });
    let read: Vec<(&[u16], &[u16])> = entries(&block[..end]).collect();
    assert_eq!(read, variables);
});
