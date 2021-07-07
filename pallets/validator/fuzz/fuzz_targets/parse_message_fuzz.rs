#![no_main]
use libfuzzer_sys::fuzz_target;
use pallet_oracle::oracle::{parse_message};

fuzz_target!(|data: &[u8]| {
    parse_message(data);
});
