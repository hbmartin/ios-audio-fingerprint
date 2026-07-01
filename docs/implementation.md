# Fingerprint Implementation

This package exposes an audio fingerprinting library to Swift while keeping the
runtime implementation in Rust. The Swift target is intentionally thin: it owns
Swift-native API shapes, copies data across the C ABI boundary, and releases
Rust-owned buffers. The Rust workspace owns audio decoding, resampling,
fingerprint generation, serialization, matching, and the C ABI exported through
the prebuilt `Fingerprint.xcframework`.

## Repository Layout

The important implementation files are:

- `Package.swift`: declares the Swift package, the public `Fingerprint` library
  target, the benchmark executable, the tests, and the binary
  `FingerprintFFI` target.
- `Sources/Fingerprint/fingerprint_uniffi.swift`: Swift facade over the C ABI.
  Despite the filename, this is a direct wrapper around `FingerprintFFI`.
- `rust/Cargo.toml`: Rust workspace root.
- `rust/crates/fingerprint-core`: pure Rust implementation of audio handling,
  fingerprinting, serialization, and matching.
- `rust/crates/fingerprint-ffi`: C ABI layer that converts Rust types into
  pointer/length/capacity structs and opaque handles.
- `rust/crates/fingerprint-ffi/include/FingerprintFFI.h`: public C header used
  by Swift through the xcframework module map.
- `Fingerprint.xcframework`: checked-in binary target consumed by SwiftPM
  during local development.
- `scripts/build-rust-xcframework.sh`: builds the Rust static libraries for
  Apple targets and packages them as an xcframework.
- `scripts/verify-xcframework.sh`: validates the xcframework slices, headers,
  module map, and expected symbols.
- `Tests/FingerprintTests`: Swift behavior tests and XCTest benchmarks.
- `Benchmarks/FingerprintBenchmarkRunner`: standalone benchmark runner that
  writes JSON and CSV reports under `codex-analysis/benchmarks`.

The runtime call stack is:

```text
Swift API
  -> FingerprintFFI C module from Fingerprint.xcframework
    -> fingerprint-ffi exported extern "C" functions
      -> fingerprint-core Rust implementation
```

## Swift Package Shape

`Package.swift` supports iOS 13 and macOS 13. The public product is:

```swift
.library(name: "Fingerprint", targets: ["Fingerprint"])
```

The `Fingerprint` Swift target depends on the binary target:

```swift
.binaryTarget(
    name: "FingerprintFFI",
    path: "Fingerprint.xcframework"
)
```

For releases, `docs/release.md` describes the temporary tag commit that changes
this binary target from a local path to a GitHub Release URL plus SwiftPM
checksum.

## Public Swift API

The Swift wrapper exposes four main data types:

- `FingerprintData`: serialized fingerprint payload with `hashes: [UInt32]`
  and `durationMs: UInt32`.
- `WindowedFingerprint`: one window of fingerprint output with
  `timestampMs`, `durationMs`, and `hashes`.
- `MatchResult`: a checkpoint match with `timestamp` and `score`.
- `FingerprintError`: Swift error enum matching Rust error categories.

The stateless top-level functions are:

- `fingerprintToBytes(hashes:durationMs:)`
- `fingerprintFromBytes(data:)`
- `fingerprintVersion()`
- `compareHashes(hashes1:hashes2:)`
- `compareHashesWithDrift(hashes1:hashes2:maxDrift:)`

The stateful public classes are:

- `Fingerprinter`: decodes an audio `Data` blob and returns windowed
  fingerprints.
- `StreamingFingerprinter`: accepts raw interleaved PCM chunks and emits hash
  values incrementally.
- `StreamingWindowedFingerprinter`: accepts raw interleaved PCM chunks and emits
  complete windowed fingerprints incrementally.
- `CheckpointMatcher`: stores timestamped fingerprint checkpoints and returns
  the best matches for a query fingerprint.

The Swift wrapper copies every Rust-owned array into Swift storage before
freeing the FFI buffer, so callers receive ordinary Swift value types rather
than borrowed pointers.

## Rust Workspace

The Rust workspace contains two crates:

- `fingerprint-core`: implementation crate.
- `fingerprint-ffi`: ABI crate built as `staticlib`, `cdylib`, and `rlib`.

`fingerprint-core` depends on:

- `rustfft` for FFT processing.
- `num-complex` for FFT buffers.
- `symphonia` with `mp3`, `wav`, and `pcm` features for MP3 decoding.
- `thiserror` for typed errors.

The core crate re-exports the stable implementation surface from
`rust/crates/fingerprint-core/src/lib.rs`, including constants, fingerprint
functions, streaming types, matching types, and serialization helpers.

## Audio Input Handling

There are two input paths.

The one-shot `Fingerprinter.fingerprintDataWindowed` path accepts encoded audio
bytes. Rust detects and decodes:

- RIFF/WAVE files.
- MP3 files that start with an ID3 tag or an MPEG frame sync header.

The streaming paths do not decode containers. They accept raw interleaved sample
arrays supplied by the caller:

- `pushSamples(samples: [Int16])`
- `pushSamplesF32(samples: [Float], channels: UInt16)`

The `Int16` push methods use the channel count captured when the streaming
handle was created. The `Float` push methods pass a channel count per call.

### WAV Decoding

`rust/crates/fingerprint-core/src/audio/wav.rs` implements a small RIFF/WAVE
parser. It scans chunks, requires `fmt ` and `data`, handles odd chunk padding,
and supports:

- PCM unsigned 8-bit.
- PCM signed 16-bit.
- PCM signed 24-bit.
- PCM signed 32-bit.
- IEEE float 32-bit.

Decoded samples are normalized to `f32` values. Unsupported bit depths or
formats return `UnsupportedFormat`. Truncated or malformed chunks return
`DecodeError`.

### MP3 Decoding

`rust/crates/fingerprint-core/src/audio/decoder.rs` uses Symphonia for MP3
decoding. The implementation caps inputs to reduce accidental memory blowups:

- Maximum MP3 input size: 128 MiB.
- Maximum decoded sample count: 64 Mi samples.

If probing fails, the public error is `UnsupportedFormat`. Decoder construction,
packet, and decode failures become typed decode errors where possible.

### Resampling and Downmixing

All fingerprinting runs at `TARGET_SAMPLE_RATE = 11_025` Hz.

`resample_to_mono` first converts interleaved channels to mono:

- One channel is copied directly.
- Multiple channels are averaged per frame.

If the source sample rate differs from 11,025 Hz, a simple linear interpolating
resampler is used. The output length is the floor of:

```text
input_frame_count / (source_sample_rate / 11_025)
```

This is intentionally lightweight and deterministic. It is not a high quality
anti-aliasing sample-rate converter.

## Fingerprint Algorithm

The fingerprint algorithm is implemented in
`rust/crates/fingerprint-core/src/fingerprint`.

The constants are:

- `FRAME_SIZE = 4_096` samples.
- `HOP_SIZE = 1_024` samples.
- `HASH_FRAME_COUNT = 8` chroma frames per hash.
- `HASH_STRIDE_FRAMES = 2` chroma frames between hash starts.
- `PITCH_CLASSES = 12`.
- `MIN_CHROMA_FREQUENCY_HZ = 28.0`.
- `MAX_CHROMA_FREQUENCY_HZ = 3_520.0`.
- `A4_HZ = 440.0`.
- `A4_PITCH_CLASS = 9.0`.
- `HASH_THRESHOLD = 0.05`.

At 11,025 Hz:

- One FFT frame covers about 371 ms.
- One hop covers about 93 ms.
- One 8-frame hash spans about 1.02 seconds of source audio.
- Hash starts are normally about 186 ms apart.

### Frame Processing

`FftProcessor` owns a reusable RustFFT plan, a Hann window, and a complex work
buffer. Each frame is:

1. Zero-padded or truncated to `FRAME_SIZE` through indexed reads.
2. Multiplied by the Hann window.
3. Transformed with a forward FFT.
4. Reduced to magnitudes for bins `0...FRAME_SIZE / 2`.

### Chroma Extraction

`chroma_from_magnitudes` maps FFT magnitudes into 12 pitch classes:

1. Convert each FFT bin index to frequency.
2. Ignore frequencies outside `[28 Hz, 3,520 Hz)`.
3. Convert frequency to a pitch class relative to A4 at 440 Hz.
4. Add squared magnitude into the pitch-class bin.
5. Average each pitch-class bin by the number of contributing FFT bins.
6. L2-normalize the 12-bin chroma vector when the norm is non-zero.

The result is one normalized `[f32; 12]` chroma vector per FFT frame.

### Hash Encoding

`encode_chroma_frames` groups chroma frames into 8-frame windows. Start offsets
advance by `HASH_STRIDE_FRAMES`, and the one-shot encoder also emits a final
hash at the last possible start when the stride does not land exactly on it.

`compute_hash` produces one `u32`:

- The low 28 bits encode whether selected chroma bins increased by more than
  `HASH_THRESHOLD` between consecutive chroma frames.
- At most 28 chroma-delta comparisons are used.
- The high nibble stores coarse energy from the first chroma frame:
  `clamp(sum(frame_0) * 4, 0, 15)`.

Inputs that do not produce at least 8 chroma frames return no hashes. A window
can pass the minimum `FRAME_SIZE` validation and still return an empty hash
array if it is too short to provide 8 chroma frames.

## Windowed Fingerprinting

`fingerprint_data_windowed` is the one-shot public path:

1. Decode audio bytes.
2. Validate non-zero sample rate and channel count.
3. Downmix and resample to 11,025 Hz.
4. Call `fingerprint_windows`.

`fingerprint_windows` converts requested durations to sample counts with:

```text
floor(milliseconds * 11_025 / 1_000)
```

It validates:

- `window_duration_ms` must convert to at least `FRAME_SIZE` samples.
- `window_interval_ms` must convert to a non-zero sample count.

If the input is shorter than one full window, it returns an empty array. For
each complete window it returns:

- `timestamp_ms`: the rounded timestamp of the window start.
- `duration_ms`: the requested window duration.
- `hashes`: the fingerprint for the window samples.

Timestamps use rounded sample-to-millisecond conversion:

```text
round(samples * 1_000 / 11_025)
```

## Streaming Fingerprinting

`StreamingFingerprinter` is for low-latency hash emission from raw PCM chunks.
It stores:

- Source sample rate and channel count.
- A queue of target-rate mono samples.
- A queue of chroma frames that have not yet been encoded.
- Total target-rate sample count for `duration_ms`.
- A reusable `FftProcessor`.

On each push:

1. Convert `Int16` to normalized `f32` when needed.
2. Downmix and resample to 11,025 Hz.
3. Append mono samples to the sample queue.
4. While at least one full frame is available, compute a chroma frame and pop
   `HOP_SIZE` samples.
5. While at least `HASH_FRAME_COUNT` chroma frames are available, compute one
   hash and pop `HASH_STRIDE_FRAMES` chroma frames.

`flush()` only emits hashes that can be made from already queued complete chroma
frames. It does not pad samples or synthesize partial hashes. The streaming path
therefore emits stride-aligned hashes as data arrives; the one-shot encoder can
add an extra terminal hash when the last possible hash start is not
stride-aligned.

`reset()` clears buffered samples, queued chroma frames, and duration state.

## Streaming Windowed Fingerprinting

`StreamingWindowedFingerprinter` emits full windows from raw PCM chunks. It
stores:

- Source sample rate and channel count.
- Requested window duration and interval.
- A queue of target-rate mono samples.
- The absolute sample index represented by the front of the queue.
- The next absolute sample index where a window should start.
- Total target-rate sample count for `duration_ms`.

On each push:

1. Convert, downmix, and resample incoming samples.
2. Append them to the target-rate sample queue.
3. Emit every complete window whose end is now available.
4. Fingerprint each window with the same one-shot `fingerprint_samples` path.
5. Advance `next_window_start` by the window interval.
6. Compact the queue by discarding samples that cannot be needed by future
   windows.

`flush()` emits only complete windows. It does not emit a partial final window.

## Serialization Format

Fingerprints serialize to a compact little-endian binary format:

```text
u32 duration_ms
u32 hash_count
u32 hashes[hash_count]
```

`fingerprint_to_bytes` caps the encoded hash count at `u32::MAX` and writes the
duration, count, and each hash as little-endian `u32` values.

`fingerprint_from_bytes` returns `nil`/`None` when:

- The input is shorter than the 8-byte header.
- The declared hash payload length overflows.
- The declared payload is not fully present.

Trailing bytes after the declared payload are ignored.

## Matching

`compare_hashes` compares two hash arrays at offset zero. It uses the shorter
input length and scores bit agreement across 32 bits per hash:

```text
matching_bits / (compared_hash_count * 32)
```

Empty inputs score `0.0`.

`compare_hashes_with_drift` searches for the best score over:

- No offset.
- Offsets where the first input starts later.
- Offsets where the second input starts later.

The drift search is capped by `max_drift` and both input lengths. The final
score is clamped to `[0.0, 1.0]`.

`CheckpointMatcher` stores checkpoints with timestamp, hash array, and duration.
Current scoring uses hashes only; duration is retained with the checkpoint but
does not affect ranking. Query results are sorted by:

1. Score descending.
2. Timestamp ascending.
3. Original insertion order.

`maxResults == 0` returns an empty result set.

## FFI Boundary and Memory Ownership

The C ABI uses plain structs with Rust-owned buffers:

- `FingerprintFfiBytes`
- `FingerprintFfiU32Array`
- `FingerprintFfiMatchArray`
- `FingerprintFfiWindowedArray`

Each buffer is returned as `ptr`, `len`, and `cap` from a Rust `Vec`. The Rust
side calls `std::mem::forget` before returning, transferring ownership to the
caller. The caller must release each returned value exactly once with the
matching free function:

- `fingerprint_ffi_free_bytes`
- `fingerprint_ffi_free_u32_array`
- `fingerprint_ffi_free_match_array`
- `fingerprint_ffi_free_windowed_array`

Windowed arrays own nested hash arrays. `fingerprint_ffi_free_windowed_array`
reconstructs the outer vector and then frees each nested hash vector.

The Swift wrapper follows a copy-then-free pattern:

1. Call FFI.
2. Copy the returned buffer into `Data` or a Swift array.
3. Defer the matching FFI free call.
4. Return Swift-owned values to the caller.

Opaque stateful Rust objects are returned as `void *` handles containing
`Box<Mutex<T>>`. This is used for:

- `CheckpointMatcher`
- `StreamingFingerprinter`
- `StreamingWindowedFingerprinter`

Operations on one handle are serialized by the mutex. A handle must not be freed
while another call using that same handle is in flight. Null handles and
poisoned mutexes generally produce fallback values such as `0`, an empty array,
or no-op behavior rather than throwing through the C ABI.

FFI functions that accept pointer/length pairs require either:

- A valid pointer for `len` elements.
- A null pointer with `len == 0`.

The Swift wrapper satisfies this by using `withUnsafeBufferPointer` and
`withUnsafeBytes`.

## Error Mapping

Rust errors are represented by `FingerprintError`:

- `DecodeError`
- `UnsupportedFormat`
- `InvalidInput`
- `IoError`

The FFI maps these to integer statuses:

```text
0 = success
1 = decode error
2 = unsupported format
3 = invalid input
4 = io error
```

On failure, FFI result structs include a Rust-owned UTF-8 message buffer. Swift
converts the message into `String`, frees the buffer, and throws the matching
`FingerprintError` case.

Constructors for streaming handles return `FingerprintFfiHandleResult`, so
invalid sample rates, invalid channel counts, too-short windows, and zero window
intervals become Swift throws.

## Build and Distribution

`scripts/build-rust-xcframework.sh` builds `fingerprint-ffi` for Apple targets:

- `aarch64-apple-ios`
- `aarch64-apple-ios-sim`
- `x86_64-apple-ios`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

Simulator and macOS slices are combined with `lipo` when both architectures are
available. The script then runs `xcodebuild -create-xcframework` with the Rust
static libraries and the public C headers.

By default, the script skips a target when the matching Rust standard library is
not installed. In CI and releases, `FINGERPRINT_REQUIRE_ALL_SLICES=1` makes any
missing Apple target fail the build.

`scripts/verify-xcframework.sh` verifies:

- `Info.plist` has the required library entries.
- iOS device includes `arm64`.
- iOS simulator includes `arm64` and `x86_64`.
- macOS includes `arm64` and `x86_64`.
- Each slice has `libfingerprint_ffi.a`, `FingerprintFFI.h`, and
  `module.modulemap`.
- The header exposes expected symbols such as `fingerprint_ffi_version`.
- The module map declares `module FingerprintFFI`.

## CI and Validation

The repository has separate Rust and Apple package validation.

Rust CI runs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo check --workspace --all-targets --all-features --locked
```

Fingerprint CI on macOS runs:

```bash
cargo test --manifest-path rust/Cargo.toml --workspace --locked
scripts/build-rust-xcframework.sh
scripts/verify-xcframework.sh Fingerprint.xcframework
swift test --filter 'FingerprintTests\.FingerprintTests'
xcodebuild -scheme Fingerprint -destination 'generic/platform=iOS' CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
xcodebuild -scheme Fingerprint -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
```

The Swift tests cover:

- Serialization layout and invalid decode cases.
- Hash comparison and drift.
- Checkpoint ordering.
- Streaming hash generation and duration tracking.
- Constructor validation errors.
- Windowed WAV fingerprinting.

The Rust tests cover the same behavior closer to the implementation, plus WAV
format variants, WAV error reporting, resampling details, and too-short sample
handling.

## Benchmarks

There are two benchmark paths:

- XCTest benchmarks in `Tests/FingerprintTests/FingerprintBenchmarkTests.swift`.
- Standalone JSON/CSV benchmark reports from
  `Benchmarks/FingerprintBenchmarkRunner`.

The benchmark workloads include:

- Small and large serialization round trips.
- Large equal and different hash comparisons.
- Drift comparison.
- Checkpoint add and query.
- Mono and stereo streaming.
- Windowed one-shot WAV fingerprinting.
- Windowed streaming with stereo resampling.
- MP3 unsupported-format fast path.

The standalone runner records environment metadata, Swift version, fingerprint
version, warmups, measured iterations, sample timings, and summary statistics.

## Important Behavioral Notes

- The target fingerprint sample rate is fixed at 11,025 Hz.
- One-shot encoded audio support is limited to WAV and MP3 detection paths.
- Streaming callers are responsible for decoding their own audio containers.
- Resampling is deterministic linear interpolation, not production-grade sample
  rate conversion.
- Window validation requires at least one FFT frame, but useful hashes require
  at least 8 chroma frames.
- `CheckpointMatcher` stores checkpoint duration but currently does not use it
  in scoring.
- FFI buffers are owned by Rust until Swift copies and frees them.
- Opaque handles are mutex-protected but must still obey the lifetime rule:
  do not free a handle while another call is using it.
