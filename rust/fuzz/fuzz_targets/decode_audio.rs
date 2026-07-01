#![no_main]

//! Fuzz the untrusted audio-container parsers (WAV + MP3 detection/decoding).
//!
//! Run with a nightly toolchain and cargo-fuzz:
//!
//! ```text
//! cargo install cargo-fuzz
//! cd rust/fuzz
//! cargo +nightly fuzz run decode_audio
//! ```

use libfuzzer_sys::fuzz_target;

use fingerprint_core::decode_audio_bytes;

fuzz_target!(|data: &[u8]| {
    // Any Ok/Err is acceptable; a panic (or memory fault) is a finding.
    let _ = decode_audio_bytes(data);
});
