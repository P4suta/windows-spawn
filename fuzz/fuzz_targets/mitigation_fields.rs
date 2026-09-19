#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../src/core_logic.rs"]
mod core_logic;

fuzz_target!(|data: &[u8]| {
    let Some(word_bytes) = data.get(..8) else {
        return;
    };
    let Some(value_bytes) = data.get(8..16) else {
        return;
    };
    let Some(shift_byte) = data.get(16) else {
        return;
    };
    let mut word_array = [0_u8; 8];
    let mut value_array = [0_u8; 8];
    word_array.copy_from_slice(word_bytes);
    value_array.copy_from_slice(value_bytes);
    let word = u64::from_le_bytes(word_array);
    let value = u64::from_le_bytes(value_array);
    let shift = u32::from(*shift_byte % 64);
    let _ = core_logic::replace_one_bit_field(word, shift, value);
    let _ = core_logic::replace_two_bit_field(word, shift, value);
});
