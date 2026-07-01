use crate::audio::DecodedAudio;
use crate::error::FingerprintError;

const RIFF: &[u8; 4] = b"RIFF";
const WAVE: &[u8; 4] = b"WAVE";
const FMT: &[u8; 4] = b"fmt ";
const DATA: &[u8; 4] = b"data";
const PCM_FORMAT: u16 = 1;
const IEEE_FLOAT_FORMAT: u16 = 3;

pub fn looks_like_wave(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == RIFF
}

pub fn decode_wave_bytes(bytes: &[u8]) -> Result<DecodedAudio, FingerprintError> {
    if bytes.len() < 12 {
        return Err(FingerprintError::decode("truncated RIFF/WAVE header"));
    }
    if &bytes[0..4] != RIFF || &bytes[8..12] != WAVE {
        return Err(FingerprintError::decode("no WAVE tag found"));
    }

    let mut offset = 12usize;
    let mut audio_format = None;
    let mut channels = None;
    let mut sample_rate = None;
    let mut bits_per_sample = None;
    let mut data_range = None;

    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = read_u32(bytes, offset + 4) as usize;
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| FingerprintError::decode("WAV chunk size overflow"))?;
        if chunk_end > bytes.len() {
            return Err(FingerprintError::decode("truncated WAV chunk"));
        }

        if chunk_id == FMT {
            if chunk_size < 16 {
                return Err(FingerprintError::decode("truncated WAV fmt chunk"));
            }
            audio_format = Some(read_u16(bytes, chunk_start));
            channels = Some(read_u16(bytes, chunk_start + 2));
            sample_rate = Some(read_u32(bytes, chunk_start + 4));
            bits_per_sample = Some(read_u16(bytes, chunk_start + 14));
        } else if chunk_id == DATA {
            data_range = Some(chunk_start..chunk_end);
        }

        offset = chunk_end + (chunk_size & 1);
        if offset > bytes.len() {
            break;
        }
    }

    let format = audio_format.ok_or_else(|| FingerprintError::decode("missing WAV fmt chunk"))?;
    let channel_count =
        channels.ok_or_else(|| FingerprintError::decode("missing WAV channel count"))?;
    let rate = sample_rate.ok_or_else(|| FingerprintError::decode("missing WAV sample rate"))?;
    let bits = bits_per_sample.ok_or_else(|| FingerprintError::decode("missing WAV bit depth"))?;
    let range = data_range.ok_or_else(|| FingerprintError::decode("missing WAV data chunk"))?;

    if channel_count == 0 {
        return Err(FingerprintError::unsupported(
            "unsupported WAV channel count: 0",
        ));
    }

    let sample_bytes = (bits / 8) as usize;
    if sample_bytes == 0 || bits % 8 != 0 {
        return Err(FingerprintError::unsupported(format!(
            "Unsupported WAV format: {bits} bit"
        )));
    }

    let mut samples = Vec::with_capacity(range.len() / sample_bytes);
    let mut index = range.start;
    while index + sample_bytes <= range.end {
        let sample = match (format, bits) {
            (PCM_FORMAT, 8) => (bytes[index] as f32 - 128.0) / 128.0,
            (PCM_FORMAT, 16) => read_i16(bytes, index) as f32 / 32_768.0,
            (PCM_FORMAT, 24) => read_i24(bytes, index) as f32 / 8_388_608.0,
            (PCM_FORMAT, 32) => read_i32(bytes, index) as f32 / 2_147_483_648.0,
            (IEEE_FLOAT_FORMAT, 32) => f32::from_bits(read_u32(bytes, index)),
            _ => {
                return Err(FingerprintError::unsupported(format!(
                    "Unsupported WAV format: {bits} bit"
                )))
            }
        };
        samples.push(sample);
        index += sample_bytes;
    }

    Ok(DecodedAudio {
        samples,
        sample_rate: rate,
        channels: channel_count,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_i16(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_i24(bytes: &[u8], offset: usize) -> i32 {
    let mut value = bytes[offset] as i32
        | ((bytes[offset + 1] as i32) << 8)
        | ((bytes[offset + 2] as i32) << 16);
    if value & 0x0080_0000 != 0 {
        value |= !0x00ff_ffff;
    }
    value
}
