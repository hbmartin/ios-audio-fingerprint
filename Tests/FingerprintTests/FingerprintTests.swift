import Foundation
import Testing

@testable import Fingerprint

struct FingerprintTests {
    @Test
    func fingerprintSerializationMatchesRecoveredLayout() {
        let data = fingerprintToBytes(hashes: [0x11223344, 0xaabbccdd], durationMs: 1_234)

        #expect(
            Array(data) == [
                0xd2, 0x04, 0x00, 0x00,
                0x02, 0x00, 0x00, 0x00,
                0x44, 0x33, 0x22, 0x11,
                0xdd, 0xcc, 0xbb, 0xaa,
            ]
        )
        #expect(
            fingerprintFromBytes(data: data)
                == FingerprintData(hashes: [0x11223344, 0xaabbccdd], durationMs: 1_234)
        )
        #expect(fingerprintFromBytes(data: Data([0x01, 0x02, 0x03])) == nil)
        #expect(fingerprintFromBytes(data: Data([0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0])) == nil)
    }

    @Test
    func hashComparisonAndDrift() {
        #expect(compareHashes(hashes1: [0, UInt32.max], hashes2: [0, UInt32.max]) == 1)
        #expect(compareHashes(hashes1: [0], hashes2: [UInt32.max]) == 0)
        #expect(compareHashes(hashes1: [], hashes2: [1, 2]) == 0)

        #expect(compareHashes(hashes1: [1, 2, 3], hashes2: [9, 1, 2, 3]) < 1)
        #expect(compareHashesWithDrift(hashes1: [1, 2, 3], hashes2: [9, 1, 2, 3], maxDrift: 1) == 1)
    }

    @Test
    func checkpointMatcherSortsByScore() {
        let matcher = CheckpointMatcher.withDrift(maxDrift: 1)
        matcher.add(timestamp: 20, hashes: [0, 1, 2], duration: 3)
        matcher.add(timestamp: 10, hashes: [7, 0, 1, 2], duration: 4)

        let matches = matcher.findTopMatches(queryHashes: [0, 1, 2], maxResults: 2)

        #expect(matcher.count() == 2)
        #expect(matches.count == 2)
        #expect(matches[0].timestamp == 10)
        #expect(matches[0].score == 1)
    }

    @Test
    func streamingProducesHashesAndTracksDuration() throws {
        let fingerprinter = try StreamingFingerprinter(sampleRate: 11_025, channels: 1)
        let samples = sineWave(sampleRate: 11_025, seconds: 2.0, frequency: 440)

        let hashes = fingerprinter.pushSamplesF32(samples: samples, channels: 1) + fingerprinter.flush()

        #expect(!hashes.isEmpty)
        #expect(fingerprinter.durationMs() == 2_000)
    }

    /// The streaming classes advertise cross-domain sharing (the Rust side owns a `Mutex`), so
    /// exercise a shared instance from concurrent tasks. Runs under ThreadSanitizer in CI.
    @Test
    func concurrentPushesToSharedStreamerAreSafe() async throws {
        let fingerprinter = try StreamingFingerprinter(sampleRate: 44_100, channels: 1)
        let samples = [Float](repeating: 0.25, count: 8_192)

        await withTaskGroup(of: Void.self) { group in
            for _ in 0..<8 {
                group.addTask {
                    for _ in 0..<16 {
                        _ = fingerprinter.pushSamplesF32(samples: samples, channels: 1)
                    }
                }
            }
        }
        _ = fingerprinter.flush()

        #expect(fingerprinter.durationMs() > 0)
    }

    @Test
    func streamingInitializersThrowInvalidInputs() {
        #expect(throws: FingerprintError.InvalidInput(message: "sample rate must be greater than 0")) {
            _ = try StreamingFingerprinter(sampleRate: 0, channels: 1)
        }

        #expect(throws: FingerprintError.InvalidInput(message: "Window too short: 11 samples, need at least 4096")) {
            _ = try StreamingWindowedFingerprinter(
                sampleRate: 11_025,
                channels: 1,
                windowDurationMs: 1,
                windowIntervalMs: 500
            )
        }
    }

    /// Golden hashes for `Fixtures/reference.mp3`, pinned to the same values as
    /// `decoded_mp3_windows_match_golden` in the Rust golden suite
    /// (rust/crates/fingerprint-core/tests/golden.rs). The constants were
    /// captured on aarch64; regenerate them together with the Rust goldens
    /// after an intentional algorithm change (`emit_golden`).
    private static let goldenMp3Hashes: [UInt32] = [
        0xa070_01fc, 0x8ec0_3e00, 0x8001_801e, 0x7003_0018, 0x680e_0030, 0x6030_0188, 0x71c0_0600,
    ]

    @Test
    func mp3FixtureMatchesRustGoldenHashes() throws {
        let url = try #require(
            Bundle.module.url(forResource: "reference", withExtension: "mp3", subdirectory: "Fixtures")
        )
        let data = try Data(contentsOf: url)
        let windows = try Fingerprinter().fingerprintDataWindowed(
            data: data,
            windowDurationMs: 1_500,
            windowIntervalMs: 500
        )
        let hashes = windows.flatMap(\.hashes)

        #expect(windows.count == 2)
        #expect(hashes.count == Self.goldenMp3Hashes.count)
        // rustfft may pick different SIMD kernels per architecture, so require
        // near-identity everywhere and exact equality on the reference
        // architecture (mirroring the Rust golden suite's policy).
        #expect(compareHashes(hashes1: hashes, hashes2: Self.goldenMp3Hashes) >= 0.99)
        #if arch(arm64)
            #expect(hashes == Self.goldenMp3Hashes)
        #endif
    }

    @Test
    func windowedWavFingerprinting() throws {
        let samples = sineWave(sampleRate: 11_025, seconds: 2.0, frequency: 440)
        let wav = waveFile(samples: samples, sampleRate: 11_025, channels: 1)
        let windows = try Fingerprinter().fingerprintDataWindowed(
            data: wav,
            windowDurationMs: 1_500,
            windowIntervalMs: 500
        )

        #expect(windows.count == 2)
        #expect(windows[0].timestampMs == 0)
        #expect(windows[1].timestampMs == 500)
        #expect(!windows[0].hashes.isEmpty)
    }

    private func sineWave(sampleRate: Int, seconds: Double, frequency: Double) -> [Float] {
        let count = Int(Double(sampleRate) * seconds)
        return (0..<count).map { index in
            Float(sin((2.0 * Double.pi * frequency * Double(index)) / Double(sampleRate)) * 0.5)
        }
    }

    private func waveFile(samples: [Float], sampleRate: UInt32, channels: UInt16) -> Data {
        var bytes: [UInt8] = []
        let dataSize = UInt32(samples.count * 2)
        appendAscii("RIFF", to: &bytes)
        appendUInt32(36 + dataSize, to: &bytes)
        appendAscii("WAVE", to: &bytes)
        appendAscii("fmt ", to: &bytes)
        appendUInt32(16, to: &bytes)
        appendUInt16(1, to: &bytes)
        appendUInt16(channels, to: &bytes)
        appendUInt32(sampleRate, to: &bytes)
        appendUInt32(sampleRate * UInt32(channels) * 2, to: &bytes)
        appendUInt16(channels * 2, to: &bytes)
        appendUInt16(16, to: &bytes)
        appendAscii("data", to: &bytes)
        appendUInt32(dataSize, to: &bytes)
        for sample in samples {
            let scaled = Int16(max(Int16.min, min(Int16.max, Int16(sample * 32767))))
            appendUInt16(UInt16(bitPattern: scaled), to: &bytes)
        }
        return Data(bytes)
    }

    private func appendAscii(_ value: String, to bytes: inout [UInt8]) {
        bytes.append(contentsOf: value.utf8)
    }

    private func appendUInt16(_ value: UInt16, to bytes: inout [UInt8]) {
        bytes.append(UInt8(value & 0xff))
        bytes.append(UInt8((value >> 8) & 0xff))
    }

    private func appendUInt32(_ value: UInt32, to bytes: inout [UInt8]) {
        bytes.append(UInt8(value & 0xff))
        bytes.append(UInt8((value >> 8) & 0xff))
        bytes.append(UInt8((value >> 16) & 0xff))
        bytes.append(UInt8((value >> 24) & 0xff))
    }
}
