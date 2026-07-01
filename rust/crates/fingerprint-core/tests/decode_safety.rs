//! Deterministic decode-safety smoke test.
//!
//! The WAV and MP3 paths parse fully untrusted bytes. This test throws a large,
//! reproducible mix of malformed and pseudo-random inputs at `decode_audio_bytes`
//! and only requires that it *returns* (Ok or Err) rather than panicking. It is a
//! fast, always-on complement to the `cargo fuzz` targets under `rust/fuzz`, and a
//! regression guard for the FFI panic-safety hardening.

use fingerprint_core::decode_audio_bytes;

/// Tiny deterministic xorshift PRNG so the corpus is identical on every run and
/// every platform (no `rand`, no clock, no `Math::random`).
struct XorShift(u64);

impl XorShift {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    fn byte(&mut self) -> u8 {
        (self.next_u32() & 0xff) as u8
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.byte()).collect()
    }
}

fn wave_prefixed(rng: &mut XorShift, len: usize) -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(rng.bytes(len.saturating_sub(4)));
    bytes
}

fn wave_with_declared_sizes(rng: &mut XorShift) -> Vec<u8> {
    // A structurally plausible header with adversarial chunk sizes.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&rng.next_u32().to_le_bytes()); // riff size (ignored)
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&rng.next_u32().to_le_bytes()); // fmt chunk size
    bytes.extend_from_slice(&(rng.next_u32() as u16).to_le_bytes()); // format
    bytes.extend_from_slice(&(rng.next_u32() as u16).to_le_bytes()); // channels
    bytes.extend_from_slice(&rng.next_u32().to_le_bytes()); // sample rate
    bytes.extend_from_slice(&rng.next_u32().to_le_bytes()); // byte rate
    bytes.extend_from_slice(&(rng.next_u32() as u16).to_le_bytes()); // block align
    bytes.extend_from_slice(&(rng.next_u32() as u16).to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&rng.next_u32().to_le_bytes()); // data chunk size (may overflow)
    let trailing = (rng.next_u32() % 64) as usize;
    bytes.extend(rng.bytes(trailing));
    bytes
}

fn mp3_prefixed(rng: &mut XorShift, len: usize) -> Vec<u8> {
    let mut bytes = if rng.byte() & 1 == 0 {
        b"ID3".to_vec()
    } else {
        vec![0xff, 0xe0 | (rng.byte() & 0x1f)]
    };
    bytes.extend(rng.bytes(len));
    bytes
}

#[test]
fn decoder_never_panics_on_malformed_input() {
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);

    // A few hand-picked degenerate inputs first.
    for fixed in [
        Vec::new(),
        b"R".to_vec(),
        b"RIFF".to_vec(),
        b"RIFF\x04\x00\x00\x00WAV".to_vec(),
        b"RIFF\xff\xff\xff\xffWAVE".to_vec(),
        b"ID3".to_vec(),
        vec![0xff, 0xfb],
        vec![0u8; 4_096],
    ] {
        let _ = decode_audio_bytes(&fixed);
    }

    // Then a large reproducible corpus of structured and random junk.
    for _ in 0..2_000 {
        let len = (rng.next_u32() % 256) as usize;
        let _ = decode_audio_bytes(&rng.bytes(len));
        let _ = decode_audio_bytes(&wave_prefixed(&mut rng, len));
        let _ = decode_audio_bytes(&wave_with_declared_sizes(&mut rng));
        let _ = decode_audio_bytes(&mp3_prefixed(&mut rng, len));
    }
}
