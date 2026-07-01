use std::io::{Cursor, ErrorKind};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::wav::{decode_wave_bytes, looks_like_wave};
use crate::audio::DecodedAudio;
use crate::error::FingerprintError;

const MAX_MP3_INPUT_BYTES: usize = 128 * 1024 * 1024;
const MAX_MP3_DECODED_SAMPLES: usize = 64 * 1024 * 1024;

pub fn decode_audio_bytes(data: &[u8]) -> Result<DecodedAudio, FingerprintError> {
    if looks_like_wave(data) {
        return decode_wave_bytes(data);
    }

    if looks_like_mp3(data) {
        return decode_mp3_bytes(data);
    }

    Err(FingerprintError::unsupported("Unsupported audio format."))
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3")
        || bytes
            .get(0..2)
            .is_some_and(|header| header[0] == 0xff && (header[1] & 0xe0) == 0xe0)
}

fn decode_mp3_bytes(data: &[u8]) -> Result<DecodedAudio, FingerprintError> {
    if data.len() > MAX_MP3_INPUT_BYTES {
        return Err(FingerprintError::invalid("MP3 input is too large"));
    }

    // Borrow `data` directly instead of cloning up to 128 MiB into an owned
    // buffer. Symphonia's `MediaSourceStream::new` requires a
    // `Box<dyn MediaSource + 'static>`, so the borrow's lifetime is erased.
    //
    // SAFETY: the `MediaSourceStream` and everything derived from it (`format`,
    // `decoder`, and each decoded `Packet`) are locals dropped before this
    // function returns, whereas `data` is borrowed for the entire function body.
    // Symphonia reads the source synchronously on the current thread and copies
    // decoded audio into owned buffers; it never retains the source past decoding
    // or moves it to another thread. The erased `'static` lifetime is therefore
    // never observed beyond the true lifetime of `data`.
    let source: Box<dyn MediaSource + '_> = Box::new(Cursor::new(data));
    let source: Box<dyn MediaSource> = unsafe { std::mem::transmute(source) };
    let media_source = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            media_source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| FingerprintError::unsupported("Unsupported audio format."))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| FingerprintError::unsupported("Unsupported audio format."))?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut channel_count = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(0);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| {
            FingerprintError::decode(format!("failed to create MP3 decoder: {error}"))
        })?;

    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(FingerprintError::decode("MP3 decoder reset required"));
            }
            Err(error) => {
                if samples.is_empty() {
                    return Err(FingerprintError::unsupported("Unsupported audio format."));
                }
                return Err(FingerprintError::decode(format!(
                    "MP3 packet error: {error}"
                )));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sample_rate = spec.rate;
                channel_count = spec.channels.count() as u16;
                let mut sample_buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buffer.copy_interleaved_ref(decoded);
                let decoded_samples = sample_buffer.samples();
                if samples.len().saturating_add(decoded_samples.len()) > MAX_MP3_DECODED_SAMPLES {
                    return Err(FingerprintError::invalid("MP3 decodes to too many samples"));
                }
                samples.extend_from_slice(decoded_samples);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => {
                return Err(FingerprintError::decode(format!(
                    "MP3 decode error: {error}"
                )));
            }
        }
    }

    if samples.is_empty() || sample_rate == 0 || channel_count == 0 {
        return Err(FingerprintError::unsupported("Unsupported audio format."));
    }

    Ok(DecodedAudio {
        samples,
        sample_rate,
        channels: channel_count,
    })
}
