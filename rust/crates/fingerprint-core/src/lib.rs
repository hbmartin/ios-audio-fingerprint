pub mod audio;
pub mod error;
pub mod fingerprint;
pub mod matching;

pub use audio::{decode_audio_bytes, resample_to_mono, DecodedAudio, TARGET_SAMPLE_RATE};
pub use error::FingerprintError;
pub use fingerprint::streaming::{StreamingFingerprinter, StreamingWindowedFingerprinter};
pub use fingerprint::{
    fingerprint_samples, fingerprint_windows, Fingerprint, Fingerprinter, WindowedFingerprint,
    FRAME_SIZE, HASH_FRAME_COUNT, HASH_STRIDE_FRAMES, HOP_SIZE,
};
pub use matching::{compare_hashes, compare_hashes_with_drift, CheckpointMatcher, MatchResult};

pub fn fingerprint_version() -> &'static str {
    "fingerprint_core 0.1.0"
}

pub fn fingerprint_to_bytes(hashes: &[u32], duration_ms: u32) -> Vec<u8> {
    let count = hashes.len().min(u32::MAX as usize);
    let mut bytes = Vec::with_capacity(8 + count * 4);
    bytes.extend_from_slice(&duration_ms.to_le_bytes());
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    for hash in hashes.iter().take(count) {
        bytes.extend_from_slice(&hash.to_le_bytes());
    }
    bytes
}

pub fn fingerprint_from_bytes(data: &[u8]) -> Option<Fingerprint> {
    if data.len() < 8 {
        return None;
    }

    let duration_ms = read_u32(data, 0);
    let hash_count = read_u32(data, 4) as usize;
    let payload_len = hash_count.checked_mul(4)?;
    let required_len = 8usize.checked_add(payload_len)?;
    if required_len > data.len() {
        return None;
    }

    let mut hashes = Vec::with_capacity(hash_count);
    for index in 0..hash_count {
        hashes.push(read_u32(data, 8 + index * 4));
    }

    Some(Fingerprint {
        hashes,
        duration_ms,
    })
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}
