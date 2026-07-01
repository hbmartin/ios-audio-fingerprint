//! Property-based tests (`proptest`).
//!
//! These assert invariants that must hold for *all* inputs rather than a handful
//! of hand-picked cases: serialization round-trips, the bounded/monotone
//! behaviour of the scorers, resampler shape guarantees, and that the byte-facing
//! entry points never panic on arbitrary input.

use fingerprint_core::audio::resampler::{resample_to_mono, samples_for_milliseconds};
use fingerprint_core::{
    compare_hashes, compare_hashes_with_drift, fingerprint_from_bytes, fingerprint_to_bytes,
    TARGET_SAMPLE_RATE,
};
use proptest::collection::vec;
use proptest::prelude::*;

proptest! {
    /// Encoding then decoding a fingerprint recovers exactly the same hashes and
    /// duration, and appended trailing bytes are ignored.
    #[test]
    fn serialization_round_trips(hashes in vec(any::<u32>(), 0..512), duration in any::<u32>()) {
        let encoded = fingerprint_to_bytes(&hashes, duration);
        prop_assert_eq!(encoded.len(), 8 + hashes.len() * 4);

        let decoded = fingerprint_from_bytes(&encoded).expect("valid payload must decode");
        prop_assert_eq!(&decoded.hashes, &hashes);
        prop_assert_eq!(decoded.duration_ms, duration);

        let mut with_trailing = encoded;
        with_trailing.extend_from_slice(&[1, 2, 3, 4, 5]);
        let decoded_trailing = fingerprint_from_bytes(&with_trailing).unwrap();
        prop_assert_eq!(decoded_trailing.hashes, hashes);
        prop_assert_eq!(decoded_trailing.duration_ms, duration);
    }

    /// Deserialization must never panic and must reject any buffer too short for
    /// its declared payload.
    #[test]
    fn from_bytes_is_total(data in vec(any::<u8>(), 0..1_024)) {
        if let Some(fingerprint) = fingerprint_from_bytes(&data) {
            // If it decoded, the buffer really did contain the whole payload.
            prop_assert!(data.len() >= 8 + fingerprint.hashes.len() * 4);
        }
    }

    /// Scores are always in [0, 1], and a sequence is always a perfect match
    /// with itself.
    #[test]
    fn compare_hashes_is_bounded_and_reflexive(
        first in vec(any::<u32>(), 0..256),
        second in vec(any::<u32>(), 0..256),
    ) {
        let score = compare_hashes(&first, &second);
        prop_assert!((0.0..=1.0).contains(&score), "score {score} out of range");
        prop_assert_eq!(compare_hashes(&first, &second), compare_hashes(&second, &first));

        if first.is_empty() {
            prop_assert_eq!(score, 0.0);
        } else {
            prop_assert_eq!(compare_hashes(&first, &first), 1.0);
        }
    }

    /// Allowing drift can only ever find an equal-or-better score than the
    /// zero-offset comparison, and stays within [0, 1].
    #[test]
    fn drift_never_reduces_score(
        first in vec(any::<u32>(), 1..128),
        second in vec(any::<u32>(), 1..128),
        max_drift in 0u32..32,
    ) {
        let base = compare_hashes(&first, &second);
        let drifted = compare_hashes_with_drift(&first, &second, max_drift);
        prop_assert!((0.0..=1.0).contains(&drifted), "drifted score {drifted} out of range");
        prop_assert!(
            drifted + f32::EPSILON >= base,
            "drift {max_drift} lowered score: {drifted} < {base}",
        );
    }

    /// Downmixing to mono at the target rate yields one sample per source frame,
    /// and never reads out of bounds regardless of the declared channel count.
    #[test]
    fn resample_mono_at_target_rate_preserves_frame_count(
        samples in vec(-1.0f32..1.0, 0..2_048),
        channels in 1u16..8,
    ) {
        let mono = resample_to_mono(&samples, TARGET_SAMPLE_RATE, channels);
        let expected_frames = samples.len() / channels as usize;
        prop_assert_eq!(mono.len(), expected_frames);
    }

    /// Resampling produces the floor-based output length documented for the
    /// linear resampler and never panics for arbitrary source rates.
    #[test]
    fn resample_output_length_is_floor_ratio(
        frames in 1usize..4_096,
        sample_rate in 1u32..192_000,
    ) {
        let samples = vec![0.0f32; frames];
        let output = resample_to_mono(&samples, sample_rate, 1);
        if sample_rate == TARGET_SAMPLE_RATE {
            prop_assert_eq!(output.len(), frames);
        } else {
            let ratio = sample_rate as f64 / TARGET_SAMPLE_RATE as f64;
            let expected = (frames as f64 / ratio).floor() as usize;
            prop_assert_eq!(output.len(), expected);
        }
    }

    /// The millisecond-to-sample conversion is monotone and matches the exact
    /// integer formula.
    #[test]
    fn samples_for_milliseconds_matches_formula(ms in any::<u32>()) {
        let expected = (ms as u64 * TARGET_SAMPLE_RATE as u64 / 1_000) as usize;
        prop_assert_eq!(samples_for_milliseconds(ms), expected);
    }
}
