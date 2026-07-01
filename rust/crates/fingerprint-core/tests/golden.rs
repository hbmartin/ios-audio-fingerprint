//! Golden / snapshot tests.
//!
//! These pin the fingerprint output for a set of fixed, deterministic inputs so
//! that output-preserving refactors (reusing the FFT plan, precomputing the
//! chroma lookup table, removing per-frame allocations, and so on) can be proven
//! to leave the produced hashes untouched.
//!
//! `rustfft` selects architecture-specific SIMD kernels, so the same input can
//! differ in a few low-order magnitude bits between x86_64 and aarch64. To stay
//! green across the ubuntu (x86_64) Rust CI and the macOS (aarch64) Fingerprint
//! CI, every check enforces:
//!
//!   * an exact hash count, and
//!   * a tight similarity floor against the committed golden.
//!
//! On the reference architecture the goldens were captured on (aarch64) the
//! check is additionally exact, so any bit-level drift from a supposedly
//! output-preserving change fails locally and in the macOS CI.
//!
//! To regenerate the constants after an intentional algorithm change, run:
//!
//! ```text
//! cargo test -p fingerprint-core --test golden -- --ignored --nocapture emit_golden
//! ```
//!
//! and paste the printed arrays back into this file.

use std::f32::consts::PI;

use fingerprint_core::fingerprint::streaming::StreamingFingerprinter;
use fingerprint_core::{
    compare_hashes, fingerprint_samples, fingerprint_windows, Fingerprinter, TARGET_SAMPLE_RATE,
};

/// Minimum bit-similarity a produced fingerprint must share with its golden on
/// any architecture. On aarch64 the match is additionally required to be exact.
const SIMILARITY_FLOOR: f32 = 0.99;

// ---------------------------------------------------------------------------
// Committed golden vectors (captured on aarch64). Regenerate with emit_golden.
// ---------------------------------------------------------------------------

const GOLDEN_SAMPLES: &[u32] = &GOLDEN_SAMPLES_DATA;
const GOLDEN_SAMPLES_DATA: [u32; 12] = [
    0x907003f0, 0x8c817c00, 0x9001800c, 0x601e0030, 0x70300100, 0x71c01600, 0x66003001, 0x5800c016,
    0x50018008, 0x50020030, 0x500d0040, 0x600800d0,
];

const GOLDEN_WINDOWS: &[u32] = &GOLDEN_WINDOWS_DATA;
const GOLDEN_WINDOWS_DATA: [u32; 32] = [
    0x907003f0, 0x8c817c00, 0x9001800c, 0x70030018, 0x60070030, 0x503001c0, 0x71e00200, 0x61801e00,
    0x61001c00, 0x6c016001, 0x6800800c, 0x50008008, 0x60010008, 0x50060030, 0x500d0040, 0x600800d0,
    0x60100080, 0x50300110, 0x60400200, 0x40400400, 0x40400400, 0x51000800, 0x42001001, 0x52002001,
    0x62002003, 0x54014002, 0x58000004, 0x48018000, 0x48018008, 0x60010018, 0x50020010, 0x40020020,
];
const GOLDEN_WINDOWS_COUNT: usize = 8;
const GOLDEN_WINDOWS_TIMESTAMPS: &[u32] = &[0, 500, 1000, 1500, 2000, 2500, 3000, 3500];

const GOLDEN_DECODED: &[u32] = &GOLDEN_DECODED_DATA;
const GOLDEN_DECODED_DATA: [u32; 8] = [
    0x907003f0, 0x8c817c00, 0x9001800c, 0x70030018, 0x60070030, 0x503001c0, 0x71e00200, 0x61801e00,
];

const GOLDEN_STREAMING: &[u32] = &GOLDEN_STREAMING_DATA;
const GOLDEN_STREAMING_DATA: [u32; 11] = [
    0x907003f0, 0x8c817c00, 0x9001800c, 0x601e0030, 0x70300100, 0x71c01600, 0x66003001, 0x5800c016,
    0x50018008, 0x50020030, 0x500d0040,
];

// ---------------------------------------------------------------------------
// Deterministic signal + container generators.
// ---------------------------------------------------------------------------

/// A fixed, richly varying mono signal: a rising chirp plus a steady tone under
/// a slow tremolo, so consecutive chroma frames differ and the encoder produces
/// a non-trivial hash sequence rather than a run of identical values.
fn reference_mono(sample_rate: u32, seconds: f32) -> Vec<f32> {
    let count = (sample_rate as f32 * seconds) as usize;
    (0..count)
        .map(|index| {
            let t = index as f32 / sample_rate as f32;
            let chirp = (2.0 * PI * (200.0 + 300.0 * t) * t).sin();
            let tone = 0.5 * (2.0 * PI * 660.0 * t).sin();
            let tremolo = 0.6 + 0.4 * (2.0 * PI * 3.0 * t).sin();
            0.4 * tremolo * (chirp + tone)
        })
        .collect()
}

/// Interleave a mono signal into `channels` identical channels of 16-bit PCM and
/// wrap it in a canonical RIFF/WAVE container.
fn reference_wave(sample_rate: u32, channels: u16, seconds: f32) -> Vec<u8> {
    let mono = reference_mono(sample_rate, seconds);
    let mut payload = Vec::with_capacity(mono.len() * channels as usize * 2);
    for sample in &mono {
        let scaled = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
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

// The four workloads under snapshot, each produced from a fixed input.

fn produce_samples() -> Vec<u32> {
    fingerprint_samples(&reference_mono(TARGET_SAMPLE_RATE, 3.0), 3_000).hashes
}

fn produce_windows() -> (Vec<u32>, usize, Vec<u32>) {
    let windows =
        fingerprint_windows(&reference_mono(TARGET_SAMPLE_RATE, 5.0), 1_500, 500).unwrap();
    let count = windows.len();
    let timestamps: Vec<u32> = windows.iter().map(|window| window.timestamp_ms).collect();
    let hashes: Vec<u32> = windows
        .into_iter()
        .flat_map(|window| window.hashes)
        .collect();
    (hashes, count, timestamps)
}

fn produce_decoded() -> Vec<u32> {
    let wave = reference_wave(44_100, 2, 2.0);
    Fingerprinter::new()
        .fingerprint_data_windowed(&wave, 1_500, 500)
        .unwrap()
        .into_iter()
        .flat_map(|window| window.hashes)
        .collect()
}

fn produce_streaming() -> Vec<u32> {
    let samples = reference_mono(TARGET_SAMPLE_RATE, 3.0);
    let mut streaming = StreamingFingerprinter::new(TARGET_SAMPLE_RATE, 1).unwrap();
    let mut hashes = streaming.push_samples_f32(&samples[..samples.len() / 2], 1);
    hashes.extend(streaming.push_samples_f32(&samples[samples.len() / 2..], 1));
    hashes.extend(streaming.flush());
    hashes
}

// ---------------------------------------------------------------------------
// Assertions.
// ---------------------------------------------------------------------------

fn assert_golden(label: &str, produced: &[u32], golden: &[u32]) {
    assert_eq!(
        produced.len(),
        golden.len(),
        "{label}: hash count drifted ({} produced vs {} golden)",
        produced.len(),
        golden.len()
    );

    if !golden.is_empty() {
        let similarity = compare_hashes(produced, golden);
        assert!(
            similarity >= SIMILARITY_FLOOR,
            "{label}: fingerprint drifted (similarity {similarity} < {SIMILARITY_FLOOR})",
        );
    }

    #[cfg(target_arch = "aarch64")]
    assert_eq!(
        produced, golden,
        "{label}: exact reference (aarch64) mismatch",
    );
}

#[test]
fn one_shot_samples_match_golden() {
    assert_golden("fingerprint_samples", &produce_samples(), GOLDEN_SAMPLES);
}

#[test]
fn one_shot_windows_match_golden() {
    let (hashes, count, timestamps) = produce_windows();
    assert_eq!(count, GOLDEN_WINDOWS_COUNT, "window count drifted");
    assert_eq!(
        timestamps, GOLDEN_WINDOWS_TIMESTAMPS,
        "window timestamps drifted"
    );
    assert_golden("fingerprint_windows", &hashes, GOLDEN_WINDOWS);
}

#[test]
fn decoded_wave_windows_match_golden() {
    assert_golden(
        "fingerprint_data_windowed",
        &produce_decoded(),
        GOLDEN_DECODED,
    );
}

#[test]
fn streaming_matches_golden() {
    assert_golden("streaming", &produce_streaming(), GOLDEN_STREAMING);
}

#[test]
fn fingerprinting_is_deterministic() {
    // Independent of the committed goldens: the same input must always produce
    // the same output within a single build.
    assert_eq!(produce_samples(), produce_samples());
    assert_eq!(produce_windows().0, produce_windows().0);
    assert_eq!(produce_decoded(), produce_decoded());
    assert_eq!(produce_streaming(), produce_streaming());
}

/// Prints the golden constants for this file. Ignored by default; run manually
/// to regenerate after an intentional algorithm change (see module docs).
#[test]
#[ignore = "prints golden constants; run manually to regenerate"]
fn emit_golden() {
    fn print_array(name: &str, values: &[u32]) {
        println!("const {name}_DATA: [u32; {}] = [", values.len());
        for chunk in values.chunks(8) {
            let line: Vec<String> = chunk.iter().map(|value| format!("0x{value:08x}")).collect();
            println!("    {},", line.join(", "));
        }
        println!("];");
    }

    let samples = produce_samples();
    let (windows, window_count, timestamps) = produce_windows();
    let decoded = produce_decoded();
    let streaming = produce_streaming();

    println!("\n// ---- paste below into golden.rs ----");
    print_array("GOLDEN_SAMPLES", &samples);
    print_array("GOLDEN_WINDOWS", &windows);
    println!("const GOLDEN_WINDOWS_COUNT: usize = {window_count};");
    println!("const GOLDEN_WINDOWS_TIMESTAMPS: &[u32] = &{timestamps:?};");
    print_array("GOLDEN_DECODED", &decoded);
    print_array("GOLDEN_STREAMING", &streaming);
    println!("// ---- end paste ----\n");
}
