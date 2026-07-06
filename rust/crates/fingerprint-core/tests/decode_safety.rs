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

/// Regression inputs for the symphonia-metadata 0.5.5 ID3v2.4 panic.
///
/// The crate's extended-header parser decodes the text-encoding restriction
/// with `(restrictions & 0x40) >> 5`, which can evaluate to 2 and hit an
/// `unreachable!()` (`id3v2/mod.rs:238`). Found by the `decode_audio` fuzz
/// target; fixed on our side by never handing tag bytes to the metadata
/// parser.
#[test]
fn decoder_never_panics_on_id3v2_extended_header_restrictions() {
    // Exact crashing input from CI (crash-9acc0995bc54e688c19ba22b45665b76c34cfb41).
    let fuzz_crash: &[u8] = &[
        0x49, 0x44, 0x33, 0x04, 0x2b, 0xcc, 0xf1, 0x06, 0x01, 0x28, 0xf5, 0xcf, 0xb4, 0xf1, 0x01,
        0x16, 0x01, 0xff, 0x32, 0x00,
    ];
    let _ = decode_audio_bytes(fuzz_crash);

    // Well-formed ID3v2.4 tag: extended header with the restrictions flag set
    // and bit 0x40 of the restrictions byte high — the general panic class.
    let mut tag = Vec::new();
    tag.extend_from_slice(b"ID3\x04\x00\x40"); // v2.4, extended-header flag
    tag.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // syncsafe tag body size: 8
    tag.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // syncsafe ext header size: 8
    tag.push(0x01); // number of flag bytes
    tag.push(0x10); // restrictions flag
    tag.push(0x01); // restrictions data length
    tag.push(0x40); // restrictions byte: (0x40 & 0x40) >> 5 == 2 -> unreachable!()
    let _ = decode_audio_bytes(&tag);
}

/// Prepending ID3v2 tags must not change the decoded audio: the decoder skips
/// them without parsing (see `skip_id3v2_tags`), and the samples must match
/// the untagged file exactly.
#[test]
fn decoder_skips_id3v2_tags_on_real_mp3() {
    let mp3: &[u8] = include_bytes!("fixtures/reference.mp3");
    let plain = decode_audio_bytes(mp3).expect("reference.mp3 must decode");

    // ID3v2.3 tag with a 32-byte zero-padded body.
    let mut v23_tagged = b"ID3\x03\x00\x00\x00\x00\x00\x20".to_vec();
    v23_tagged.extend_from_slice(&[0u8; 32]);
    v23_tagged.extend_from_slice(mp3);

    // ID3v2.4 tag with a footer (flag 0x10), followed by a second tag.
    let mut v24_tagged = b"ID3\x04\x00\x10\x00\x00\x00\x10".to_vec();
    v24_tagged.extend_from_slice(&[0u8; 16]);
    v24_tagged.extend_from_slice(b"3DI\x04\x00\x10\x00\x00\x00\x10");
    v24_tagged.extend_from_slice(b"ID3\x02\x00\x00\x00\x00\x00\x08");
    v24_tagged.extend_from_slice(&[0u8; 8]);
    v24_tagged.extend_from_slice(mp3);

    for tagged in [v23_tagged, v24_tagged] {
        let decoded = decode_audio_bytes(&tagged).expect("tagged MP3 must decode");
        assert_eq!(decoded.samples, plain.samples);
        assert_eq!(decoded.sample_rate, plain.sample_rate);
        assert_eq!(decoded.channels, plain.channels);
    }
}

/// A footer exists only in ID3v2.4. A v2.2/v2.3 tag with the 0x10 flag bit set
/// (undefined in those versions) must not trigger a 10-byte footer skip, or the
/// skip eats into the first MPEG frame and the decoded samples silently diverge
/// from the untagged file.
#[test]
fn decoder_ignores_footer_flag_before_id3v2_4() {
    let mp3: &[u8] = include_bytes!("fixtures/reference.mp3");
    let plain = decode_audio_bytes(mp3).expect("reference.mp3 must decode");

    // ID3v2.3 header (major version 3) with the 0x10 flag bit set and a
    // 16-byte zero body. There is no footer, so exactly header + body is
    // skipped and decoding must start at the first frame.
    let mut v23_stray_footer_flag = b"ID3\x03\x00\x10\x00\x00\x00\x10".to_vec();
    v23_stray_footer_flag.extend_from_slice(&[0u8; 16]);
    v23_stray_footer_flag.extend_from_slice(mp3);

    let decoded = decode_audio_bytes(&v23_stray_footer_flag).expect("tagged MP3 must decode");
    assert_eq!(decoded.samples, plain.samples);
    assert_eq!(decoded.sample_rate, plain.sample_rate);
    assert_eq!(decoded.channels, plain.channels);
}

/// Some encoders prepend an ID3v2 tag to a WAV file. The tag is stripped
/// without parsing and the underlying RIFF/WAVE container must still decode,
/// identically to the untagged WAV.
#[test]
fn decoder_decodes_id3v2_tagged_wave() {
    let wave = reference_wave(8_000, 1, 0.05);
    let plain = decode_audio_bytes(&wave).expect("reference WAV must decode");

    // ID3v2.3 tag with a 16-byte zero body, then the RIFF/WAVE payload.
    let mut tagged = b"ID3\x03\x00\x00\x00\x00\x00\x10".to_vec();
    tagged.extend_from_slice(&[0u8; 16]);
    tagged.extend_from_slice(&wave);

    let decoded = decode_audio_bytes(&tagged).expect("ID3-tagged WAV must decode");
    assert_eq!(decoded.samples, plain.samples);
    assert_eq!(decoded.sample_rate, plain.sample_rate);
    assert_eq!(decoded.channels, plain.channels);
}

/// Interleave a mono ramp into 16-bit PCM and wrap it in a canonical RIFF/WAVE
/// container. Kept local to this test file so the decode-safety suite stays
/// self-contained.
fn reference_wave(sample_rate: u32, channels: u16, seconds: f32) -> Vec<u8> {
    let count = (sample_rate as f32 * seconds) as usize;
    let mut payload = Vec::with_capacity(count * channels as usize * 2);
    for index in 0..count {
        let scaled = ((index % 256) as i16 - 128) * 128;
        for _ in 0..channels {
            payload.extend_from_slice(&scaled.to_le_bytes());
        }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload);
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
