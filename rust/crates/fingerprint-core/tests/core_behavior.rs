use assert_approx_eq::assert_approx_eq;
use fingerprint_core::audio::decoder::decode_audio_bytes;
use fingerprint_core::audio::resampler::{resample_to_mono, samples_for_milliseconds};
use fingerprint_core::fingerprint::streaming::{
    StreamingFingerprinter, StreamingWindowedFingerprinter,
};
use fingerprint_core::{
    compare_hashes, compare_hashes_with_drift, fingerprint_from_bytes, fingerprint_samples,
    fingerprint_to_bytes, fingerprint_windows, CheckpointMatcher, FingerprintError, FRAME_SIZE,
    TARGET_SAMPLE_RATE,
};

#[test]
fn serialization_uses_recovered_layout_and_trailing_bytes_are_ignored() {
    let bytes = fingerprint_to_bytes(&[0x1122_3344, 0xaabb_ccdd], 1_234);
    assert_eq!(
        bytes,
        vec![
            0xd2, 0x04, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x44, 0x33, 0x22, 0x11, 0xdd, 0xcc,
            0xbb, 0xaa,
        ]
    );

    let decoded = fingerprint_from_bytes(&bytes).unwrap();
    assert_eq!(decoded.duration_ms, 1_234);
    assert_eq!(decoded.hashes, vec![0x1122_3344, 0xaabb_ccdd]);
    assert!(fingerprint_from_bytes(&[1, 2, 3]).is_none());
    assert!(fingerprint_from_bytes(&[0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0]).is_none());

    let mut with_trailing = bytes;
    with_trailing.extend_from_slice(&[9, 9, 9]);
    assert_eq!(
        fingerprint_from_bytes(&with_trailing).unwrap().hashes.len(),
        2
    );
}

#[test]
fn matching_and_checkpoint_ordering_follow_spec() {
    assert_eq!(compare_hashes(&[0, u32::MAX], &[0, u32::MAX]), 1.0);
    assert_eq!(compare_hashes(&[0], &[u32::MAX]), 0.0);
    assert_eq!(compare_hashes(&[], &[1, 2]), 0.0);
    assert!(compare_hashes(&[1, 2, 3], &[9, 1, 2, 3]) < 1.0);
    assert_eq!(compare_hashes_with_drift(&[1, 2, 3], &[9, 1, 2, 3], 1), 1.0);

    let mut matcher = CheckpointMatcher::with_drift(1);
    matcher.add(20.0, vec![0, 1, 2], 3.0);
    matcher.add(10.0, vec![7, 0, 1, 2], 4.0);
    matcher.add(30.0, vec![0, 1, 2], 5.0);

    let matches = matcher.find_top_matches(&[0, 1, 2], 2);
    assert_eq!(matcher.count(), 3);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].timestamp, 10.0);
    assert_eq!(matches[0].score, 1.0);
}

#[test]
fn wav_decoder_supports_required_integer_and_float_shapes() {
    let u8_wav = wave_file(1, 8, 1, 8_000, vec![0, 128, 255]);
    let decoded = decode_audio_bytes(&u8_wav).unwrap();
    assert_eq!(decoded.channels, 1);
    assert_eq!(decoded.sample_rate, 8_000);
    assert_approx_eq!(decoded.samples[0], -1.0, 0.00001);
    assert_approx_eq!(decoded.samples[1], 0.0, 0.00001);

    let i16_wav = wave_file(1, 16, 2, 11_025, i16_bytes(&[-32_768, 0, 16_384, 32_767]));
    let decoded = decode_audio_bytes(&i16_wav).unwrap();
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.samples.len(), 4);
    assert_approx_eq!(decoded.samples[0], -1.0, 0.00001);
    assert_approx_eq!(decoded.samples[2], 0.5, 0.0001);

    let i24_wav = wave_file(1, 24, 1, 11_025, i24_bytes(&[-8_388_608, 0, 8_388_607]));
    let decoded = decode_audio_bytes(&i24_wav).unwrap();
    assert_approx_eq!(decoded.samples[0], -1.0, 0.00001);
    assert_approx_eq!(decoded.samples[1], 0.0, 0.00001);

    let i32_wav = wave_file(
        1,
        32,
        1,
        11_025,
        i32_bytes(&[-2_147_483_648, 0, 1_073_741_824]),
    );
    let decoded = decode_audio_bytes(&i32_wav).unwrap();
    assert_approx_eq!(decoded.samples[0], -1.0, 0.00001);
    assert_approx_eq!(decoded.samples[2], 0.5, 0.00001);

    let f32_wav = wave_file(3, 32, 1, 11_025, f32_bytes(&[-0.25, 0.5]));
    let decoded = decode_audio_bytes(&f32_wav).unwrap();
    assert_eq!(decoded.samples, vec![-0.25, 0.5]);
}

#[test]
fn wav_decoder_reports_typed_errors() {
    assert!(matches!(
        decode_audio_bytes(&[]),
        Err(FingerprintError::UnsupportedFormat { .. })
    ));
    assert!(matches!(
        decode_audio_bytes(b"RIFF\x04\x00\x00\x00WAV"),
        Err(FingerprintError::DecodeError { .. })
    ));

    let unsupported = wave_file(1, 12, 1, 11_025, vec![0, 0, 0, 0]);
    assert!(matches!(
        decode_audio_bytes(&unsupported),
        Err(FingerprintError::UnsupportedFormat { .. })
    ));
}

#[test]
fn resampling_downmixes_and_uses_floor_output_count() {
    let stereo = vec![1.0, -1.0, 0.5, 0.25, -0.5, 0.5];
    let mono = resample_to_mono(&stereo, TARGET_SAMPLE_RATE, 2);
    assert_eq!(mono, vec![0.0, 0.375, 0.0]);

    let input = vec![0.0; 44_101];
    let output = resample_to_mono(&input, 44_100, 1);
    assert_eq!(output.len(), 11_025);
}

#[test]
fn one_shot_and_streaming_fingerprinting_produce_hashes() {
    let samples = sine_wave(TARGET_SAMPLE_RATE, 2.0, 440.0);
    let fingerprint = fingerprint_samples(&samples, 2_000);
    assert!(!fingerprint.hashes.is_empty());

    let windows = fingerprint_windows(&samples, 1_500, 500).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].timestamp_ms, 0);
    assert_eq!(windows[1].timestamp_ms, 500);
    assert!(!windows[0].hashes.is_empty());

    assert!(matches!(
        fingerprint_windows(&samples, 1, 500),
        Err(FingerprintError::InvalidInput { .. })
    ));
    assert!(matches!(
        fingerprint_windows(&samples, 1_500, 0),
        Err(FingerprintError::InvalidInput { .. })
    ));
}

#[test]
fn streaming_matches_duration_and_window_timestamps_for_target_rate() {
    let samples = sine_wave(TARGET_SAMPLE_RATE, 2.0, 440.0);
    let mut streaming = StreamingFingerprinter::new(TARGET_SAMPLE_RATE, 1).unwrap();
    let mut hashes = streaming.push_samples_f32(&samples[..samples.len() / 2], 1);
    hashes.extend(streaming.push_samples_f32(&samples[samples.len() / 2..], 1));
    hashes.extend(streaming.flush());
    assert!(!hashes.is_empty());
    assert_eq!(streaming.duration_ms(), 2_000);

    let mut windowed =
        StreamingWindowedFingerprinter::new(TARGET_SAMPLE_RATE, 1, 1_500, 500).unwrap();
    let mut windows = windowed.push_samples_f32(&samples[..samples.len() / 2], 1);
    windows.extend(windowed.push_samples_f32(&samples[samples.len() / 2..], 1));
    windows.extend(windowed.flush());
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].timestamp_ms, 0);
    assert_eq!(windows[1].timestamp_ms, 500);
    assert_eq!(windowed.duration_ms(), 2_000);
}

#[test]
fn streaming_windows_match_one_shot_windows_for_any_chunking() {
    // Both paths slice the same global chroma-frame grid, so streamed windows
    // must equal one-shot windows for identical input, however it is chunked.
    let samples = sine_wave(TARGET_SAMPLE_RATE, 4.0, 440.0);
    let one_shot = fingerprint_windows(&samples, 1_500, 500).unwrap();
    assert!(!one_shot.is_empty());

    for chunk_size in [733usize, 4_096, 11_025] {
        let mut streaming =
            StreamingWindowedFingerprinter::new(TARGET_SAMPLE_RATE, 1, 1_500, 500).unwrap();
        let mut streamed = Vec::new();
        for chunk in samples.chunks(chunk_size) {
            streamed.extend(streaming.push_samples_f32(chunk, 1));
        }
        streamed.extend(streaming.flush());
        assert_eq!(streamed, one_shot, "chunk size {chunk_size} diverged");
    }
}

#[test]
fn find_top_matches_truncation_preserves_full_ordering() {
    // The top-k selection must return exactly the leading prefix of the fully
    // sorted ordering: score desc, then timestamp asc, then insertion order.
    let mut matcher = CheckpointMatcher::with_drift(2);
    for index in 0..64u32 {
        let hashes: Vec<u32> = (0..16)
            .map(|h| h ^ index.wrapping_mul(2_654_435_761))
            .collect();
        matcher.add((64 - index) as f32, hashes, 1.0);
    }
    let query: Vec<u32> = (0..16).collect();

    let all = matcher.find_top_matches(&query, 64);
    for max_results in [1u32, 5, 33, 63] {
        let top = matcher.find_top_matches(&query, max_results);
        assert_eq!(top.as_slice(), &all[..max_results as usize]);
    }
}

#[test]
fn too_short_samples_produce_no_hashes() {
    let samples = vec![0.0; FRAME_SIZE - 1];
    assert!(fingerprint_samples(&samples, 1).hashes.is_empty());
    assert_eq!(samples_for_milliseconds(1_000), 11_025);
}

fn wave_file(format: u16, bits: u16, channels: u16, sample_rate: u32, payload: Vec<u8>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    append_u32(&mut bytes, 36 + payload.len() as u32);
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    append_u32(&mut bytes, 16);
    append_u16(&mut bytes, format);
    append_u16(&mut bytes, channels);
    append_u32(&mut bytes, sample_rate);
    append_u32(
        &mut bytes,
        sample_rate * channels as u32 * (bits as u32 / 8),
    );
    append_u16(&mut bytes, channels * (bits / 8));
    append_u16(&mut bytes, bits);
    bytes.extend_from_slice(b"data");
    append_u32(&mut bytes, payload.len() as u32);
    bytes.extend_from_slice(&payload);
    bytes
}

fn i16_bytes(values: &[i16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn i24_bytes(values: &[i32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        let raw = value.to_le_bytes();
        bytes.extend_from_slice(&raw[..3]);
    }
    bytes
}

fn i32_bytes(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn append_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn sine_wave(sample_rate: u32, seconds: f32, frequency: f32) -> Vec<f32> {
    let count = (sample_rate as f32 * seconds) as usize;
    (0..count)
        .map(|index| {
            ((2.0 * std::f32::consts::PI * frequency * index as f32) / sample_rate as f32).sin()
                * 0.5
        })
        .collect()
}
