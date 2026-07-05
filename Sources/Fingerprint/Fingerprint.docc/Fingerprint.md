# ``Fingerprint``

Fast, deterministic audio fingerprinting for iOS and macOS.

## Overview

Fingerprint turns audio into a compact array of `UInt32` hashes that are robust
to volume changes, re-encoding, and small timing shifts. The public API is
Swift; decoding, resampling, FFT analysis, hashing, serialization, and matching
run in Rust, shipped as a prebuilt binary framework.

All analysis happens at a fixed 11,025 Hz mono rate: input is downmixed,
low-pass filtered, and resampled, transformed with a 4,096-sample Hann-windowed
FFT every 1,024 samples, reduced to a normalized 12-bin chroma vector per
frame, and encoded into 32-bit hashes.

Fingerprint whole files with ``Fingerprinter``, or feed raw PCM as it arrives
with ``StreamingFingerprinter`` and ``StreamingWindowedFingerprinter`` — both
windowed paths cut their windows from the same analysis grid, so streaming and
one-shot results agree for identical input.

```swift
import Fingerprint

let audio = try Data(contentsOf: url)                    // WAV or MP3 bytes
let windows = try Fingerprinter().fingerprintDataWindowed(
    data: audio,
    windowDurationMs: 1000,
    windowIntervalMs: 500
)
```

## Topics

### Fingerprinting encoded audio

- ``Fingerprinter``
- ``FingerprinterProtocol``
- ``WindowedFingerprint``

### Fingerprinting live PCM

- ``StreamingFingerprinter``
- ``StreamingFingerprinterProtocol``
- ``StreamingWindowedFingerprinter``
- ``StreamingWindowedFingerprinterProtocol``

### Comparing and matching

- ``compareHashes(hashes1:hashes2:)``
- ``compareHashesWithDrift(hashes1:hashes2:maxDrift:)``
- ``CheckpointMatcher``
- ``CheckpointMatcherProtocol``
- ``MatchResult``

### Serialization

- ``fingerprintToBytes(hashes:durationMs:)``
- ``fingerprintFromBytes(data:)``
- ``FingerprintData``

### Errors and versioning

- ``FingerprintError``
- ``fingerprintVersion()``
