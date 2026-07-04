# Fingerprint

A fast, deterministic **audio fingerprinting** library for iOS and macOS. The
public API is Swift; the heavy lifting — audio decoding, resampling, FFT,
chroma-based fingerprinting, serialization, and matching — runs in Rust and is
delivered as a prebuilt `Fingerprint.xcframework` binary target.

Use it to generate compact, content-based fingerprints of audio, compare them
for similarity (with tolerance for time drift), and locate matching checkpoints
inside a longer recording — one-shot from encoded files, or incrementally from a
live PCM stream.

- **Platforms:** iOS 13+, macOS 13+
- **Requirements:** Swift 6.2+ toolchain (Xcode 26+); the package uses the Swift 6 language mode
- **Distribution:** Swift Package Manager (binary xcframework + thin Swift facade)
- **License:** [Mozilla Public License 2.0](LICENSE.md)

---

## Contents

- [Why](#why)
- [How it works](#how-it-works)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Usage](#usage)
  - [One-shot fingerprinting (WAV / MP3)](#one-shot-fingerprinting-wav--mp3)
  - [Streaming fingerprinting (raw PCM)](#streaming-fingerprinting-raw-pcm)
  - [Comparing fingerprints](#comparing-fingerprints)
  - [Checkpoint matching](#checkpoint-matching)
  - [Serialization](#serialization)
- [API overview](#api-overview)
- [Building from source](#building-from-source)
- [Testing](#testing)
- [Benchmarks](#benchmarks)
- [Releasing](#releasing)
- [Repository layout](#repository-layout)
- [License](#license)

---

## Why

Audio fingerprinting turns a chunk of audio into a small array of `UInt32`
hashes that are robust to volume changes, re-encoding, and small timing shifts.
This library is designed to be:

- **Portable** — one Rust core, exported through a stable C ABI, packaged for
  Apple device, simulator, and macOS slices.
- **Deterministic** — the same audio always produces the same hashes. No
  hidden randomness, no platform-dependent math.
- **Value-typed and safe** — every Rust-owned buffer is copied into ordinary
  Swift `Data`/arrays and freed before you get it back. You never hold a
  borrowed pointer.
- **Flexible** — decode a whole file at once, or feed raw PCM as it arrives and
  get hashes/windows back incrementally with low latency.

## How it works

```text
Swift API
  └─ FingerprintFFI  (C module from Fingerprint.xcframework)
       └─ fingerprint-ffi   (extern "C" ABI: pointer/len/cap structs + opaque handles)
            └─ fingerprint-core   (pure Rust: decode → resample → FFT → chroma → hash → match)
```

All fingerprinting runs at a fixed **11,025 Hz** mono target rate. Audio is
downmixed and resampled with an anti-aliasing polyphase windowed-sinc filter,
transformed with a 4096-sample Hann-windowed real FFT (1024-sample hops),
reduced to a normalized 12-bin **chroma** vector per frame, and encoded into
32-bit hashes. Each hash packs 28 bits of chroma-delta information plus a
coarse energy nibble, spanning roughly one second of source audio.

Windowed fingerprints — one-shot and streaming — are cut from a single global
frame grid: overlapping windows share their FFT work, and streaming windows
match one-shot windows for identical input.

The full algorithm — constants, decoding paths, streaming state machines,
serialization format, matching, and the FFI memory-ownership contract — is
documented in **[docs/implementation.md](docs/implementation.md)**.

## Installation

Add the package to your `Package.swift`:

```swift
dependencies: [
    .package(url: "https://github.com/hbmartin/ios-audio-fingerprint.git", from: "0.1.0")
],
targets: [
    .target(
        name: "YourApp",
        dependencies: [
            .product(name: "Fingerprint", package: "ios-audio-fingerprint")
        ]
    )
]
```

Or in Xcode: **File → Add Package Dependencies…** and enter the repository URL.

SwiftPM resolves a released tag, downloads `Fingerprint.xcframework.zip`, and
verifies its checksum — no Rust toolchain required to *consume* the package.

## Quick start

```swift
import Fingerprint

// Fingerprint an encoded audio file in ~1s windows, every 500 ms.
let audio = try Data(contentsOf: url)                    // WAV or MP3 bytes
let windows = try Fingerprinter().fingerprintDataWindowed(
    data: audio,
    windowDurationMs: 1000,
    windowIntervalMs: 500
)

for w in windows {
    print("t=\(w.timestampMs)ms  \(w.hashes.count) hashes")
}
```

## Usage

### One-shot fingerprinting (WAV / MP3)

`Fingerprinter` accepts encoded audio bytes and auto-detects the container.
Supported inputs: **RIFF/WAVE** (PCM 8/16/24/32-bit and 32-bit IEEE float) and
**MP3** (ID3 tag or MPEG frame-sync). Unsupported formats throw
`FingerprintError.UnsupportedFormat`.

```swift
let fingerprinter = Fingerprinter()
let windows = try fingerprinter.fingerprintDataWindowed(
    data: audioData,
    windowDurationMs: 1000,   // must map to ≥ one FFT frame
    windowIntervalMs: 500     // hop between window starts
)
```

Each `WindowedFingerprint` carries `timestampMs`, `durationMs`, and its
`hashes: [UInt32]`.

### Streaming fingerprinting (raw PCM)

When you already have decoded PCM (e.g. from an audio unit or a network stream),
push samples as they arrive. Streaming paths **do not** decode containers — you
supply interleaved samples and the source sample rate / channel count up front.

Emit **hashes** incrementally:

```swift
let streamer = try StreamingFingerprinter(sampleRate: 44_100, channels: 2)

// Int16 uses the channel count from init; Float takes channels per call.
let hashes = streamer.pushSamples(samples: pcmInt16)          // [UInt32]
_ = streamer.pushSamplesF32(samples: pcmFloat, channels: 2)

let tail = streamer.flush()          // hashes from remaining complete frames
print("processed \(streamer.durationMs()) ms")
streamer.reset()                     // clear buffered state
```

Emit complete **windows** incrementally:

```swift
let windowed = try StreamingWindowedFingerprinter(
    sampleRate: 44_100,
    channels: 2,
    windowDurationMs: 1000,
    windowIntervalMs: 500
)

let ready = windowed.pushSamples(samples: pcmInt16)           // [WindowedFingerprint]
let last  = windowed.flush()                                  // only complete windows
```

### Comparing fingerprints

Scores are in `[0.0, 1.0]`, measuring bit agreement across the compared hashes.

```swift
let score = compareHashes(hashes1: a, hashes2: b)

// Tolerate up to `maxDrift` hash-positions of misalignment between the two.
let drifted = compareHashesWithDrift(hashes1: a, hashes2: b, maxDrift: 8)
```

### Checkpoint matching

`CheckpointMatcher` stores timestamped fingerprints and returns the best matches
for a query, sorted by score (desc), then timestamp (asc), then insertion order.

```swift
let matcher = CheckpointMatcher()
matcher.setDrift(maxDrift: 8)
matcher.add(timestamp: 12.5, hashes: checkpointHashes, duration: 1.0)

let matches = matcher.findTopMatches(queryHashes: query, maxResults: 5)
for m in matches {
    print("candidate at \(m.timestamp)s scored \(m.score)")
}

print(matcher.count())
matcher.clear()
```

### Serialization

Fingerprints serialize to a compact little-endian binary blob
(`u32 durationMs`, `u32 hashCount`, then the hashes):

```swift
let data = fingerprintToBytes(hashes: hashes, durationMs: 1000)   // Data
let round = fingerprintFromBytes(data: data)                      // FingerprintData?
// Malformed or truncated input returns nil rather than throwing.

print(fingerprintVersion())   // Rust core version string
```

## API overview

**Top-level functions**

| Function | Purpose |
| --- | --- |
| `fingerprintToBytes(hashes:durationMs:)` | Serialize hashes → `Data` |
| `fingerprintFromBytes(data:)` | Deserialize `Data` → `FingerprintData?` |
| `compareHashes(hashes1:hashes2:)` | Similarity score at offset 0 |
| `compareHashesWithDrift(hashes1:hashes2:maxDrift:)` | Best score across a drift window |
| `fingerprintVersion()` | Underlying Rust core version |

**Types**

- `FingerprintData` — `hashes: [UInt32]`, `durationMs: UInt32`
- `WindowedFingerprint` — `timestampMs`, `durationMs`, `hashes`
- `MatchResult` — `timestamp: Float`, `score: Float`
- `FingerprintError` — `.DecodeError`, `.UnsupportedFormat`, `.InvalidInput`, `.IoError` (each with a `message`)

**Classes**

- `Fingerprinter` — one-shot windowed fingerprinting of encoded audio
- `StreamingFingerprinter` — incremental hashes from raw PCM
- `StreamingWindowedFingerprinter` — incremental windows from raw PCM
- `CheckpointMatcher` — store checkpoints and rank query matches

## Building from source

You only need the Rust toolchain to *rebuild the binary*; consumers don't.

```bash
# Install Rust and the Apple targets you want to build.
rustup target add \
  aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios \
  aarch64-apple-darwin x86_64-apple-darwin

# Build the Rust FFI static libs and package them into the xcframework.
scripts/build-rust-xcframework.sh

# Verify slices, headers, module map, and exported symbols.
scripts/verify-xcframework.sh Fingerprint.xcframework
```

By default the build **skips** any Apple target whose Rust standard library
isn't installed (handy for local iteration). For a complete framework, require
every slice:

```bash
FINGERPRINT_REQUIRE_ALL_SLICES=1 scripts/build-rust-xcframework.sh
```

`Package.swift` points `FingerprintFFI` at the local `Fingerprint.xcframework`
during development; release tags repoint it at the uploaded GitHub Release asset
and checksum (see [Releasing](#releasing)).

## Testing

```bash
# Rust workspace (unit + behavior tests, closest to the implementation)
cargo test --manifest-path rust/Cargo.toml --workspace --locked

# Swift package tests (Swift Testing; serialization, comparison/drift, matching,
# streaming, constructor validation, windowed WAV fingerprinting, MP3 golden
# hashes pinned against the Rust suite, concurrent-push thread safety)
swift test --skip FingerprintBenchmarkTests
```

CI runs `cargo fmt`/`clippy`/`test` for Rust and, on macOS, lints the Swift
sources (`swift format lint --strict`), builds the xcframework, builds the
Swift package with warnings as errors, runs the Swift tests, and compiles the
package for iOS device and
simulator. See `.github/workflows/`.

The public API surface is pinned in `docs/public-api.txt`: CI fails if the
extracted surface (`python3 scripts/check-public-api.py`) drifts from that
baseline. Intentional API changes must regenerate it in the same PR
(`python3 scripts/check-public-api.py --update`), which makes every API
change an explicit, reviewable part of the diff.

## Benchmarks

Two paths are available:

- **XCTest benchmarks** in `Tests/FingerprintTests/FingerprintBenchmarkTests.swift`.
- **Standalone runner** that writes JSON/CSV reports with environment metadata,
  warmups, per-iteration timings, and summary statistics:

```bash
swift run FingerprintBenchmarkRunner
```

Workloads cover serialization round-trips, large equal/different comparisons,
drift comparison, checkpoint add/query, mono/stereo streaming, one-shot
windowed WAV, streaming windowed with resampling, and the MP3 unsupported-format
fast path.

## Releasing

Releases build the xcframework from a clean state, publish
`Fingerprint.xcframework.zip` as a GitHub Release asset, and create a tag whose
`Package.swift` references that URL + SwiftPM checksum. Trigger the
**Fingerprint Release** workflow with a `vMAJOR.MINOR.PATCH` version. The full
lifecycle, verification steps, and recovery procedures are in
**[docs/release.md](docs/release.md)**.

## Repository layout

| Path | What it is |
| --- | --- |
| `Package.swift` | SwiftPM manifest (library, benchmark exe, tests, binary target) |
| `Sources/Fingerprint/` | Thin Swift facade over the C ABI |
| `Fingerprint.xcframework/` | Prebuilt binary target (iOS device, iOS simulator, macOS) |
| `rust/crates/fingerprint-core/` | Pure Rust: decode, resample, fingerprint, match, serialize |
| `rust/crates/fingerprint-ffi/` | C ABI layer + public `FingerprintFFI.h` |
| `scripts/` | Build and verify the xcframework |
| `Tests/FingerprintTests/` | Swift behavior tests and XCTest benchmarks |
| `Benchmarks/FingerprintBenchmarkRunner/` | Standalone JSON/CSV benchmark runner |
| `docs/` | Implementation and release documentation |

## License

Licensed under the **Mozilla Public License, v. 2.0**. See [LICENSE.md](LICENSE.md).
