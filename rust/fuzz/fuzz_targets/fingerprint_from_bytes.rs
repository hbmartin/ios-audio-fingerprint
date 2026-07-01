#![no_main]

//! Fuzz the fingerprint deserializer, which parses an untrusted length-prefixed
//! binary payload.
//!
//! ```text
//! cd rust/fuzz
//! cargo +nightly fuzz run fingerprint_from_bytes
//! ```

use libfuzzer_sys::fuzz_target;

use fingerprint_core::fingerprint_from_bytes;

fuzz_target!(|data: &[u8]| {
    let _ = fingerprint_from_bytes(data);
});
