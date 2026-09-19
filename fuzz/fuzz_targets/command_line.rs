#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../src/core_logic.rs"]
mod core_logic;

fuzz_target!(|data: &[u8]| {
    let units: Vec<u16> = data
        .chunks(2)
        .take(1_024)
        .map(|bytes| match bytes {
            [low, high] => u16::from_le_bytes([*low, *high]),
            [low] => u16::from(*low),
            _ => 0,
        })
        .collect();
    let _ = core_logic::quoted_argument_len(&units);
    if let Ok(mut line) = core_logic::CommandLine::new(&units) {
        let _ = line.push_regular(&units);
        let _ = line.push_raw(&units);
        let _ = line.finish();
    }
});
