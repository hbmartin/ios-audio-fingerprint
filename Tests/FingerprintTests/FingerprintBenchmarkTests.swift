import Foundation
import XCTest
@testable import Fingerprint

final class FingerprintBenchmarkTests: XCTestCase {
    func testBenchmarkSerializationRoundTripSmallFingerprint() {
        let hashes = deterministicHashes(count: 128)
        var checksum: UInt64 = 0

        measure {
            let data = fingerprintToBytes(hashes: hashes, durationMs: 60_000)
            let decoded = fingerprintFromBytes(data: data)
            checksum &+= UInt64(decoded?.hashes.count ?? 0)
            checksum &+= UInt64(data.count)
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkSerializationRoundTripLargeFingerprint() {
        let hashes = deterministicHashes(count: 16_384)
        var checksum: UInt64 = 0

        measure {
            let data = fingerprintToBytes(hashes: hashes, durationMs: 3_600_000)
            let decoded = fingerprintFromBytes(data: data)
            checksum &+= UInt64(decoded?.hashes.last ?? 0)
            checksum &+= UInt64(data.count)
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkCompareHashesLargeEqualInputs() {
        let first = deterministicHashes(count: 65_536)
        let second = first
        var score: Float = 0

        measure {
            score += compareHashes(hashes1: first, hashes2: second)
        }

        XCTAssertGreaterThan(score, 0)
    }

    func testBenchmarkCompareHashesLargeDifferentInputs() {
        let first = deterministicHashes(count: 65_536)
        let second = deterministicHashes(count: 65_536, seed: 0xfeed_cafe)
        var score: Float = 0

        measure {
            score += compareHashes(hashes1: first, hashes2: second)
        }

        XCTAssertGreaterThanOrEqual(score, 0)
    }

    func testBenchmarkCompareHashesWithDrift() {
        let base = deterministicHashes(count: 8_192)
        let shifted = Array(repeating: UInt32(0xdead_beef), count: 64) + base
        var score: Float = 0

        measure {
            score += compareHashesWithDrift(hashes1: base, hashes2: shifted, maxDrift: 64)
        }

        XCTAssertGreaterThan(score, 0)
    }

    func testBenchmarkCheckpointMatcherAddAndQuery() {
        let checkpoints = (0..<1_000).map { index in
            (
                timestamp: Float(index) * 2.5,
                hashes: deterministicHashes(count: 256, seed: UInt32(index + 1)),
                duration: Float(30)
            )
        }
        let query = checkpoints[500].hashes
        var checksum: UInt64 = 0

        measure {
            let matcher = CheckpointMatcher.withDrift(maxDrift: 4)
            for checkpoint in checkpoints {
                matcher.add(timestamp: checkpoint.timestamp, hashes: checkpoint.hashes, duration: checkpoint.duration)
            }
            let matches = matcher.findTopMatches(queryHashes: query, maxResults: 10)
            checksum &+= UInt64(matches.count)
            checksum &+= UInt64(matcher.count())
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkStreamingFingerprinterMonoF32FiveSeconds() {
        let samples = compositeWave(sampleRate: 11_025, seconds: 5.0, channels: 1)
        var checksum: UInt64 = 0

        measure {
            let fingerprinter = StreamingFingerprinter(sampleRate: 11_025, channels: 1)
            let hashes = fingerprinter.pushSamplesF32(samples: samples, channels: 1) + fingerprinter.flush()
            checksum &+= UInt64(hashes.count)
            checksum &+= UInt64(fingerprinter.durationMs())
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkStreamingFingerprinterStereoF32ResampleFiveSeconds() {
        let samples = compositeWave(sampleRate: 44_100, seconds: 5.0, channels: 2)
        var checksum: UInt64 = 0

        measure {
            let fingerprinter = StreamingFingerprinter(sampleRate: 44_100, channels: 2)
            let hashes = fingerprinter.pushSamplesF32(samples: samples, channels: 2) + fingerprinter.flush()
            checksum &+= UInt64(hashes.count)
            checksum &+= UInt64(fingerprinter.durationMs())
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkWindowedWavFingerprintingSixSeconds() {
        let samples = compositeWave(sampleRate: 11_025, seconds: 6.0, channels: 1)
        let wav = waveFile(samples: samples, sampleRate: 11_025, channels: 1)
        var checksum: UInt64 = 0

        measure {
            let windows = try! Fingerprinter().fingerprintDataWindowed(
                data: wav,
                windowDurationMs: 2_000,
                windowIntervalMs: 500
            )
            checksum &+= UInt64(windows.count)
            checksum &+= UInt64(windows.reduce(0) { $0 + $1.hashes.count })
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkWindowedStreamingFingerprinterStereoResampleSixSeconds() {
        let samples = compositeWave(sampleRate: 44_100, seconds: 6.0, channels: 2)
        let chunkSize = 44_100
        var checksum: UInt64 = 0

        measure {
            let fingerprinter = StreamingWindowedFingerprinter(
                sampleRate: 44_100,
                channels: 2,
                windowDurationMs: 2_000,
                windowIntervalMs: 500
            )

            var windows: [WindowedFingerprint] = []
            var offset = 0
            while offset < samples.count {
                let end = min(offset + chunkSize, samples.count)
                windows.append(contentsOf: fingerprinter.pushSamplesF32(samples: Array(samples[offset..<end]), channels: 2))
                offset = end
            }
            windows.append(contentsOf: fingerprinter.flush())
            checksum &+= UInt64(windows.count)
            checksum &+= UInt64(fingerprinter.durationMs())
        }

        XCTAssertGreaterThan(checksum, 0)
    }

    func testBenchmarkMp3UnsupportedFastPath() {
        let mp3LikeData = Data([0xff, 0xfb, 0x90, 0x64] + Array(repeating: 0, count: 4_096))
        var unsupportedCount = 0

        measure {
            do {
                _ = try Fingerprinter().fingerprintDataWindowed(
                    data: mp3LikeData,
                    windowDurationMs: 2_000,
                    windowIntervalMs: 500
                )
            } catch FingerprintError.UnsupportedFormat {
                unsupportedCount += 1
            } catch {
                XCTFail("Unexpected error: \(error)")
            }
        }

        XCTAssertGreaterThan(unsupportedCount, 0)
    }

    private func deterministicHashes(count: Int, seed: UInt32 = 0x1234_5678) -> [UInt32] {
        var state = seed
        var hashes: [UInt32] = []
        hashes.reserveCapacity(count)
        for _ in 0..<count {
            state = state &* 1_664_525 &+ 1_013_904_223
            hashes.append(state)
        }
        return hashes
    }

    private func compositeWave(sampleRate: Int, seconds: Double, channels: Int) -> [Float] {
        let frameCount = Int(Double(sampleRate) * seconds)
        var samples: [Float] = []
        samples.reserveCapacity(frameCount * channels)
        for frame in 0..<frameCount {
            let time = Double(frame) / Double(sampleRate)
            let left = Float(
                sin(2.0 * Double.pi * 220.0 * time) * 0.35
                    + sin(2.0 * Double.pi * 440.0 * time) * 0.20
                    + sin(2.0 * Double.pi * 880.0 * time) * 0.10
            )
            if channels == 1 {
                samples.append(left)
            } else {
                let right = Float(
                    sin(2.0 * Double.pi * 330.0 * time) * 0.30
                        + sin(2.0 * Double.pi * 660.0 * time) * 0.15
                )
                samples.append(left)
                samples.append(right)
            }
        }
        return samples
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
            let clamped = max(-1, min(1, sample))
            let scaled = Int16(clamped * Float(Int16.max))
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
