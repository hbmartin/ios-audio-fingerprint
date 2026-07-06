use std::io::{Cursor, ErrorKind, Read, Seek, SeekFrom};
use std::sync::OnceLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, Probe};
use symphonia::default::formats::MpaReader;

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

/// A probe that only knows the MPEG-audio format.
///
/// `symphonia::default::get_probe()` additionally registers the ID3v2
/// metadata reader, which panics on malformed ID3v2.4 extended headers
/// (`unreachable!` in symphonia-metadata 0.5.5, `id3v2/mod.rs:238`; found by
/// the `decode_audio` fuzz target). Fingerprinting never consumes tag
/// metadata, so leading tags are skipped without parsing by
/// [`skip_id3v2_tags`] and the metadata reader stays unregistered.
fn mp3_probe() -> &'static Probe {
    static PROBE: OnceLock<Probe> = OnceLock::new();
    PROBE.get_or_init(|| {
        let mut probe = Probe::default();
        probe.register_all::<MpaReader>();
        probe
    })
}

/// Skip any ID3v2 tags prepended to `data` without parsing their contents.
///
/// Tag layout: `"ID3"`, major version, minor version, flags (1 byte), 28-bit
/// syncsafe length of the tag body (4 bytes), then the body, plus a 10-byte
/// footer if the major version is 4 and flag `0x10` is set. A tag that claims
/// to extend past the end of `data` yields an empty slice, which the caller
/// reports as unsupported.
fn skip_id3v2_tags(mut data: &[u8]) -> &[u8] {
    while data.len() >= 10 && data.starts_with(b"ID3") {
        let major_version = data[3];
        let flags = data[5];
        let body_len = data[6..10]
            .iter()
            .fold(0usize, |len, &byte| (len << 7) | usize::from(byte & 0x7f));
        // A footer exists only in ID3v2.4; in v2.2/v2.3 the 0x10 flag bit is
        // undefined and must not be treated as a footer indicator, or the skip
        // would eat 10 bytes of the following audio.
        let footer_len = if major_version >= 4 && flags & 0x10 != 0 {
            10
        } else {
            0
        };
        let total = 10usize.saturating_add(body_len).saturating_add(footer_len);
        if total >= data.len() {
            return &[];
        }
        data = &data[total..];
    }
    data
}

/// A `MediaSource` over caller-owned bytes with the borrow lifetime erased.
///
/// Symphonia's `MediaSourceStream` only accepts `'static` sources, which
/// previously forced `decode_mp3_bytes` to copy the entire input (up to
/// [`MAX_MP3_INPUT_BYTES`]) just to satisfy the bound. The raw-pointer view
/// avoids that copy.
///
/// # Safety
///
/// Construction is `unsafe`: the caller promises the referenced bytes outlive
/// every use of the source. `decode_mp3_bytes` upholds this by consuming the
/// source fully inside the function that borrows `data` — the source is
/// dropped with the format reader before the function returns, and no handle
/// to it escapes.
struct BorrowedByteSource {
    cursor: Cursor<&'static [u8]>,
}

impl BorrowedByteSource {
    /// # Safety
    ///
    /// `data` must remain live and unmodified for the whole lifetime of the
    /// returned source.
    unsafe fn new(data: &[u8]) -> Self {
        let erased: &'static [u8] =
            unsafe { std::slice::from_raw_parts(data.as_ptr(), data.len()) };
        Self {
            cursor: Cursor::new(erased),
        }
    }
}

impl Read for BorrowedByteSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl Seek for BorrowedByteSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl MediaSource for BorrowedByteSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.cursor.get_ref().len() as u64)
    }
}

fn decode_mp3_bytes(data: &[u8]) -> Result<DecodedAudio, FingerprintError> {
    if data.len() > MAX_MP3_INPUT_BYTES {
        return Err(FingerprintError::invalid("MP3 input is too large"));
    }

    let data = skip_id3v2_tags(data);

    // A leading ID3v2 tag can wrap a non-MPEG container: some encoders prepend
    // tags to WAV files, and `looks_like_mp3` routes any `ID3`-prefixed input
    // here. With the tag stripped, re-sniff for WAV before probing as MPEG
    // audio so tagged WAV decodes instead of being rejected as unsupported.
    if looks_like_wave(data) {
        return decode_wave_bytes(data);
    }

    // SAFETY: the source is consumed entirely within this function while
    // `data` is borrowed, and nothing derived from it outlives the probe or
    // decoder locals below (see `BorrowedByteSource`).
    let source: Box<dyn MediaSource> = Box::new(unsafe { BorrowedByteSource::new(data) });
    let media_source = MediaSourceStream::new(source, Default::default());

    // The custom probe registers only `MpaReader`, and symphonia 0.5's
    // `Probe::format` ignores the hint entirely (marker-scan only), so an
    // empty hint is equivalent and avoids implying the extension gates format
    // selection.
    let probed = mp3_probe()
        .format(
            &Hint::new(),
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
