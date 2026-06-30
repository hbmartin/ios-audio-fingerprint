import Fingerprint
import Foundation

struct BenchmarkCase {
    let name: String
    let category: String
    let workload: String
    let run: () throws -> UInt64
}

struct BenchmarkResult: Codable {
    let name: String
    let category: String
    let workload: String
    let iterations: Int
    let warmups: Int
    let checksum: UInt64
    let minMs: Double
    let medianMs: Double
    let meanMs: Double
    let p90Ms: Double
    let maxMs: Double
    let standardDeviationMs: Double
    let samplesMs: [Double]
}

struct BenchmarkReport: Codable {
    let label: String
    let timestamp: String
    let configuration: String
    let fingerprintVersion: String
    let swiftVersion: String
    let system: SystemInfo
    let results: [BenchmarkResult]
}

struct SystemInfo: Codable {
    let operatingSystem: String
    let processorCount: Int
    let activeProcessorCount: Int
    let physicalMemoryBytes: UInt64
}

struct Options {
    var outputDir = "codex-analysis/benchmarks"
    var label: String
    var iterations = 15
    var warmups = 5

    init(arguments: [String]) throws {
        let defaultLabelFormatter = DateFormatter()
        defaultLabelFormatter.calendar = Calendar(identifier: .gregorian)
        defaultLabelFormatter.locale = Locale(identifier: "en_US_POSIX")
        defaultLabelFormatter.timeZone = TimeZone(secondsFromGMT: 0)
        defaultLabelFormatter.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        label = "baseline-\(defaultLabelFormatter.string(from: Date()))"

        var index = 1
        while index < arguments.count {
            switch arguments[index] {
            case "--output-dir":
                outputDir = try Self.value(after: &index, in: arguments, option: "--output-dir")
            case "--label":
                label = try Self.value(after: &index, in: arguments, option: "--label")
            case "--iterations":
                iterations = try Self.positiveInt(after: &index, in: arguments, option: "--iterations")
            case "--warmups":
                warmups = try Self.nonNegativeInt(after: &index, in: arguments, option: "--warmups")
            case "--help", "-h":
                print("""
                Usage: swift run -c release FingerprintBenchmarkRunner [options]

                Options:
                  --output-dir <path>   Directory that will receive a label subdirectory.
                  --label <name>        Baseline label. Defaults to a UTC timestamp.
                  --iterations <count>  Measured iterations per case. Defaults to 15.
                  --warmups <count>     Untimed warmup iterations per case. Defaults to 5.
                """)
                Foundation.exit(0)
            default:
                throw BenchmarkError.invalidArgument("Unknown option: \(arguments[index])")
            }
            index += 1
        }
    }

    private static func value(after index: inout Int, in arguments: [String], option: String) throws -> String {
        let valueIndex = index + 1
        guard valueIndex < arguments.count else {
            throw BenchmarkError.invalidArgument("Missing value for \(option).")
        }
        index = valueIndex
        return arguments[valueIndex]
    }

    private static func positiveInt(after index: inout Int, in arguments: [String], option: String) throws -> Int {
        let raw = try value(after: &index, in: arguments, option: option)
        guard let value = Int(raw), value > 0 else {
            throw BenchmarkError.invalidArgument("\(option) must be a positive integer.")
        }
        return value
    }

    private static func nonNegativeInt(after index: inout Int, in arguments: [String], option: String) throws -> Int {
        let raw = try value(after: &index, in: arguments, option: option)
        guard let value = Int(raw), value >= 0 else {
            throw BenchmarkError.invalidArgument("\(option) must be a non-negative integer.")
        }
        return value
    }
}

enum BenchmarkError: Error, CustomStringConvertible {
    case invalidArgument(String)
    case expectedUnsupportedFormat

    var description: String {
        switch self {
        case let .invalidArgument(message):
            return message
        case .expectedUnsupportedFormat:
            return "MP3 fast-path benchmark did not throw FingerprintError.UnsupportedFormat."
        }
    }
}

let options: Options
do {
    options = try Options(arguments: CommandLine.arguments)
} catch {
    FileHandle.standardError.write(Data("Error: \(error)\n".utf8))
    Foundation.exit(2)
}

do {
    let cases = makeBenchmarkCases()
    let report = try runBenchmarks(cases: cases, options: options)
    let outputURL = try write(report: report, outputDir: options.outputDir, label: options.label)
    print("Wrote benchmark results to \(outputURL.path)")
    print("Fastest median: \(report.results.min { $0.medianMs < $1.medianMs }?.name ?? "n/a")")
    print("Slowest median: \(report.results.max { $0.medianMs < $1.medianMs }?.name ?? "n/a")")
} catch {
    FileHandle.standardError.write(Data("Benchmark failed: \(error)\n".utf8))
    Foundation.exit(1)
}

func makeBenchmarkCases() -> [BenchmarkCase] {
    let smallHashes = deterministicHashes(count: 128)
    let largeHashes = deterministicHashes(count: 16_384)
    let compareFirst = deterministicHashes(count: 65_536)
    let compareDifferent = deterministicHashes(count: 65_536, seed: 0xfeed_cafe)
    let driftBase = deterministicHashes(count: 8_192)
    let driftShifted = Array(repeating: UInt32(0xdead_beef), count: 64) + driftBase
    let checkpoints = (0..<1_000).map { index in
        (
            timestamp: Float(index) * 2.5,
            hashes: deterministicHashes(count: 256, seed: UInt32(index + 1)),
            duration: Float(30)
        )
    }
    let query = checkpoints[500].hashes
    let monoFiveSeconds = compositeWave(sampleRate: 11_025, seconds: 5.0, channels: 1)
    let stereoFiveSeconds = compositeWave(sampleRate: 44_100, seconds: 5.0, channels: 2)
    let monoSixSeconds = compositeWave(sampleRate: 11_025, seconds: 6.0, channels: 1)
    let sixSecondWav = waveFile(samples: monoSixSeconds, sampleRate: 11_025, channels: 1)
    let stereoSixSeconds = compositeWave(sampleRate: 44_100, seconds: 6.0, channels: 2)
    let mp3LikeData = Data([0xff, 0xfb, 0x90, 0x64] + Array(repeating: 0, count: 4_096))

    return [
        BenchmarkCase(
            name: "serialization_round_trip_small_fingerprint",
            category: "serialization",
            workload: "128 deterministic hashes, 60s duration"
        ) {
            let data = fingerprintToBytes(hashes: smallHashes, durationMs: 60_000)
            let decoded = fingerprintFromBytes(data: data)
            return UInt64(data.count) &+ UInt64(decoded?.hashes.count ?? 0)
        },
        BenchmarkCase(
            name: "serialization_round_trip_large_fingerprint",
            category: "serialization",
            workload: "16,384 deterministic hashes, 60m duration"
        ) {
            let data = fingerprintToBytes(hashes: largeHashes, durationMs: 3_600_000)
            let decoded = fingerprintFromBytes(data: data)
            return UInt64(data.count) &+ UInt64(decoded?.hashes.last ?? 0)
        },
        BenchmarkCase(
            name: "compare_hashes_large_equal_inputs",
            category: "comparison",
            workload: "65,536 hashes compared against identical input"
        ) {
            UInt64(compareHashes(hashes1: compareFirst, hashes2: compareFirst) * 1_000_000)
        },
        BenchmarkCase(
            name: "compare_hashes_large_different_inputs",
            category: "comparison",
            workload: "65,536 hashes compared against a different deterministic input"
        ) {
            UInt64(compareHashes(hashes1: compareFirst, hashes2: compareDifferent) * 1_000_000)
        },
        BenchmarkCase(
            name: "compare_hashes_with_drift",
            category: "comparison",
            workload: "8,192 hashes searched with a 64-hash offset and max drift 64"
        ) {
            UInt64(compareHashesWithDrift(hashes1: driftBase, hashes2: driftShifted, maxDrift: 64) * 1_000_000)
        },
        BenchmarkCase(
            name: "checkpoint_matcher_add_and_query",
            category: "matching",
            workload: "1,000 checkpoints, 256 hashes each, drift 4, top 10 query"
        ) {
            let matcher = CheckpointMatcher.withDrift(maxDrift: 4)
            for checkpoint in checkpoints {
                matcher.add(timestamp: checkpoint.timestamp, hashes: checkpoint.hashes, duration: checkpoint.duration)
            }
            let matches = matcher.findTopMatches(queryHashes: query, maxResults: 10)
            return UInt64(matches.count) &+ UInt64(matcher.count())
        },
        BenchmarkCase(
            name: "streaming_fingerprinter_mono_f32_five_seconds",
            category: "fingerprinting",
            workload: "5s mono Float32 synthetic tone at 11,025 Hz"
        ) {
            let fingerprinter = StreamingFingerprinter(sampleRate: 11_025, channels: 1)
            let hashes = fingerprinter.pushSamplesF32(samples: monoFiveSeconds, channels: 1) + fingerprinter.flush()
            return UInt64(hashes.count) &+ UInt64(fingerprinter.durationMs())
        },
        BenchmarkCase(
            name: "streaming_fingerprinter_stereo_f32_resample_five_seconds",
            category: "fingerprinting",
            workload: "5s stereo Float32 synthetic tone at 44,100 Hz, downmixed and resampled"
        ) {
            let fingerprinter = StreamingFingerprinter(sampleRate: 44_100, channels: 2)
            let hashes = fingerprinter.pushSamplesF32(samples: stereoFiveSeconds, channels: 2) + fingerprinter.flush()
            return UInt64(hashes.count) &+ UInt64(fingerprinter.durationMs())
        },
        BenchmarkCase(
            name: "windowed_wav_fingerprinting_six_seconds",
            category: "fingerprinting",
            workload: "6s mono 16-bit PCM WAV at 11,025 Hz, 2s windows every 500ms"
        ) {
            let windows = try Fingerprinter().fingerprintDataWindowed(
                data: sixSecondWav,
                windowDurationMs: 2_000,
                windowIntervalMs: 500
            )
            return UInt64(windows.count) &+ UInt64(windows.reduce(0) { $0 + $1.hashes.count })
        },
        BenchmarkCase(
            name: "streaming_windowed_fingerprinter_stereo_resample_six_seconds",
            category: "fingerprinting",
            workload: "6s stereo Float32 synthetic tone at 44,100 Hz in 44,100-sample chunks"
        ) {
            let fingerprinter = StreamingWindowedFingerprinter(
                sampleRate: 44_100,
                channels: 2,
                windowDurationMs: 2_000,
                windowIntervalMs: 500
            )
            var windows: [WindowedFingerprint] = []
            var offset = 0
            let chunkSize = 44_100
            while offset < stereoSixSeconds.count {
                let end = min(offset + chunkSize, stereoSixSeconds.count)
                windows.append(contentsOf: fingerprinter.pushSamplesF32(samples: Array(stereoSixSeconds[offset..<end]), channels: 2))
                offset = end
            }
            windows.append(contentsOf: fingerprinter.flush())
            return UInt64(windows.count) &+ UInt64(fingerprinter.durationMs())
        },
        BenchmarkCase(
            name: "mp3_unsupported_fast_path",
            category: "format",
            workload: "4,100-byte MP3-like payload, unsupported format error path"
        ) {
            do {
                _ = try Fingerprinter().fingerprintDataWindowed(
                    data: mp3LikeData,
                    windowDurationMs: 2_000,
                    windowIntervalMs: 500
                )
            } catch FingerprintError.UnsupportedFormat {
                return 1
            }
            throw BenchmarkError.expectedUnsupportedFormat
        },
    ]
}

func runBenchmarks(cases: [BenchmarkCase], options: Options) throws -> BenchmarkReport {
    var results: [BenchmarkResult] = []
    results.reserveCapacity(cases.count)

    for benchmark in cases {
        for _ in 0..<options.warmups {
            _ = try benchmark.run()
        }

        var samples: [Double] = []
        samples.reserveCapacity(options.iterations)
        var checksum: UInt64 = 0

        for _ in 0..<options.iterations {
            let start = DispatchTime.now().uptimeNanoseconds
            checksum &+= try benchmark.run()
            let end = DispatchTime.now().uptimeNanoseconds
            samples.append(Double(end - start) / 1_000_000.0)
        }

        let stats = statistics(samples)
        let result = BenchmarkResult(
            name: benchmark.name,
            category: benchmark.category,
            workload: benchmark.workload,
            iterations: options.iterations,
            warmups: options.warmups,
            checksum: checksum,
            minMs: stats.min,
            medianMs: stats.median,
            meanMs: stats.mean,
            p90Ms: stats.p90,
            maxMs: stats.max,
            standardDeviationMs: stats.standardDeviation,
            samplesMs: samples
        )
        results.append(result)
        print("\(benchmark.name): median \(format(stats.median)) ms, p90 \(format(stats.p90)) ms")
    }

    let timestamp = ISO8601DateFormatter().string(from: Date())
    return BenchmarkReport(
        label: options.label,
        timestamp: timestamp,
        configuration: buildConfiguration(),
        fingerprintVersion: fingerprintVersion(),
        swiftVersion: swiftVersion(),
        system: systemInfo(),
        results: results
    )
}

func write(report: BenchmarkReport, outputDir: String, label: String) throws -> URL {
    let baseURL = URL(fileURLWithPath: outputDir, isDirectory: true)
    let outputURL = baseURL.appendingPathComponent(label, isDirectory: true)
    try FileManager.default.createDirectory(at: outputURL, withIntermediateDirectories: true)

    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    try encoder.encode(report).write(to: outputURL.appendingPathComponent("results.json"), options: .atomic)
    try csv(for: report).write(to: outputURL.appendingPathComponent("results.csv"), atomically: true, encoding: .utf8)
    try markdown(for: report).write(to: outputURL.appendingPathComponent("summary.md"), atomically: true, encoding: .utf8)
    return outputURL
}

func statistics(_ samples: [Double]) -> (min: Double, median: Double, mean: Double, p90: Double, max: Double, standardDeviation: Double) {
    let sorted = samples.sorted()
    let mean = samples.reduce(0, +) / Double(samples.count)
    let variance = samples.reduce(0) { $0 + pow($1 - mean, 2) } / Double(samples.count)
    return (
        min: sorted.first ?? 0,
        median: percentile(sorted, 0.50),
        mean: mean,
        p90: percentile(sorted, 0.90),
        max: sorted.last ?? 0,
        standardDeviation: sqrt(variance)
    )
}

func percentile(_ sorted: [Double], _ percentile: Double) -> Double {
    guard !sorted.isEmpty else {
        return 0
    }
    guard sorted.count > 1 else {
        return sorted[0]
    }
    let position = percentile * Double(sorted.count - 1)
    let lower = Int(position.rounded(.down))
    let upper = Int(position.rounded(.up))
    if lower == upper {
        return sorted[lower]
    }
    let fraction = position - Double(lower)
    return sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

func csv(for report: BenchmarkReport) -> String {
    var lines = [
        "name,category,iterations,warmups,checksum,min_ms,median_ms,mean_ms,p90_ms,max_ms,standard_deviation_ms,workload",
    ]
    for result in report.results {
        lines.append(
            [
                csvEscape(result.name),
                csvEscape(result.category),
                String(result.iterations),
                String(result.warmups),
                String(result.checksum),
                format(result.minMs),
                format(result.medianMs),
                format(result.meanMs),
                format(result.p90Ms),
                format(result.maxMs),
                format(result.standardDeviationMs),
                csvEscape(result.workload),
            ].joined(separator: ",")
        )
    }
    return lines.joined(separator: "\n") + "\n"
}

func markdown(for report: BenchmarkReport) -> String {
    var output = """
    # Fingerprint Benchmark Results

    - Label: `\(report.label)`
    - Timestamp: `\(report.timestamp)`
    - Configuration: `\(report.configuration)`
    - Fingerprint version: `\(report.fingerprintVersion)`
    - Swift: `\(report.swiftVersion)`
    - OS: `\(report.system.operatingSystem)`
    - CPUs: `\(report.system.activeProcessorCount)` active / `\(report.system.processorCount)` total
    - Memory: `\(report.system.physicalMemoryBytes)` bytes
    - Iterations: `\(report.results.first?.iterations ?? 0)` measured, `\(report.results.first?.warmups ?? 0)` warmups per benchmark

    | Benchmark | Category | Median ms | Mean ms | P90 ms | Min ms | Max ms | StdDev ms |
    | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
    """
    for result in report.results.sorted(by: { $0.medianMs > $1.medianMs }) {
        output += "\n| `\(result.name)` | \(result.category) | \(format(result.medianMs)) | \(format(result.meanMs)) | \(format(result.p90Ms)) | \(format(result.minMs)) | \(format(result.maxMs)) | \(format(result.standardDeviationMs)) |"
    }
    output += "\n\n## Workloads\n\n"
    for result in report.results {
        output += "- `\(result.name)`: \(result.workload)\n"
    }
    return output
}

func csvEscape(_ value: String) -> String {
    if value.contains(",") || value.contains("\"") || value.contains("\n") {
        return "\"\(value.replacingOccurrences(of: "\"", with: "\"\""))\""
    }
    return value
}

func format(_ value: Double) -> String {
    String(format: "%.6f", value)
}

func buildConfiguration() -> String {
    #if DEBUG
    return "debug"
    #else
    return "release"
    #endif
}

func swiftVersion() -> String {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/xcrun")
    process.arguments = ["swift", "--version"]

    let pipe = Pipe()
    process.standardOutput = pipe
    process.standardError = Pipe()

    do {
        try process.run()
        process.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return String(data: data, encoding: .utf8)?
            .split(separator: "\n")
            .first
            .map(String.init) ?? "unknown"
    } catch {
        return "unknown"
    }
}

func systemInfo() -> SystemInfo {
    let processInfo = ProcessInfo.processInfo
    return SystemInfo(
        operatingSystem: processInfo.operatingSystemVersionString,
        processorCount: processInfo.processorCount,
        activeProcessorCount: processInfo.activeProcessorCount,
        physicalMemoryBytes: processInfo.physicalMemory
    )
}

func deterministicHashes(count: Int, seed: UInt32 = 0x1234_5678) -> [UInt32] {
    var state = seed
    var hashes: [UInt32] = []
    hashes.reserveCapacity(count)
    for _ in 0..<count {
        state = state &* 1_664_525 &+ 1_013_904_223
        hashes.append(state)
    }
    return hashes
}

func compositeWave(sampleRate: Int, seconds: Double, channels: Int) -> [Float] {
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

func waveFile(samples: [Float], sampleRate: UInt32, channels: UInt16) -> Data {
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

func appendAscii(_ value: String, to bytes: inout [UInt8]) {
    bytes.append(contentsOf: value.utf8)
}

func appendUInt16(_ value: UInt16, to bytes: inout [UInt8]) {
    bytes.append(UInt8(value & 0xff))
    bytes.append(UInt8((value >> 8) & 0xff))
}

func appendUInt32(_ value: UInt32, to bytes: inout [UInt8]) {
    bytes.append(UInt8(value & 0xff))
    bytes.append(UInt8((value >> 8) & 0xff))
    bytes.append(UInt8((value >> 16) & 0xff))
    bytes.append(UInt8((value >> 24) & 0xff))
}
