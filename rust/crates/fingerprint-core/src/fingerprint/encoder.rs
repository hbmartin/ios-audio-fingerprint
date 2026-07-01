use crate::fingerprint::{HASH_FRAME_COUNT, HASH_STRIDE_FRAMES, HASH_THRESHOLD, PITCH_CLASSES};

pub fn encode_chroma_frames(frames: &[[f32; PITCH_CLASSES]]) -> Vec<u32> {
    if frames.len() < HASH_FRAME_COUNT {
        return Vec::new();
    }

    let last_start = frames.len() - HASH_FRAME_COUNT;
    let mut starts = Vec::new();
    let mut start = 0usize;
    while start <= last_start {
        starts.push(start);
        start += HASH_STRIDE_FRAMES;
    }
    if starts.last().copied() != Some(last_start) {
        starts.push(last_start);
    }

    starts
        .into_iter()
        .map(|start| compute_hash(&frames[start..start + HASH_FRAME_COUNT]))
        .collect()
}

pub fn compute_hash(frames: &[[f32; PITCH_CLASSES]]) -> u32 {
    if frames.len() < 2 {
        return 0;
    }

    let mut hash = 0u32;
    let mut bit = 0u32;
    for offset in 1..frames.len().min(4) {
        let pitch_limit = if offset == 3 { 8 } else { PITCH_CLASSES };
        for pitch in 0..pitch_limit {
            if frames[offset][pitch] - frames[offset - 1][pitch] > HASH_THRESHOLD {
                hash |= 1u32 << bit;
            }
            bit += 1;
            if bit == 28 {
                break;
            }
        }
        if bit == 28 {
            break;
        }
    }

    let coarse_energy: f32 = frames[0].iter().sum();
    let energy_nibble = ((coarse_energy * 4.0) as i32).clamp(0, 15) as u32;
    hash ^ (energy_nibble << 28)
}
