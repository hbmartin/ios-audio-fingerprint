import Foundation

public protocol CheckpointMatcherProtocol: AnyObject {
    func add(timestamp: Float, hashes: [UInt32], duration: Float)
    func clear()
    func count() -> UInt32
    func findTopMatches(queryHashes: [UInt32], maxResults: UInt32) -> [MatchResult]
    func setDrift(maxDrift: UInt32)
}

open class CheckpointMatcher: CheckpointMatcherProtocol {
    public struct NoPointer {
        public init() {}
    }

    private struct Checkpoint {
        let timestamp: Float
        let hashes: [UInt32]
        let duration: Float
    }

    private var checkpoints: [Checkpoint]
    private var maximumDrift: UInt32

    public required init(unsafeFromRawPointer _: UnsafeMutableRawPointer) {
        checkpoints = []
        maximumDrift = 0
    }

    public init(noPointer _: NoPointer) {
        checkpoints = []
        maximumDrift = 0
    }

    public convenience init() {
        self.init(noPointer: NoPointer())
    }

    private init(maxDrift: UInt32) {
        checkpoints = []
        maximumDrift = maxDrift
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    public static func withDrift(maxDrift: UInt32) -> CheckpointMatcher {
        CheckpointMatcher(maxDrift: maxDrift)
    }

    open func add(timestamp: Float, hashes: [UInt32], duration: Float) {
        checkpoints.append(Checkpoint(timestamp: timestamp, hashes: hashes, duration: duration))
    }

    open func clear() {
        checkpoints.removeAll(keepingCapacity: true)
    }

    open func count() -> UInt32 {
        UInt32(clamping: checkpoints.count)
    }

    open func findTopMatches(queryHashes: [UInt32], maxResults: UInt32) -> [MatchResult] {
        checkpoints
            .map { checkpoint in
                MatchResult(
                    timestamp: checkpoint.timestamp,
                    score: FingerprintCore.compareWithDrift(queryHashes, checkpoint.hashes, maximumDrift)
                )
            }
            .sorted {
                if $0.score == $1.score {
                    return $0.timestamp < $1.timestamp
                }
                return $0.score > $1.score
            }
            .prefix(Int(maxResults))
            .map { $0 }
    }

    open func setDrift(maxDrift: UInt32) {
        maximumDrift = maxDrift
    }
}

public protocol FingerprinterProtocol: AnyObject {
    func fingerprintDataWindowed(data: Data, windowDurationMs: UInt32, windowIntervalMs: UInt32) throws -> [WindowedFingerprint]
}

open class Fingerprinter: FingerprinterProtocol {
    public struct NoPointer {
        public init() {}
    }

    public required init(unsafeFromRawPointer _: UnsafeMutableRawPointer) {}
    public init(noPointer _: NoPointer) {}
    public init() {}

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    open func fingerprintDataWindowed(data: Data, windowDurationMs: UInt32, windowIntervalMs: UInt32) throws -> [WindowedFingerprint] {
        let audio = try FingerprintCore.decodeAudioFile(data)
        let samples = FingerprintCore.resampleToMono(samples: audio.samples, sampleRate: audio.sampleRate, channels: audio.channels)
        return try FingerprintCore.fingerprintWindows(
            samples: samples,
            windowDurationMs: windowDurationMs,
            windowIntervalMs: windowIntervalMs
        )
    }
}

public protocol StreamingFingerprinterProtocol: AnyObject {
    func durationMs() -> UInt32
    func flush() -> [UInt32]
    func pushSamples(samples: [Int16]) -> [UInt32]
    func pushSamplesF32(samples: [Float], channels: UInt16) -> [UInt32]
    func reset()
}

open class StreamingFingerprinter: StreamingFingerprinterProtocol {
    public struct NoPointer {
        public init() {}
    }

    private let sampleRate: UInt32
    private let channels: UInt16
    private var buffer: [Float]
    private var chromaFrames: [[Float]]
    private var totalSamplesAtTargetRate: Int

    public required init(unsafeFromRawPointer _: UnsafeMutableRawPointer) {
        sampleRate = FingerprintCore.targetSampleRate
        channels = 1
        buffer = []
        chromaFrames = []
        totalSamplesAtTargetRate = 0
    }

    public init(noPointer _: NoPointer) {
        sampleRate = FingerprintCore.targetSampleRate
        channels = 1
        buffer = []
        chromaFrames = []
        totalSamplesAtTargetRate = 0
    }

    public init(sampleRate: UInt32, channels: UInt16) {
        self.sampleRate = sampleRate
        self.channels = channels
        buffer = []
        chromaFrames = []
        totalSamplesAtTargetRate = 0
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    open func durationMs() -> UInt32 {
        UInt32((UInt64(totalSamplesAtTargetRate) * 1000) / UInt64(FingerprintCore.targetSampleRate))
    }

    open func flush() -> [UInt32] {
        emitAvailableHashes()
    }

    open func pushSamples(samples: [Int16]) -> [UInt32] {
        let floats = samples.map { Float($0) / 32768.0 }
        let mono = FingerprintCore.resampleToMono(samples: floats, sampleRate: sampleRate, channels: channels)
        buffer.append(contentsOf: mono)
        totalSamplesAtTargetRate += mono.count
        processBufferedFrames()
        return emitAvailableHashes()
    }

    open func pushSamplesF32(samples: [Float], channels: UInt16) -> [UInt32] {
        let mono = FingerprintCore.resampleToMono(samples: samples, sampleRate: sampleRate, channels: channels)
        buffer.append(contentsOf: mono)
        totalSamplesAtTargetRate += mono.count
        processBufferedFrames()
        return emitAvailableHashes()
    }

    open func reset() {
        buffer.removeAll(keepingCapacity: true)
        chromaFrames.removeAll(keepingCapacity: true)
        totalSamplesAtTargetRate = 0
    }

    private func processBufferedFrames() {
        while buffer.count >= FingerprintCore.frameSize {
            let frame = Array(buffer[..<FingerprintCore.frameSize])
            chromaFrames.append(FingerprintCore.chroma(for: frame))
            buffer.removeFirst(min(FingerprintCore.hopSize, buffer.count))
        }
    }

    private func emitAvailableHashes() -> [UInt32] {
        var hashes: [UInt32] = []
        while chromaFrames.count >= FingerprintCore.hashFrameCount {
            hashes.append(FingerprintCore.computeHash(Array(chromaFrames[..<FingerprintCore.hashFrameCount])))
            chromaFrames.removeFirst(2)
        }
        return hashes
    }
}

public protocol StreamingWindowedFingerprinterProtocol: AnyObject {
    func durationMs() -> UInt32
    func flush() -> [WindowedFingerprint]
    func pushSamples(samples: [Int16]) -> [WindowedFingerprint]
    func pushSamplesF32(samples: [Float], channels: UInt16) -> [WindowedFingerprint]
    func reset()
}

open class StreamingWindowedFingerprinter: StreamingWindowedFingerprinterProtocol {
    public struct NoPointer {
        public init() {}
    }

    private let sampleRate: UInt32
    private let channels: UInt16
    private let windowDurationMs: UInt32
    private let windowIntervalMs: UInt32
    private var samplesAtTargetRate: [Float]
    private var nextWindowStart: Int

    public required init(unsafeFromRawPointer _: UnsafeMutableRawPointer) {
        sampleRate = FingerprintCore.targetSampleRate
        channels = 1
        windowDurationMs = 10_000
        windowIntervalMs = 2_000
        samplesAtTargetRate = []
        nextWindowStart = 0
    }

    public init(noPointer _: NoPointer) {
        sampleRate = FingerprintCore.targetSampleRate
        channels = 1
        windowDurationMs = 10_000
        windowIntervalMs = 2_000
        samplesAtTargetRate = []
        nextWindowStart = 0
    }

    public init(sampleRate: UInt32, channels: UInt16, windowDurationMs: UInt32, windowIntervalMs: UInt32) {
        self.sampleRate = sampleRate
        self.channels = channels
        self.windowDurationMs = windowDurationMs
        self.windowIntervalMs = windowIntervalMs
        samplesAtTargetRate = []
        nextWindowStart = 0
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    open func durationMs() -> UInt32 {
        UInt32((UInt64(samplesAtTargetRate.count) * 1000) / UInt64(FingerprintCore.targetSampleRate))
    }

    open func flush() -> [WindowedFingerprint] {
        emitAvailableWindows()
    }

    open func pushSamples(samples: [Int16]) -> [WindowedFingerprint] {
        let floats = samples.map { Float($0) / 32768.0 }
        let mono = FingerprintCore.resampleToMono(samples: floats, sampleRate: sampleRate, channels: channels)
        samplesAtTargetRate.append(contentsOf: mono)
        return emitAvailableWindows()
    }

    open func pushSamplesF32(samples: [Float], channels: UInt16) -> [WindowedFingerprint] {
        let mono = FingerprintCore.resampleToMono(samples: samples, sampleRate: sampleRate, channels: channels)
        samplesAtTargetRate.append(contentsOf: mono)
        return emitAvailableWindows()
    }

    open func reset() {
        samplesAtTargetRate.removeAll(keepingCapacity: true)
        nextWindowStart = 0
    }

    private func emitAvailableWindows() -> [WindowedFingerprint] {
        let windowSamples = FingerprintCore.samples(forMilliseconds: windowDurationMs)
        let intervalSamples = FingerprintCore.samples(forMilliseconds: windowIntervalMs)
        guard windowSamples >= FingerprintCore.frameSize, intervalSamples > 0 else {
            return []
        }

        var windows: [WindowedFingerprint] = []
        while nextWindowStart + windowSamples <= samplesAtTargetRate.count {
            let end = nextWindowStart + windowSamples
            let window = Array(samplesAtTargetRate[nextWindowStart..<end])
            windows.append(
                WindowedFingerprint(
                    timestampMs: UInt32((UInt64(nextWindowStart) * 1000) / UInt64(FingerprintCore.targetSampleRate)),
                    durationMs: windowDurationMs,
                    hashes: FingerprintCore.fingerprintSamples(window, durationMs: windowDurationMs).hashes
                )
            )
            nextWindowStart += intervalSamples
        }
        return windows
    }
}

public struct FingerprintData: Equatable, Hashable {
    public var hashes: [UInt32]
    public var durationMs: UInt32

    public init(hashes: [UInt32], durationMs: UInt32) {
        self.hashes = hashes
        self.durationMs = durationMs
    }
}

public struct MatchResult: Equatable, Hashable {
    public var timestamp: Float
    public var score: Float

    public init(timestamp: Float, score: Float) {
        self.timestamp = timestamp
        self.score = score
    }
}

public struct WindowedFingerprint: Equatable, Hashable {
    public var timestampMs: UInt32
    public var durationMs: UInt32
    public var hashes: [UInt32]

    public init(timestampMs: UInt32, durationMs: UInt32, hashes: [UInt32]) {
        self.timestampMs = timestampMs
        self.durationMs = durationMs
        self.hashes = hashes
    }
}

public enum FingerprintError: Error, Equatable, Hashable {
    case DecodeError(message: String)
    case UnsupportedFormat(message: String)
    case InvalidInput(message: String)
    case IoError(message: String)
}

extension FingerprintError: LocalizedError {
    public var errorDescription: String? {
        switch self {
        case let .DecodeError(message),
             let .UnsupportedFormat(message),
             let .InvalidInput(message),
             let .IoError(message):
            return message
        }
    }
}

public func compareHashes(hashes1: [UInt32], hashes2: [UInt32]) -> Float {
    FingerprintCore.compare(hashes1, hashes2)
}

public func compareHashesWithDrift(hashes1: [UInt32], hashes2: [UInt32], maxDrift: UInt32) -> Float {
    FingerprintCore.compareWithDrift(hashes1, hashes2, maxDrift)
}

public func fingerprintFromBytes(data: Data) -> FingerprintData? {
    FingerprintCore.fingerprint(from: data)
}

public func fingerprintToBytes(hashes: [UInt32], durationMs: UInt32) -> Data {
    FingerprintCore.bytes(for: FingerprintData(hashes: hashes, durationMs: durationMs))
}

public func fingerprintVersion() -> String {
    "fingerprint_uniffi 0.1.0"
}

private enum FingerprintCore {
    static let targetSampleRate: UInt32 = 11_025
    static let frameSize = 4_096
    static let hopSize = 1_024
    static let hashFrameCount = 8
    private static let hashThreshold: Float = 0.05

    struct AudioFile {
        let samples: [Float]
        let sampleRate: UInt32
        let channels: UInt16
    }

    static func compare(_ first: [UInt32], _ second: [UInt32]) -> Float {
        compareAtOffset(first, second, firstStart: 0, secondStart: 0)
    }

    static func compareWithDrift(_ first: [UInt32], _ second: [UInt32], _ maxDrift: UInt32) -> Float {
        guard !first.isEmpty, !second.isEmpty else {
            return 0
        }

        var best = compare(first, second)
        let driftLimit = min(Int(maxDrift), min(first.count, second.count))
        guard driftLimit > 0 else {
            return best
        }

        for drift in 1...driftLimit {
            best = max(best, compareAtOffset(first, second, firstStart: drift, secondStart: 0))
            best = max(best, compareAtOffset(first, second, firstStart: 0, secondStart: drift))
        }
        return best
    }

    private static func compareAtOffset(_ first: [UInt32], _ second: [UInt32], firstStart: Int, secondStart: Int) -> Float {
        guard firstStart < first.count, secondStart < second.count else {
            return 0
        }

        let count = min(first.count - firstStart, second.count - secondStart)
        guard count > 0 else {
            return 0
        }

        var matchingBits = 0
        for index in 0..<count {
            matchingBits += Int((~(first[firstStart + index] ^ second[secondStart + index])).nonzeroBitCount)
        }
        return Float(matchingBits) / Float(count * 32)
    }

    static func fingerprint(from data: Data) -> FingerprintData? {
        let bytes = [UInt8](data)
        guard bytes.count >= 8 else {
            return nil
        }
        let duration = readUInt32(bytes, 0)
        let count = Int(readUInt32(bytes, 4))
        guard count <= (bytes.count - 8) / 4 else {
            return nil
        }

        var hashes: [UInt32] = []
        hashes.reserveCapacity(count)
        for index in 0..<count {
            hashes.append(readUInt32(bytes, 8 + index * 4))
        }
        return FingerprintData(hashes: hashes, durationMs: duration)
    }

    static func bytes(for fingerprint: FingerprintData) -> Data {
        var bytes: [UInt8] = []
        bytes.reserveCapacity(8 + fingerprint.hashes.count * 4)
        appendUInt32(fingerprint.durationMs, to: &bytes)
        appendUInt32(UInt32(clamping: fingerprint.hashes.count), to: &bytes)
        for hash in fingerprint.hashes {
            appendUInt32(hash, to: &bytes)
        }
        return Data(bytes)
    }

    static func decodeAudioFile(_ data: Data) throws -> AudioFile {
        let bytes = [UInt8](data)
        if bytes.starts(with: Array("RIFF".utf8)) {
            return try decodeWave(bytes)
        }
        if bytes.starts(with: Array("ID3".utf8)) || looksLikeMp3(bytes) {
            throw FingerprintError.UnsupportedFormat(message: "MP3 decoding is not available in the source Swift reimplementation.")
        }
        throw FingerprintError.UnsupportedFormat(message: "Unsupported audio format.")
    }

    static func resampleToMono(samples: [Float], sampleRate: UInt32, channels: UInt16) -> [Float] {
        let channelCount = max(Int(channels), 1)
        let frameCount = samples.count / channelCount
        guard frameCount > 0 else {
            return []
        }

        var mono = Array(repeating: Float(0), count: frameCount)
        if channelCount == 1 {
            for index in 0..<frameCount {
                mono[index] = samples[index]
            }
        } else {
            for frame in 0..<frameCount {
                var sum: Float = 0
                let base = frame * channelCount
                for channel in 0..<channelCount {
                    sum += samples[base + channel]
                }
                mono[frame] = sum / Float(channelCount)
            }
        }

        guard sampleRate != targetSampleRate else {
            return mono
        }

        let ratio = Double(sampleRate) / Double(targetSampleRate)
        let outputCount = Int(Double(mono.count) / ratio)
        guard outputCount > 0 else {
            return []
        }

        var output = Array(repeating: Float(0), count: outputCount)
        for index in 0..<outputCount {
            let sourcePosition = Double(index) * ratio
            let sourceIndex = Int(sourcePosition)
            if sourceIndex + 1 < mono.count {
                let fraction = Float(sourcePosition - Double(sourceIndex))
                output[index] = mono[sourceIndex] + (mono[sourceIndex + 1] - mono[sourceIndex]) * fraction
            } else if sourceIndex < mono.count {
                output[index] = mono[sourceIndex]
            }
        }
        return output
    }

    static func fingerprintWindows(samples: [Float], windowDurationMs: UInt32, windowIntervalMs: UInt32) throws -> [WindowedFingerprint] {
        let windowSamples = self.samples(forMilliseconds: windowDurationMs)
        let intervalSamples = self.samples(forMilliseconds: windowIntervalMs)
        guard windowSamples >= frameSize else {
            throw FingerprintError.InvalidInput(message: "Window too short: \(windowSamples) samples, need at least \(frameSize)")
        }
        guard intervalSamples > 0 else {
            throw FingerprintError.InvalidInput(message: "Window interval must be greater than 0")
        }
        guard samples.count >= windowSamples else {
            return []
        }

        var windows: [WindowedFingerprint] = []
        var start = 0
        var timestamp: UInt32 = 0
        while start + windowSamples <= samples.count {
            let window = Array(samples[start..<(start + windowSamples)])
            let fingerprint = fingerprintSamples(window, durationMs: windowDurationMs)
            windows.append(WindowedFingerprint(timestampMs: timestamp, durationMs: windowDurationMs, hashes: fingerprint.hashes))
            start += intervalSamples
            timestamp &+= windowIntervalMs
        }
        return windows
    }

    static func fingerprintSamples(_ samples: [Float], durationMs: UInt32) -> FingerprintData {
        var chromaFrames: [[Float]] = []
        var offset = 0
        while offset + frameSize <= samples.count {
            chromaFrames.append(chroma(for: Array(samples[offset..<(offset + frameSize)])))
            offset += hopSize
        }
        return FingerprintData(hashes: encode(chromaFrames), durationMs: durationMs)
    }

    static func samples(forMilliseconds milliseconds: UInt32) -> Int {
        Int((UInt64(milliseconds) * UInt64(targetSampleRate)) / 1000)
    }

    static func chroma(for frame: [Float]) -> [Float] {
        let magnitudes = fftMagnitudes(frame)
        return chroma(fromMagnitudes: magnitudes, sampleRate: targetSampleRate)
    }

    static func computeHash(_ frames: [[Float]]) -> UInt32 {
        guard frames.count >= 2 else {
            return 0
        }

        var hash: UInt32 = 0
        var bit = 0
        for offset in 1..<min(frames.count, 4) {
            let pitchLimit = offset == 3 ? 8 : 12
            for pitch in 0..<pitchLimit {
                if frames[offset][pitch] - frames[offset - 1][pitch] > hashThreshold {
                    hash |= UInt32(1) << UInt32(bit)
                }
                bit += 1
                if bit == 28 {
                    break
                }
            }
            if bit == 28 {
                break
            }
        }

        let coarseEnergy = frames[0].reduce(Float(0), +)
        hash ^= UInt32(max(0, min(15, Int(coarseEnergy * 4)))) << 28
        return hash
    }

    private static func encode(_ frames: [[Float]]) -> [UInt32] {
        guard frames.count >= hashFrameCount else {
            return []
        }

        var hashes: [UInt32] = []
        var start = 0
        while start + hashFrameCount <= frames.count {
            hashes.append(computeHash(Array(frames[start..<(start + hashFrameCount)])))
            start += 2
        }

        let lastStart = frames.count - hashFrameCount
        if lastStart > 0, (lastStart - 0) % 2 != 0 {
            hashes.append(computeHash(Array(frames[lastStart..<(lastStart + hashFrameCount)])))
        }
        return hashes
    }

    private static func chroma(fromMagnitudes magnitudes: [Float], sampleRate: UInt32) -> [Float] {
        var bins = Array(repeating: Float(0), count: 12)
        var counts = Array(repeating: Float(0), count: 12)
        let denominator = max(1, (magnitudes.count * 2) - 2)

        for index in 0..<magnitudes.count {
            let frequency = (Float(sampleRate) / Float(denominator)) * Float(index)
            guard frequency >= 28, frequency < 3520 else {
                continue
            }
            let rawPitch = fmodf(log2f(frequency / 440) * 12 + 9, 12)
            let pitch = Int(min(Float(11), rawPitch >= 0 ? rawPitch : rawPitch + 12))
            bins[pitch] += magnitudes[index] * magnitudes[index]
            counts[pitch] += 1
        }

        for index in 0..<12 where counts[index] > 0 {
            bins[index] /= counts[index]
        }

        let norm = sqrtf(bins.reduce(Float(0)) { $0 + $1 * $1 })
        if norm > 0.000001 {
            for index in 0..<12 {
                bins[index] /= norm
            }
        }
        return bins
    }

    private static func fftMagnitudes(_ frame: [Float]) -> [Float] {
        let count = frameSize
        var real = Array(repeating: Double(0), count: count)
        var imaginary = Array(repeating: Double(0), count: count)
        for index in 0..<min(frame.count, count) {
            let window = 0.5 * (1.0 - cos((2.0 * Double.pi * Double(index)) / Double(count - 1)))
            real[index] = Double(frame[index]) * window
        }

        var j = 0
        for i in 1..<count {
            var bit = count >> 1
            while j & bit != 0 {
                j ^= bit
                bit >>= 1
            }
            j ^= bit
            if i < j {
                real.swapAt(i, j)
                imaginary.swapAt(i, j)
            }
        }

        var length = 2
        while length <= count {
            let angle = -2.0 * Double.pi / Double(length)
            let wLengthReal = cos(angle)
            let wLengthImaginary = sin(angle)
            var i = 0
            while i < count {
                var wReal = 1.0
                var wImaginary = 0.0
                for offset in 0..<(length / 2) {
                    let even = i + offset
                    let odd = even + length / 2
                    let oddReal = real[odd] * wReal - imaginary[odd] * wImaginary
                    let oddImaginary = real[odd] * wImaginary + imaginary[odd] * wReal
                    real[odd] = real[even] - oddReal
                    imaginary[odd] = imaginary[even] - oddImaginary
                    real[even] += oddReal
                    imaginary[even] += oddImaginary

                    let nextReal = wReal * wLengthReal - wImaginary * wLengthImaginary
                    wImaginary = wReal * wLengthImaginary + wImaginary * wLengthReal
                    wReal = nextReal
                }
                i += length
            }
            length <<= 1
        }

        var magnitudes = Array(repeating: Float(0), count: count / 2 + 1)
        for index in 0..<magnitudes.count {
            magnitudes[index] = Float(hypot(real[index], imaginary[index]))
        }
        return magnitudes
    }

    private static func decodeWave(_ bytes: [UInt8]) throws -> AudioFile {
        guard bytes.count >= 12, String(bytes: bytes[8..<12], encoding: .ascii) == "WAVE" else {
            throw FingerprintError.DecodeError(message: "no WAVE tag found")
        }

        var offset = 12
        var audioFormat: UInt16?
        var channels: UInt16?
        var sampleRate: UInt32?
        var bitsPerSample: UInt16?
        var dataRange: Range<Int>?

        while offset + 8 <= bytes.count {
            let chunkId = String(bytes: bytes[offset..<(offset + 4)], encoding: .ascii) ?? ""
            let chunkSize = Int(readUInt32(bytes, offset + 4))
            let chunkStart = offset + 8
            let chunkEnd = min(chunkStart + chunkSize, bytes.count)

            if chunkId == "fmt ", chunkEnd - chunkStart >= 16 {
                audioFormat = readUInt16(bytes, chunkStart)
                channels = readUInt16(bytes, chunkStart + 2)
                sampleRate = readUInt32(bytes, chunkStart + 4)
                bitsPerSample = readUInt16(bytes, chunkStart + 14)
            } else if chunkId == "data" {
                dataRange = chunkStart..<chunkEnd
            }

            offset = chunkStart + chunkSize + (chunkSize & 1)
        }

        guard let format = audioFormat,
              let channelCount = channels,
              let rate = sampleRate,
              let bits = bitsPerSample,
              let range = dataRange
        else {
            throw FingerprintError.DecodeError(message: "Failed to decode WAV data")
        }

        let sampleBytes = Int(bits / 8)
        guard sampleBytes > 0 else {
            throw FingerprintError.UnsupportedFormat(message: "Unsupported WAV format: \(bits) bit")
        }

        var samples: [Float] = []
        samples.reserveCapacity((range.count / sampleBytes))
        var index = range.lowerBound
        while index + sampleBytes <= range.upperBound {
            switch (format, bits) {
            case (1, 8):
                samples.append((Float(bytes[index]) - 128) / 128)
            case (1, 16):
                samples.append(Float(readInt16(bytes, index)) / 32768)
            case (1, 24):
                samples.append(Float(readInt24(bytes, index)) / 8_388_608)
            case (1, 32):
                samples.append(Float(readInt32(bytes, index)) / 2_147_483_648)
            case (3, 32):
                samples.append(Float(bitPattern: readUInt32(bytes, index)))
            default:
                throw FingerprintError.UnsupportedFormat(message: "Unsupported WAV format: \(bits) bit")
            }
            index += sampleBytes
        }

        return AudioFile(samples: samples, sampleRate: rate, channels: channelCount)
    }

    private static func looksLikeMp3(_ bytes: [UInt8]) -> Bool {
        guard bytes.count >= 2 else {
            return false
        }
        return bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0
    }

    private static func appendUInt32(_ value: UInt32, to bytes: inout [UInt8]) {
        bytes.append(UInt8(value & 0xff))
        bytes.append(UInt8((value >> 8) & 0xff))
        bytes.append(UInt8((value >> 16) & 0xff))
        bytes.append(UInt8((value >> 24) & 0xff))
    }

    private static func readUInt16(_ bytes: [UInt8], _ offset: Int) -> UInt16 {
        UInt16(bytes[offset]) | (UInt16(bytes[offset + 1]) << 8)
    }

    private static func readInt16(_ bytes: [UInt8], _ offset: Int) -> Int16 {
        Int16(bitPattern: readUInt16(bytes, offset))
    }

    private static func readInt24(_ bytes: [UInt8], _ offset: Int) -> Int32 {
        var value = Int32(bytes[offset]) | (Int32(bytes[offset + 1]) << 8) | (Int32(bytes[offset + 2]) << 16)
        if value & 0x0080_0000 != 0 {
            value |= -0x0100_0000
        }
        return value
    }

    private static func readInt32(_ bytes: [UInt8], _ offset: Int) -> Int32 {
        Int32(bitPattern: readUInt32(bytes, offset))
    }

    private static func readUInt32(_ bytes: [UInt8], _ offset: Int) -> UInt32 {
        UInt32(bytes[offset])
            | (UInt32(bytes[offset + 1]) << 8)
            | (UInt32(bytes[offset + 2]) << 16)
            | (UInt32(bytes[offset + 3]) << 24)
    }
}
