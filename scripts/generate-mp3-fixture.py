#!/usr/bin/env python3
"""Generate the committed MP3 golden fixture.

Renders the same deterministic reference signal the Rust golden tests use for
WAV (a rising chirp plus a steady tone under a slow tremolo), duplicates it
into two identical channels, and encodes it as 128 kbps CBR MP3 with LAME
(via the `lameenc` wheel).

The encoded bytes are committed at:

    rust/crates/fingerprint-core/tests/fixtures/reference.mp3
    Tests/FingerprintTests/Fixtures/reference.mp3

Both copies must stay byte-identical: the Rust and Swift golden tests pin
hashes for the same input. Regenerating the fixture (or bumping the LAME
version) changes the encoded bytes, so the golden hash constants in
rust/crates/fingerprint-core/tests/golden.rs and
Tests/FingerprintTests/FingerprintTests.swift must be regenerated in the same
change (see the golden.rs module docs).

    pip install lameenc
    python3 scripts/generate-mp3-fixture.py
"""

import math
import struct
from pathlib import Path

import lameenc

SAMPLE_RATE = 44_100
SECONDS = 2.0
CHANNELS = 2

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUTS = [
    REPO_ROOT / "rust" / "crates" / "fingerprint-core" / "tests" / "fixtures" / "reference.mp3",
    REPO_ROOT / "Tests" / "FingerprintTests" / "Fixtures" / "reference.mp3",
]


def reference_mono() -> list[float]:
    count = int(SAMPLE_RATE * SECONDS)
    samples = []
    for index in range(count):
        t = index / SAMPLE_RATE
        chirp = math.sin(2.0 * math.pi * (200.0 + 300.0 * t) * t)
        tone = 0.5 * math.sin(2.0 * math.pi * 660.0 * t)
        tremolo = 0.6 + 0.4 * math.sin(2.0 * math.pi * 3.0 * t)
        samples.append(0.4 * tremolo * (chirp + tone))
    return samples


def main() -> None:
    mono = reference_mono()
    interleaved = bytearray()
    for sample in mono:
        scaled = max(-32_768, min(32_767, int(sample * 32_767)))
        interleaved += struct.pack("<h", scaled) * CHANNELS

    encoder = lameenc.Encoder()
    encoder.set_bit_rate(128)
    encoder.set_in_sample_rate(SAMPLE_RATE)
    encoder.set_channels(CHANNELS)
    encoder.set_quality(2)
    encoded = encoder.encode(bytes(interleaved))
    encoded += encoder.flush()

    for output in OUTPUTS:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(encoded)
        print(f"wrote {len(encoded)} bytes to {output.relative_to(REPO_ROOT)}")


if __name__ == "__main__":
    main()
