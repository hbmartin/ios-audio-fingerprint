# Pure Rust Fingerprint Reimplementation Specification

## 1. Purpose

This document specifies a pure Rust reimplementation of the missing
fingerprint source code behind `Fingerprint.xcframework`.

The implementation target is a Rust workspace that:

- Recreates the public fingerprinting, matching, serialization, and streaming
  behavior recovered from the binary.
- Avoids the existing closed binary dependency at runtime.
- Uses only Rust source and Rust crates for the core implementation.
- Can optionally expose the same Swift-facing API shape through UniFFI.
- Includes enough tests, fixtures, and benchmarks to compare future changes
  against the current Swift source replacement and, where possible, the
  original binary.

The intended primary output is a source-backed package that can replace
`FingerprintFFI` while preserving the behavior expected by the existing Swift
package and tests.

## 2. Evidence Base

The spec is based on local reverse-engineering artifacts and the current source
replacement:

- `codex-analysis/exports/core-index.tsv`
- `codex-analysis/exports/functions.tsv`
- `codex-analysis/exports/symbols.tsv`
- `codex-analysis/exports/rust-strings.tsv`
- `codex-analysis/exports/oxidizer-core.txt`
- `codex-analysis/exports/reoxide/core-index.tsv`
- `codex-analysis/exports/reoxide/core-decompile.c`
- `codex-analysis/logs/reoxide-ghidra-core-export.log`
- `codex-analysis/reimplementation-plan.md`
- `Sources/Fingerprint/fingerprint_uniffi.swift`
- `Tests/FingerprintTests/FingerprintTests.swift`
- `Tests/FingerprintTests/FingerprintBenchmarkTests.swift`
- `codex-analysis/benchmarks/baseline-20260630T171947Z/summary.md`

Tool usage behind those artifacts:

- Ghidra was used for Mach-O loading, function discovery, decompilation, and
  data export.
- GhidRust was loaded into Ghidra and contributed Rust demangling, Rust common
  data types, Rust string analysis, and Rust standard-library recognition.
- Oxidizer was used for focused Rust-aware decompilation and symbol recovery.
- ReOxide was later run successfully from the local macOS build in
  `/Users/haroldmartin/Downloads/reoxide/venv`. Its Ghidra-integrated Rust
  printer produced focused decompilation for 27 core functions.

## 3. Confidence Levels

Use these confidence labels in implementation comments and issue tracking:

- High confidence: confirmed by symbol names, decompilation, current API tests,
  and current Swift behavior.
- Medium confidence: inferred from decompilation plus current Swift behavior,
  but not yet byte-for-byte tested against the original binary.
- Low confidence: intentionally chosen behavior for robustness or developer
  ergonomics where binary parity is not yet proven.

Current confidence map:

| Area | Confidence | Notes |
| --- | --- | --- |
| Public API shape | High | Recovered from `fingerprint_uniffi` symbols and Swift bindings. |
| Serialization layout | High | Confirmed by decompilation and tests. |
| Hash comparison | High | Confirmed by decompilation shape and tests. |
| Drift matching | High | Confirmed at semantic level; exact allocation/sorting internals not relevant. |
| Checkpoint matcher API | High | Confirmed by symbols and wrapper methods. |
| Audio target rate and frame constants | High | `0x2b11` = `11025`, `0x1000` = `4096`; hop and hash stride inferred consistently. |
| FFT/chroma pipeline | Medium | Confirmed function names and constants; exact floating-point details need differential tests. |
| Hash bit layout | Medium | ReOxide confirms the threshold and energy nibble; exact delta-bit mapping still needs fixture comparison. |
| MP3 support in Rust | High | Original binary includes Symphonia MP3 symbols and `decode_mp3_bytes`. |
| Swift reimplementation MP3 behavior | High | Current Swift source reports MP3 as unsupported. |

## 4. Product Requirements

### 4.1 Functional Requirements

The Rust reimplementation must provide:

1. Fingerprint serialization to and from bytes.
2. Fingerprint comparison by normalized bit agreement.
3. Fingerprint comparison with bounded drift.
4. Checkpoint storage and top-match lookup.
5. One-shot byte decoding and windowed fingerprinting.
6. Streaming fingerprinting from integer PCM and `f32` samples.
7. Streaming windowed fingerprinting from integer PCM and `f32` samples.
8. WAV decoding for common PCM and float WAV payloads.
9. MP3 decoding using pure Rust Symphonia components.
10. A compatibility layer that can regenerate Swift bindings with UniFFI.
11. Unit, integration, differential, property, and benchmark tests.

### 4.2 Non-Functional Requirements

The implementation must:

- Be memory safe without application-level `unsafe`.
- Avoid C, C++, Objective-C, system audio APIs, FFmpeg, CoreAudio, Accelerate,
  or other native runtime dependencies in the Rust core.
- Compile on macOS and iOS targets supported by the current package.
- Be deterministic for the same input bytes and sample buffers.
- Return typed errors instead of panicking on malformed inputs.
- Keep streaming memory bounded where possible.
- Preserve enough API compatibility to be wrapped by UniFFI and consumed from
  Swift.
- Provide release-mode benchmarks stored in a comparable format.

### 4.3 Explicit Non-Goals

The initial Rust implementation does not need to:

- Reconstruct the exact original repository history.
- Preserve private symbol addresses or binary layout.
- Match every internal allocation strategy in the original binary.
- Provide a C ABI unless needed by packaging.
- Support every Symphonia format by default.
- Claim byte-for-byte parity for audio fingerprints until differential fixture
  tests pass.

## 5. Workspace Layout

Use a Cargo workspace:

```text
rust/
  Cargo.toml
  crates/
    fingerprint-core/
      Cargo.toml
      src/
        lib.rs
        audio/
          mod.rs
          decoder.rs
          resampler.rs
          wav.rs
        fingerprint/
          mod.rs
          chroma.rs
          encoder.rs
          fft.rs
          streaming.rs
        matching.rs
        error.rs
      tests/
        serialization.rs
        matching.rs
        audio_decode.rs
        streaming.rs
        windowed.rs
        fixtures.rs
      benches/
        fingerprint_bench.rs
    fingerprint-uniffi/
      Cargo.toml
      src/
        lib.rs
        api.rs
      src/fingerprint_uniffi.udl
      uniffi.toml
    fingerprint-cli/
      Cargo.toml
      src/main.rs
  fixtures/
    audio/
      sine_11025_mono_i16.wav
      sine_44100_stereo_i16.wav
      short.mp3
    fingerprints/
      known_layout.bin
```

`fingerprint-core` is the authoritative implementation. `fingerprint-uniffi`
contains only wrapper types, UniFFI annotations, and conversion glue.
`fingerprint-cli` is optional but useful for local fixture generation,
differential checks, and debugging.

## 6. Cargo Dependencies

### 6.1 Core Dependencies

Use pure Rust dependencies only:

```toml
[dependencies]
thiserror = "2"
rustfft = "6"
num-complex = "0.4"
hound = "3"
symphonia = { version = "0.5", default-features = false, features = ["mp3", "wav", "pcm"] }
```

Notes:

- `rustfft` matches the recovered use of the Rust FFT ecosystem and avoids
  platform-specific FFT APIs.
- `symphonia` provides pure Rust MP3 decoding. This is required for original
  binary parity.
- `hound` is acceptable for simple WAV handling and is also visible in the
  recovered Rust strings. The implementation may use Symphonia for WAV instead,
  but tests must cover the WAV formats listed in this spec either way.
- Do not enable broad Symphonia defaults unless explicitly needed. Keep format
  support deliberate.

### 6.2 Optional Dependencies

```toml
[dev-dependencies]
criterion = "0.5"
proptest = "1"
assert_approx_eq = "1"
```

```toml
[features]
default = ["mp3"]
mp3 = []
wav = []
uniffi = []
differential-original = []
```

The `differential-original` feature may call into the existing xcframework
during tests on macOS. It must not be enabled by default.

## 7. Core Public API

The Rust core API should be idiomatic Rust but map cleanly to the recovered
Swift/UniFFI surface.

### 7.1 Data Types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    pub hashes: Vec<u32>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub timestamp: f32,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowedFingerprint {
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub hashes: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}
```

`DecodedAudio.samples` must be interleaved by channel.

### 7.2 Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("decode error: {message}")]
    DecodeError { message: String },

    #[error("unsupported format: {message}")]
    UnsupportedFormat { message: String },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("io error: {message}")]
    IoError { message: String },
}
```

Error mapping requirements:

- Invalid or truncated fingerprint serialization returns `None` from
  `Fingerprint::from_bytes`, not an error.
- Invalid audio bytes return `DecodeError` if the container is recognized but
  cannot be decoded.
- Unrecognized containers return `UnsupportedFormat`.
- Unsupported WAV sample formats return `UnsupportedFormat`.
- Window duration shorter than one FFT frame returns `InvalidInput` in one-shot
  fingerprinting.
- Window interval of zero returns `InvalidInput` in one-shot fingerprinting.
- I/O errors from readers, if any, map to `IoError`.

### 7.3 Top-Level Functions

```rust
pub fn fingerprint_version() -> &'static str;

pub fn fingerprint_to_bytes(hashes: &[u32], duration_ms: u32) -> Vec<u8>;

pub fn fingerprint_from_bytes(data: &[u8]) -> Option<Fingerprint>;

pub fn compare_hashes(hashes1: &[u32], hashes2: &[u32]) -> f32;

pub fn compare_hashes_with_drift(
    hashes1: &[u32],
    hashes2: &[u32],
    max_drift: u32,
) -> f32;
```

`fingerprint_version()` should initially return:

```text
fingerprint_core 0.1.0
```

If the UniFFI wrapper needs to preserve the existing Swift string exactly, the
wrapper may return `fingerprint_uniffi 0.1.0` while the core returns its own
crate version.

## 8. Serialization Format

### 8.1 Binary Layout

The serialized fingerprint format is:

```text
offset  size  type       meaning
0       4     u32 LE     duration_ms
4       4     u32 LE     hash_count
8       4*N   u32 LE[]   hashes
```

Example:

```rust
let bytes = fingerprint_to_bytes(&[0x11223344, 0xaabbccdd], 1234);
assert_eq!(
    bytes,
    vec![
        0xd2, 0x04, 0x00, 0x00,
        0x02, 0x00, 0x00, 0x00,
        0x44, 0x33, 0x22, 0x11,
        0xdd, 0xcc, 0xbb, 0xaa,
    ],
);
```

### 8.2 Decoding Rules

`Fingerprint::from_bytes(data)` must:

1. Return `None` when `data.len() < 8`.
2. Read `duration_ms` from bytes `0..4`.
3. Read `hash_count` from bytes `4..8`.
4. Return `None` when `8 + hash_count * 4 > data.len()`.
5. Decode exactly `hash_count` hashes from little-endian `u32` values.
6. Ignore trailing bytes after the declared hash payload.

The trailing-byte behavior is required by the recovered decompilation check:
the payload is valid if declared bytes fit within the input length.

### 8.3 Encoding Rules

`Fingerprint::to_bytes()` must:

1. Allocate `8 + hashes.len() * 4` bytes.
2. Write `duration_ms` as little-endian `u32`.
3. Write `hashes.len()` as little-endian `u32`.
4. Write every hash as little-endian `u32`.

If `hashes.len() > u32::MAX`, return `InvalidInput` from a fallible API or
truncate only in the UniFFI compatibility layer if forced by generated binding
limitations. The core should prefer fallible behavior for impossible sizes.

## 9. Matching

### 9.1 Direct Hash Comparison

`compare_hashes(first, second)` returns a normalized bit-agreement score.

Algorithm:

```rust
fn compare_hashes(first: &[u32], second: &[u32]) -> f32 {
    let count = first.len().min(second.len());
    if count == 0 {
        return 0.0;
    }

    let mut matching_bits = 0usize;
    for i in 0..count {
        matching_bits += (!(first[i] ^ second[i])).count_ones() as usize;
    }

    matching_bits as f32 / (count * 32) as f32
}
```

Required results:

- Identical non-empty slices return `1.0`.
- Completely inverted single hashes, such as `[0]` versus `[u32::MAX]`, return
  `0.0`.
- Empty input on either side returns `0.0`.
- Different lengths compare only the overlapping prefix.

### 9.2 Drift-Aware Comparison

`compare_hashes_with_drift(first, second, max_drift)` must:

1. Return `0.0` if either input is empty.
2. Compute the direct score at drift `0`.
3. Let `drift_limit = min(max_drift, first.len(), second.len())`.
4. For every `drift` in `1..=drift_limit`:
   - Compare `first[drift..]` against `second`.
   - Compare `first` against `second[drift..]`.
   - Keep the maximum score.
5. Return the maximum score.

This matches the recovered behavior that checks positive and negative offsets
and selects the best score.

### 9.3 Numeric Semantics

- Scores are `f32`.
- Scores must always be finite for finite inputs.
- No score should be below `0.0` or above `1.0`.
- Sorting by score must use ordinary descending numeric comparison. NaN scores
  must not occur.

## 10. Checkpoint Matching

### 10.1 Types

```rust
#[derive(Debug, Clone)]
struct Checkpoint {
    timestamp: f32,
    hashes: Vec<u32>,
    duration: f32,
}

#[derive(Debug, Clone)]
pub struct CheckpointMatcher {
    checkpoints: Vec<Checkpoint>,
    max_drift: u32,
}
```

### 10.2 Constructors

```rust
impl CheckpointMatcher {
    pub fn new() -> Self;
    pub fn with_drift(max_drift: u32) -> Self;
}
```

`new()` is equivalent to `with_drift(0)`.

### 10.3 Methods

```rust
impl CheckpointMatcher {
    pub fn add(&mut self, timestamp: f32, hashes: Vec<u32>, duration: f32);
    pub fn clear(&mut self);
    pub fn count(&self) -> u32;
    pub fn set_drift(&mut self, max_drift: u32);
    pub fn find_top_matches(&self, query_hashes: &[u32], max_results: u32) -> Vec<MatchResult>;
}
```

Behavior:

- `add` appends a checkpoint and preserves insertion order.
- `duration` is stored even though it is not currently returned by
  `MatchResult`. This preserves the recovered internal shape and leaves room
  for future API parity.
- `count` saturates at `u32::MAX` if the vector length exceeds `u32::MAX`.
- `find_top_matches` computes
  `compare_hashes_with_drift(query_hashes, checkpoint.hashes, self.max_drift)`
  for each checkpoint.
- Results are sorted by descending score.
- Ties are sorted by ascending timestamp.
- If score and timestamp are both equal, preserve original insertion order.
- Return at most `max_results` entries.
- `max_results == 0` returns an empty vector.

## 11. Audio Decoding

### 11.1 Decoder Entry Point

```rust
pub fn decode_audio_bytes(data: &[u8]) -> Result<DecodedAudio, FingerprintError>;
```

The function must return interleaved `f32` samples in the range that the decoder
produces. It should not hard clip valid decoder output unless a codec reports a
larger-than-normal range. Downstream fingerprinting should be robust to samples
outside `[-1.0, 1.0]`.

### 11.2 Format Recognition

The decoder should recognize:

- RIFF/WAVE data beginning with `RIFF....WAVE`.
- MP3 data beginning with ID3 metadata, `ID3`.
- MP3 frame sync data beginning with `0xff` followed by a byte whose high
  three bits are `0b111`.
- Any format Symphonia can confidently probe when MP3/WAV features are enabled.

Unrecognized input returns `UnsupportedFormat`.

### 11.3 WAV Requirements

The WAV path must support:

- PCM unsigned 8-bit.
- PCM signed 16-bit little-endian.
- PCM signed 24-bit little-endian.
- PCM signed 32-bit little-endian.
- IEEE float 32-bit little-endian.
- Mono and multi-channel interleaved data.
- Non-audio chunks before, between, and after `fmt ` and `data`.
- Odd-sized chunks with RIFF padding.

WAV output conversion:

| Format | Conversion |
| --- | --- |
| PCM u8 | `(sample as f32 - 128.0) / 128.0` |
| PCM i16 | `sample as f32 / 32768.0` |
| PCM i24 | `sample as f32 / 8388608.0` |
| PCM i32 | `sample as f32 / 2147483648.0` |
| Float f32 | Preserve value as `f32` |

Unsupported WAV formats return `UnsupportedFormat`.

### 11.4 MP3 Requirements

The original binary contains Symphonia MP3 symbols and a recovered
`fingerprint_core::audio::decoder::decode_mp3_bytes` function. The pure Rust
implementation must therefore support MP3 decoding.

Implementation requirements:

- Use Symphonia's pure Rust MP3 demuxer/decoder path.
- Decode the default audio track.
- Convert decoded sample buffers to interleaved `f32`.
- Preserve the decoded channel count.
- Preserve the decoded sample rate.
- Decode all packets until EOF.
- Treat recoverable packet-level decode errors conservatively:
  - Skip packets only if Symphonia documents the error as recoverable.
  - Return `DecodeError` for unsupported or fatal codec errors.
- Return `UnsupportedFormat` when the input only superficially looks like MP3
  but no MP3 stream can be decoded.

### 11.5 Decoder Tests

Required tests:

- Invalid empty input returns `UnsupportedFormat`.
- Invalid random bytes return `UnsupportedFormat` or `DecodeError`, but never
  panic.
- Truncated RIFF/WAVE returns `DecodeError`.
- Each required WAV sample format decodes to expected `f32` values.
- Stereo WAV preserves interleaving and channel count before resampling.
- MP3 fixture decodes to non-empty samples, positive sample rate, and positive
  channel count.

## 12. Resampling and Mono Conversion

### 12.1 Constants

```rust
pub const TARGET_SAMPLE_RATE: u32 = 11_025;
```

### 12.2 Function

```rust
pub fn resample_to_mono(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32>;
```

Input samples are interleaved. Output samples are mono at `TARGET_SAMPLE_RATE`.

### 12.3 Channel Handling

Let `channel_count = max(channels as usize, 1)` and
`frame_count = samples.len() / channel_count`.

For each frame:

- If `channel_count == 1`, copy the sample.
- Otherwise average all channel samples for the frame:

```rust
mono[frame] = sum(samples[base..base + channel_count]) / channel_count as f32;
```

Ignore trailing samples that do not complete a full frame.

### 12.4 Resampling

If `sample_rate == TARGET_SAMPLE_RATE`, return the mono data.

Otherwise use linear interpolation:

```rust
let ratio = sample_rate as f64 / TARGET_SAMPLE_RATE as f64;
let output_count = (mono.len() as f64 / ratio).floor() as usize;

for out_index in 0..output_count {
    let source_position = out_index as f64 * ratio;
    let source_index = source_position.floor() as usize;
    let fraction = (source_position - source_index as f64) as f32;

    output[out_index] =
        if source_index + 1 < mono.len() {
            mono[source_index] + (mono[source_index + 1] - mono[source_index]) * fraction
        } else if source_index < mono.len() {
            mono[source_index]
        } else {
            0.0
        };
}
```

Invalid `sample_rate == 0` should return `InvalidInput` from fallible APIs.
The low-level helper may return an empty vector if kept infallible for parity.

### 12.5 Streaming Resampling

The initial implementation may use chunk-local linear resampling to match the
current Swift replacement. A higher-fidelity streaming resampler may be added
later, but it must pass chunking equivalence tests before replacing the simple
path.

Chunking equivalence target:

- Pushing all samples in one call and pushing the same samples in arbitrary
  chunks should produce identical hashes for target-rate inputs.
- For non-target-rate inputs, exact chunk independence is not guaranteed with
  simple chunk-local interpolation. Record this as a known limitation unless a
  stateful fractional-position resampler is implemented.

## 13. Fingerprint Pipeline

### 13.1 Constants

```rust
pub const TARGET_SAMPLE_RATE: u32 = 11_025;
pub const FRAME_SIZE: usize = 4_096;
pub const HOP_SIZE: usize = 1_024;
pub const HASH_FRAME_COUNT: usize = 8;
pub const HASH_STRIDE_FRAMES: usize = 2;
pub const PITCH_CLASSES: usize = 12;
pub const MIN_CHROMA_FREQUENCY_HZ: f32 = 28.0;
pub const MAX_CHROMA_FREQUENCY_HZ: f32 = 3_520.0;
pub const A4_HZ: f32 = 440.0;
pub const A4_PITCH_CLASS: f32 = 9.0;
pub const HASH_THRESHOLD: f32 = 0.05;
```

### 13.2 One-Shot Fingerprinting

```rust
pub struct Fingerprinter;

impl Fingerprinter {
    pub fn new() -> Self;

    pub fn fingerprint_data_windowed(
        &self,
        data: &[u8],
        window_duration_ms: u32,
        window_interval_ms: u32,
    ) -> Result<Vec<WindowedFingerprint>, FingerprintError>;
}
```

Pipeline:

1. Decode audio bytes with `decode_audio_bytes`.
2. Convert decoded interleaved audio to mono `TARGET_SAMPLE_RATE` samples.
3. Run windowed fingerprinting over the mono target-rate samples.

### 13.3 Windowed Fingerprinting

```rust
pub fn fingerprint_windows(
    samples: &[f32],
    window_duration_ms: u32,
    window_interval_ms: u32,
) -> Result<Vec<WindowedFingerprint>, FingerprintError>;
```

Rules:

1. Convert `window_duration_ms` to samples:
   `window_samples = window_duration_ms as u64 * 11025 / 1000`.
2. Convert `window_interval_ms` to samples:
   `interval_samples = window_interval_ms as u64 * 11025 / 1000`.
3. If `window_samples < FRAME_SIZE`, return `InvalidInput`.
4. If `interval_samples == 0`, return `InvalidInput`.
5. If `samples.len() < window_samples`, return an empty vector.
6. Starting at sample offset `0`, emit windows while
   `start + window_samples <= samples.len()`.
7. Each output window has:
   - `timestamp_ms = emitted_window_index * window_interval_ms` for one-shot
     mode.
   - `duration_ms = window_duration_ms`.
   - `hashes = fingerprint_samples(window_samples, window_duration_ms).hashes`.
8. Increment `start += interval_samples`.

### 13.4 Whole-Sample Fingerprinting

```rust
pub fn fingerprint_samples(samples: &[f32], duration_ms: u32) -> Fingerprint;
```

Rules:

1. Create an empty chroma frame vector.
2. For offsets `0, HOP_SIZE, 2 * HOP_SIZE, ...`, while
   `offset + FRAME_SIZE <= samples.len()`:
   - Take the frame `samples[offset..offset + FRAME_SIZE]`.
   - Apply Hann window and FFT.
   - Convert magnitudes to 12-bin chroma.
   - Append chroma frame.
3. Encode chroma frames into hashes.
4. Return `Fingerprint { hashes, duration_ms }`.

Do not zero-pad incomplete trailing frames in the initial parity implementation.

### 13.5 FFT Processing

Use `rustfft` for a forward FFT of `FRAME_SIZE` real samples represented as
complex values with zero imaginary parts.

Before FFT, apply a Hann window:

```rust
window(index) = 0.5 * (1.0 - cos(2.0 * PI * index / (FRAME_SIZE - 1)))
```

For each frame:

1. Fill a `Vec<Complex<f32>>` of length `FRAME_SIZE`.
2. For `index < input_frame.len()`, set:
   `real = input_frame[index] * hann[index]`.
3. Set imaginary part to `0.0`.
4. Run forward FFT.
5. Compute magnitudes for bins `0..=FRAME_SIZE / 2`:
   `sqrt(real * real + imag * imag)`.

Implementation note:

- Precompute the Hann window once per `FftProcessor`.
- Reuse FFT scratch buffers in streaming paths.
- The original binary used `rustfft`; this should minimize behavioral drift.

### 13.6 Chroma Extraction

```rust
pub fn chroma_from_magnitudes(magnitudes: &[f32], sample_rate: u32) -> [f32; 12];
```

Algorithm:

1. Initialize `bins[12] = 0.0` and `counts[12] = 0.0`.
2. Let `denominator = max(1, magnitudes.len() * 2 - 2)`.
3. For every FFT magnitude index:
   - `frequency = sample_rate as f32 / denominator as f32 * index as f32`.
   - Skip if `frequency < 28.0`.
   - Skip if `frequency >= 3520.0`.
   - Compute:
     `raw_pitch = (log2(frequency / 440.0) * 12.0 + 9.0) mod 12.0`.
   - If `raw_pitch < 0.0`, add `12.0`.
   - `pitch = min(11, raw_pitch as usize)`.
   - Accumulate `bins[pitch] += magnitude * magnitude`.
   - Accumulate `counts[pitch] += 1.0`.
4. For each pitch with `counts[pitch] > 0.0`, divide:
   `bins[pitch] /= counts[pitch]`.
5. Compute L2 norm:
   `norm = sqrt(sum(bin * bin))`.
6. If `norm > 0.000001`, divide every bin by `norm`.
7. Return bins.

The output is a normalized 12-bin pitch-class energy vector.

### 13.7 Hash Encoding

```rust
pub fn encode_chroma_frames(frames: &[[f32; 12]]) -> Vec<u32>;
pub fn compute_hash(frames: &[[f32; 12]]) -> u32;
```

Encoding rules:

1. If `frames.len() < HASH_FRAME_COUNT`, return an empty vector.
2. Compute hash windows of 8 chroma frames.
3. Use starts `0, 2, 4, ...` while `start + 8 <= frames.len()`.
4. Always include the final possible start `frames.len() - 8` if it was not
   already included by the stride loop.
5. Do not duplicate the final window.

Equivalent start generation:

```rust
let last_start = frames.len() - HASH_FRAME_COUNT;
let mut starts = Vec::new();
let mut start = 0;
while start <= last_start {
    starts.push(start);
    start += HASH_STRIDE_FRAMES;
}
if *starts.last().unwrap() != last_start {
    starts.push(last_start);
}
```

Current inferred hash layout:

1. Return `0` when fewer than 2 chroma frames are supplied.
2. Initialize `hash = 0`.
3. Fill bits `0..27` from positive pitch deltas:
   - Bits `0..11`: `frames[1][pitch] - frames[0][pitch] > HASH_THRESHOLD`.
   - Bits `12..23`: `frames[2][pitch] - frames[1][pitch] > HASH_THRESHOLD`.
   - Bits `24..27`: `frames[3][pitch] - frames[2][pitch] > HASH_THRESHOLD`
     for pitches `0..3`.
4. Compute `coarse_energy = sum(frames[0])`.
5. Compute `energy_nibble = clamp((coarse_energy * 4.0) as i32, 0, 15)`.
6. Set high bits with `hash ^ (energy_nibble as u32) << 28`.

The high-nibble operation is written as XOR because that is how the current
source replacement models the recovered behavior. Since bits `28..31` are
otherwise unset before this operation, XOR and OR are equivalent for the current
layout.

Parity note:

- The exact original `compute_hash` decompilation is incomplete and noisy.
- Implement this layout first because it matches the current tested source
  replacement.
- Add differential tests against original-binary fixture outputs before
  declaring exact original parity.

## 14. Streaming Fingerprinter

### 14.1 Type

```rust
pub struct StreamingFingerprinter {
    sample_rate: u32,
    channels: u16,
    buffer: VecDeque<f32>,
    chroma_frames: VecDeque<[f32; 12]>,
    total_samples_at_target_rate: usize,
    fft: FftProcessor,
}
```

`VecDeque` is recommended to avoid repeated `Vec::remove(0)` costs. A ring
buffer is also acceptable.

### 14.2 Constructor

```rust
impl StreamingFingerprinter {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, FingerprintError>;
}
```

Validation:

- `sample_rate > 0`
- `channels > 0`

For UniFFI compatibility, the wrapper may expose a constructor with the same
signature and translate errors to `FingerprintError`.

### 14.3 Methods

```rust
impl StreamingFingerprinter {
    pub fn duration_ms(&self) -> u32;
    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<u32>;
    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<u32>;
    pub fn flush(&mut self) -> Vec<u32>;
    pub fn reset(&mut self);
}
```

### 14.4 Integer Sample Path

`push_samples(&[i16])`:

1. Convert each sample to `f32` by `sample as f32 / 32768.0`.
2. Use the instance's `self.channels`.
3. Downmix and resample to target-rate mono.
4. Append target samples to `buffer`.
5. Increment `total_samples_at_target_rate` by appended target sample count.
6. Process available FFT frames.
7. Emit available hashes.

### 14.5 Float Sample Path

`push_samples_f32(&[f32], channels)`:

1. Use the method argument `channels`, not necessarily `self.channels`.
2. Downmix and resample to target-rate mono.
3. Append and process exactly as the integer path.

This mirrors the current Swift replacement, where the float path accepts a
channel count per call.

### 14.6 Buffer Processing

`process_buffer()`:

```rust
while buffer.len() >= FRAME_SIZE {
    let frame = first FRAME_SIZE samples;
    let chroma = fft.process_to_chroma(frame);
    chroma_frames.push_back(chroma);
    discard HOP_SIZE samples from buffer;
}
```

Do not process incomplete frames.

### 14.7 Hash Emission

`emit_hashes()`:

```rust
while chroma_frames.len() >= HASH_FRAME_COUNT {
    let first_eight = chroma_frames[0..8];
    hashes.push(compute_hash(first_eight));
    discard HASH_STRIDE_FRAMES chroma frames;
}
```

`flush()` calls `emit_hashes()` but does not zero-pad incomplete sample or
chroma buffers. It should not implicitly call `reset()`.

### 14.8 Duration

`duration_ms()` returns:

```rust
(total_samples_at_target_rate as u64 * 1000 / TARGET_SAMPLE_RATE as u64) as u32
```

Use saturating conversion to `u32` for extremely long streams.

### 14.9 Reset

`reset()` clears:

- sample buffer
- chroma frame queue
- total target-rate sample count
- any stateful resampler position, if implemented

It does not change `sample_rate` or `channels`.

## 15. Streaming Windowed Fingerprinter

### 15.1 Type

```rust
pub struct StreamingWindowedFingerprinter {
    sample_rate: u32,
    channels: u16,
    window_duration_ms: u32,
    window_interval_ms: u32,
    samples_at_target_rate: VecDeque<f32>,
    buffer_start_sample: usize,
    next_window_start: usize,
    fft: FftProcessor,
}
```

`buffer_start_sample` is an absolute target-rate sample index for the first
sample currently held in `samples_at_target_rate`. It allows old emitted data
to be discarded while preserving correct timestamps.

### 15.2 Constructor

```rust
impl StreamingWindowedFingerprinter {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        window_duration_ms: u32,
        window_interval_ms: u32,
    ) -> Result<Self, FingerprintError>;
}
```

Validation:

- `sample_rate > 0`
- `channels > 0`
- `window_interval_ms` maps to at least one target-rate sample
- `window_duration_ms` maps to at least `FRAME_SIZE` target-rate samples

The current Swift replacement returns no error from construction and simply
emits no windows for invalid streaming window parameters. The Rust core should
prefer a fallible constructor. If strict Swift compatibility is required, the
UniFFI wrapper can expose a non-throwing constructor that stores an invalid
state and makes push/flush return empty results, but that compatibility choice
should be explicit.

### 15.3 Methods

```rust
impl StreamingWindowedFingerprinter {
    pub fn duration_ms(&self) -> u32;
    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<WindowedFingerprint>;
    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<WindowedFingerprint>;
    pub fn flush(&mut self) -> Vec<WindowedFingerprint>;
    pub fn reset(&mut self);
}
```

### 15.4 Push Behavior

Both push paths:

1. Convert to interleaved `f32` if needed.
2. Downmix and resample to mono target-rate.
3. Append target-rate samples to `samples_at_target_rate`.
4. Emit all complete windows.
5. Optionally compact already-unused samples.

### 15.5 Window Emission

Let:

```rust
window_samples = samples_for_milliseconds(window_duration_ms);
interval_samples = samples_for_milliseconds(window_interval_ms);
available_end = buffer_start_sample + samples_at_target_rate.len();
```

While:

```rust
next_window_start + window_samples <= available_end
```

emit:

```rust
let relative_start = next_window_start - buffer_start_sample;
let relative_end = relative_start + window_samples;
let window = samples_at_target_rate[relative_start..relative_end];
let timestamp_ms = next_window_start as u64 * 1000 / TARGET_SAMPLE_RATE as u64;
let hashes = fingerprint_samples(window, window_duration_ms).hashes;
WindowedFingerprint { timestamp_ms, duration_ms: window_duration_ms, hashes }
next_window_start += interval_samples;
```

For target-rate sample streams, this must match one-shot windowing timestamps.

### 15.6 Memory Compaction

After emitting windows, discard samples where:

```rust
absolute_sample_index < next_window_start
```

because no future window can start before `next_window_start`. Update
`buffer_start_sample` accordingly.

If exact current Swift behavior is needed for debugging, provide a test-only
mode that disables compaction.

### 15.7 Duration

`duration_ms()` returns the total target-rate samples accepted by the instance,
not the duration of retained buffer memory:

```rust
total_samples_seen_at_target_rate * 1000 / TARGET_SAMPLE_RATE
```

If using `buffer_start_sample + len` as total, ensure compaction does not reduce
reported duration.

## 16. UniFFI Compatibility Layer

The recovered binary exposed `fingerprint_uniffi` objects and functions. To
preserve that integration path, add a UniFFI crate that wraps the core.

### 16.1 UniFFI Data Types

Expose records:

```rust
pub struct FingerprintData {
    pub hashes: Vec<u32>,
    pub duration_ms: u32,
}

pub struct MatchResult {
    pub timestamp: f32,
    pub score: f32,
}

pub struct WindowedFingerprint {
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub hashes: Vec<u32>,
}
```

Expose error enum with associated messages:

```rust
pub enum FingerprintError {
    DecodeError { message: String },
    UnsupportedFormat { message: String },
    InvalidInput { message: String },
    IoError { message: String },
}
```

### 16.2 UniFFI Interfaces

Expose:

- `Fingerprinter`
- `StreamingFingerprinter`
- `StreamingWindowedFingerprinter`
- `CheckpointMatcher`

The wrapper should use `Arc<Mutex<T>>` around mutable core types, matching the
recovered use of `ArcInner` and `MutexGuard` in the binary.

### 16.3 Swift Name Compatibility

Generated Swift should preserve the existing names:

- `FingerprintData.hashes`
- `FingerprintData.durationMs`
- `MatchResult.timestamp`
- `MatchResult.score`
- `WindowedFingerprint.timestampMs`
- `WindowedFingerprint.durationMs`
- `WindowedFingerprint.hashes`
- `fingerprintToBytes(hashes:durationMs:)`
- `fingerprintFromBytes(data:)`
- `compareHashes(hashes1:hashes2:)`
- `compareHashesWithDrift(hashes1:hashes2:maxDrift:)`
- `fingerprintVersion()`
- `Fingerprinter.fingerprintDataWindowed(data:windowDurationMs:windowIntervalMs:)`
- `StreamingFingerprinter.durationMs()`
- `StreamingFingerprinter.pushSamples(samples:)`
- `StreamingFingerprinter.pushSamplesF32(samples:channels:)`
- `StreamingFingerprinter.flush()`
- `StreamingFingerprinter.reset()`
- `StreamingWindowedFingerprinter.durationMs()`
- `StreamingWindowedFingerprinter.pushSamples(samples:)`
- `StreamingWindowedFingerprinter.pushSamplesF32(samples:channels:)`
- `StreamingWindowedFingerprinter.flush()`
- `StreamingWindowedFingerprinter.reset()`
- `CheckpointMatcher.add(timestamp:hashes:duration:)`
- `CheckpointMatcher.clear()`
- `CheckpointMatcher.count()`
- `CheckpointMatcher.findTopMatches(queryHashes:maxResults:)`
- `CheckpointMatcher.setDrift(maxDrift:)`
- `CheckpointMatcher.withDrift(maxDrift:)`

If UniFFI cannot generate exactly matching Swift names from idiomatic Rust
names, add wrapper functions with explicit UDL names.

### 16.4 Threading Semantics

Wrapper objects must be safe to call from Swift across threads:

- Use `Arc<Mutex<CoreType>>`.
- Lock only around the immediate operation.
- Do not hold a lock while invoking callbacks. There are no callbacks in the
  current API.
- If a mutex is poisoned, return `FingerprintError::InvalidInput` or a wrapper
  internal error message rather than panicking.

## 17. Packaging for Apple Platforms

The Rust workspace should be able to produce a binary replacement only after
the source implementation and tests are stable.

Recommended crate type for `fingerprint-uniffi`:

```toml
[lib]
crate-type = ["staticlib", "cdylib", "rlib"]
```

Supported targets:

- `aarch64-apple-ios`
- `aarch64-apple-ios-sim`
- `x86_64-apple-ios` if Intel simulator support is still needed
- `aarch64-apple-darwin` for local tests

Build flow:

1. `cargo build -p fingerprint-uniffi --release --target aarch64-apple-ios`
2. `cargo build -p fingerprint-uniffi --release --target aarch64-apple-ios-sim`
3. Generate Swift bindings with `uniffi-bindgen`.
4. Package headers/modulemaps/Swift sources as required by the current SwiftPM
   package.
5. Create or update `Fingerprint.xcframework`.

The Swift package may later switch between:

- source Swift fallback,
- local Rust source build plugin,
- prebuilt Rust `Fingerprint.xcframework`.

This spec only requires that the Rust source implementation itself is complete.

## 18. Test Specification

### 18.1 Serialization Tests

Required tests:

- Exact byte layout for `[0x11223344, 0xaabbccdd]` and duration `1234`.
- Empty hash vector roundtrip.
- Large hash vector roundtrip.
- `from_bytes([]) == None`.
- `from_bytes(&[1, 2, 3]) == None`.
- Declared count larger than available payload returns `None`.
- Trailing bytes after declared hashes are ignored.
- Maximum duration `u32::MAX` roundtrips.

### 18.2 Matching Tests

Required tests:

- Identical hashes return `1.0`.
- Inverted single hash returns `0.0`.
- Empty input returns `0.0`.
- Different lengths compare overlapping prefix.
- Drift of 1 recovers perfect shifted match:
  `[1,2,3]` versus `[9,1,2,3]` with `max_drift=1` returns `1.0`.
- Drift of 0 does not recover that shifted match.
- Symmetric drift cases work in both directions.
- Scores are finite and in `0.0..=1.0` for random vectors.

### 18.3 Checkpoint Tests

Required tests:

- `new().count() == 0`.
- `add` increments count.
- `clear` resets count.
- `set_drift` changes matching behavior.
- `find_top_matches` sorts by score descending.
- Equal scores sort by timestamp ascending.
- `max_results` limits output.
- `max_results == 0` returns empty output.
- Stored `duration` does not affect current score ordering.

### 18.4 WAV Decoder Tests

Required tests:

- PCM u8 mono.
- PCM i16 mono.
- PCM i24 mono.
- PCM i32 mono.
- IEEE float f32 mono.
- Stereo i16 with expected interleaving.
- Extra unknown chunk before `fmt `.
- Extra unknown odd-sized chunk requiring RIFF padding.
- Missing `fmt ` returns `DecodeError`.
- Missing `data` returns `DecodeError`.
- Unsupported bits per sample returns `UnsupportedFormat`.

### 18.5 MP3 Decoder Tests

Required tests:

- A small checked-in MP3 fixture decodes to non-empty samples.
- Fixture reports expected channel count and sample rate.
- ID3-prefixed MP3 decodes.
- Frame-sync-only MP3 decodes.
- Truncated MP3 returns a typed error and never panics.

### 18.6 Resampler Tests

Required tests:

- Mono target-rate input is returned unchanged.
- Stereo target-rate input averages left/right frames.
- 44,100 Hz to 11,025 Hz produces approximately one quarter as many frames.
- 22,050 Hz to 11,025 Hz produces approximately one half as many frames.
- Non-divisible sample rates use floor output count.
- Trailing incomplete interleaved samples are ignored.

### 18.7 FFT/Chroma Tests

Required tests:

- Silence produces all-zero chroma.
- A sine wave at 440 Hz emphasizes the expected pitch class.
- Scaling a sine wave amplitude does not change normalized chroma materially.
- Chroma contains finite values for finite input.
- Chroma L2 norm is approximately 1.0 for non-silent tonal input.

### 18.8 Fingerprinting Tests

Required tests:

- Fewer than `FRAME_SIZE` samples produce zero hashes.
- A 2-second 440 Hz target-rate sine produces non-empty hashes.
- Windowed one-shot fingerprinting of 2 seconds with 1500 ms windows and
  500 ms interval emits two windows at timestamps `0` and `500`.
- Stereo 44,100 Hz synthetic audio fingerprints after resampling.
- Window duration shorter than `FRAME_SIZE` target-rate samples returns
  `InvalidInput`.
- Zero interval returns `InvalidInput`.

### 18.9 Streaming Tests

Required tests:

- Target-rate mono streaming all-at-once equals target-rate mono streaming in
  chunks.
- `duration_ms()` tracks pushed target-rate audio duration.
- `flush()` emits available hashes but does not reset state.
- `reset()` clears state and duration.
- `push_samples` and `push_samples_f32` produce equivalent hashes for the same
  i16-equivalent target-rate mono input.
- Streaming windowed target-rate input matches one-shot windowed output.
- Streaming windowed timestamps remain correct after internal compaction.

### 18.10 Differential Tests

Add an ignored or feature-gated test suite that compares against the original
binary while it is still present:

```text
cargo test -p fingerprint-core --features differential-original -- --ignored
```

Differential fixture set:

- Serialization fixtures.
- Matching fixtures.
- WAV sine mono 11,025 Hz.
- WAV stereo 44,100 Hz.
- MP3 fixture.
- Streaming all-at-once fixture.
- Streaming chunked fixture.
- Windowed fingerprinting fixture.

For audio fingerprint hashes:

- First compare exact hash vectors.
- If exact hashes do not match, compare lengths, timestamps, durations, and
  matching scores.
- Record any divergence in a fixture manifest with the implementation commit
  and original binary hash.

## 19. Benchmark Specification

Mirror the current stored benchmark workloads:

1. Serialization round trip, 128 hashes.
2. Serialization round trip, 16,384 hashes.
3. Compare 65,536 equal hashes.
4. Compare 65,536 different hashes.
5. Compare 8,192 hashes with max drift 64.
6. Checkpoint matcher: 1,000 checkpoints, 256 hashes each, drift 4, top 10.
7. Streaming mono `f32`, 5 seconds at 11,025 Hz.
8. Streaming stereo `f32`, 5 seconds at 44,100 Hz.
9. One-shot windowed WAV fingerprinting, 6 seconds at 11,025 Hz.
10. Streaming windowed stereo resample, 6 seconds at 44,100 Hz.
11. MP3 decode and fingerprint, using a real short fixture.

Use Criterion and also provide a JSON/CSV/Markdown export compatible with:

```text
codex-analysis/benchmarks/baseline-20260630T171947Z/
```

Current Swift release baseline medians on the local Apple M4 Pro host:

| Benchmark | Median ms |
| --- | ---: |
| `windowed_wav_fingerprinting_six_seconds` | 10.937728 |
| `streaming_windowed_fingerprinter_stereo_resample_six_seconds` | 10.122813 |
| `streaming_fingerprinter_stereo_f32_resample_five_seconds` | 3.825584 |
| `streaming_fingerprinter_mono_f32_five_seconds` | 3.513354 |
| `checkpoint_matcher_add_and_query` | 0.889875 |
| `compare_hashes_with_drift` | 0.393771 |
| `serialization_round_trip_large_fingerprint` | 0.124979 |
| `compare_hashes_large_equal_inputs` | 0.023458 |
| `compare_hashes_large_different_inputs` | 0.022667 |
| `serialization_round_trip_small_fingerprint` | 0.001125 |
| `mp3_unsupported_fast_path` | 0.000250 |

Rust performance acceptance:

- Rust release medians should be no worse than 1.25x the Swift baseline for
  workloads that both implementations support.
- MP3 decode benchmarks are not comparable to the Swift baseline because the
  Swift source replacement currently reports MP3 as unsupported.
- Track allocations for streaming paths. Repeated front-removal from vectors is
  not acceptable in the Rust implementation.

## 20. Implementation Phases

### Phase 1: Skeleton and Serialization

Deliver:

- Cargo workspace.
- `fingerprint-core` crate.
- Error type.
- Public data types.
- Serialization encode/decode.
- Direct and drift matching.
- Checkpoint matcher.

Acceptance:

- Serialization, matching, and checkpoint unit tests pass.
- No audio dependencies required yet.

### Phase 2: Audio Decode

Deliver:

- WAV decoder support.
- MP3 decoder support through Symphonia.
- `DecodedAudio` tests and fixtures.

Acceptance:

- WAV format tests pass.
- MP3 fixture test passes.
- Malformed inputs return typed errors without panics.

### Phase 3: Fingerprint Pipeline

Deliver:

- Resampler.
- FFT processor.
- Chroma extractor.
- Hash encoder.
- One-shot windowed fingerprinting.

Acceptance:

- Synthetic sine fingerprint tests pass.
- One-shot windowed tests pass.
- Chroma tests pass.

### Phase 4: Streaming

Deliver:

- `StreamingFingerprinter`.
- `StreamingWindowedFingerprinter`.
- Memory-bounded buffering.
- Chunking tests.

Acceptance:

- Target-rate chunking equivalence passes.
- Streaming windowed output matches one-shot output for target-rate fixtures.

### Phase 5: UniFFI Wrapper

Deliver:

- `fingerprint-uniffi` crate.
- UDL or proc-macro UniFFI definitions.
- Generated Swift bindings.
- Swift package integration proof.

Acceptance:

- Swift API names match the current package.
- Existing Swift correctness tests pass against Rust bindings.
- Error variants lower correctly into Swift.

### Phase 6: Differential and Benchmark Hardening

Deliver:

- Original binary differential fixtures.
- Criterion benchmarks.
- Export script for benchmark JSON/CSV/Markdown.
- Performance comparison report.

Acceptance:

- Differential tests pass or documented divergences are limited to known
  medium-confidence areas.
- Benchmarks are stored under `codex-analysis/benchmarks/`.
- Rust implementation meets performance acceptance targets.

## 21. Coding Guidelines

### 21.1 Safety

- Do not use `unsafe` in `fingerprint-core` unless a measured bottleneck
  requires it and the unsafe block has a narrow, reviewed invariant.
- Check all external input lengths before indexing.
- Avoid panics in public APIs.
- Use `Result` for invalid runtime inputs.

### 21.2 Numeric Robustness

- Prefer `f32` for algorithmic parity with the recovered Rust ecosystem.
- Use `f64` only for intermediate resampling position math if needed.
- Ensure all public scores are finite.
- For non-finite input samples, choose one behavior and test it:
  - Preferred robust behavior: map non-finite samples to `0.0` at input
    boundaries.
  - Strict parity behavior: preserve non-finite samples and document possible
    downstream NaN behavior.
- The recommended implementation should use the robust behavior unless
  differential tests prove the original behavior matters.

### 21.3 Allocation

- Preallocate vectors where output size is known.
- Reuse FFT buffers in streaming.
- Use `VecDeque` or ring buffers for front-discard workloads.
- Avoid allocating a new `Vec<f32>` for every streaming frame if a borrowed or
  scratch-buffer path is straightforward.

### 21.4 API Stability

- Keep `fingerprint-core` idiomatic and fallible.
- Keep `fingerprint-uniffi` compatibility-focused.
- Do not let Swift naming constraints leak into core module names.

## 22. Documentation Requirements

The Rust implementation must include:

- Crate-level docs explaining the fingerprint pipeline.
- Public API docs for every exported type and function.
- A `docs/format.md` file describing the serialized fingerprint format.
- A `docs/algorithm.md` file describing constants, FFT/chroma/hash encoding,
  and known parity caveats.
- A `docs/testing.md` file explaining unit, fixture, differential, and
  benchmark test commands.

## 23. Open Questions

These must be resolved with differential tests before claiming exact parity:

1. Exact hash bit packing in the original `compute_hash`.
2. Exact FFT normalization, if any, in the original `rustfft` path.
3. Exact MP3 decode sample conversion and channel layout from Symphonia.
4. Whether streaming non-target-rate chunking in the original carried
   fractional resampler state across pushes.
5. Whether constructors should be fallible in the Swift-facing layer or match
   the current source replacement's non-throwing constructors.
6. Whether `flush()` should leave incomplete state intact or mark the stream as
   finalized. Current replacement leaves state intact.
7. Whether malformed `f32` input should be sanitized or preserved for exact
   parity.

## 24. Definition of Done

The pure Rust reimplementation is complete when:

1. `cargo fmt --check` passes.
2. `cargo clippy --workspace --all-targets -- -D warnings` passes.
3. `cargo test --workspace` passes.
4. MP3 fixture decoding is supported and tested.
5. Swift package tests pass through the UniFFI wrapper, if Swift compatibility
   is in scope for the milestone.
6. Release benchmarks are stored as JSON, CSV, and Markdown.
7. Differential tests against the original binary either pass or have an
   explicit, reviewed divergence report.
8. The existing binary target can be removed or retained only as a test fixture,
   not as an implementation dependency.
