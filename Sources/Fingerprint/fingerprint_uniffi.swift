import FingerprintFFI
import Foundation

private let fingerprintFfiStatusOk: UInt32 = 0

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

    private var raw: UnsafeMutableRawPointer?

    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    public init(noPointer _: NoPointer) {
        raw = fingerprint_ffi_checkpoint_new(0)
    }

    public convenience init() {
        self.init(noPointer: NoPointer())
    }

    private init(maxDrift: UInt32) {
        raw = fingerprint_ffi_checkpoint_new(maxDrift)
    }

    deinit {
        if let raw {
            fingerprint_ffi_checkpoint_free(raw)
        }
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw ?? UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    public static func withDrift(maxDrift: UInt32) -> CheckpointMatcher {
        CheckpointMatcher(maxDrift: maxDrift)
    }

    open func add(timestamp: Float, hashes: [UInt32], duration: Float) {
        hashes.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_checkpoint_add(raw, timestamp, buffer.baseAddress, buffer.count, duration)
        }
    }

    open func clear() {
        fingerprint_ffi_checkpoint_clear(raw)
    }

    open func count() -> UInt32 {
        fingerprint_ffi_checkpoint_count(raw)
    }

    open func findTopMatches(queryHashes: [UInt32], maxResults: UInt32) -> [MatchResult] {
        let ffiMatches = queryHashes.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_checkpoint_find_top_matches(raw, buffer.baseAddress, buffer.count, maxResults)
        }
        return takeMatchArray(ffiMatches)
    }

    open func setDrift(maxDrift: UInt32) {
        fingerprint_ffi_checkpoint_set_drift(raw, maxDrift)
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
        let result = data.withUnsafeBytes { rawBuffer in
            let pointer = rawBuffer.bindMemory(to: UInt8.self).baseAddress
            return fingerprint_ffi_fingerprint_data_windowed(pointer, rawBuffer.count, windowDurationMs, windowIntervalMs)
        }

        if result.status != fingerprintFfiStatusOk {
            fingerprint_ffi_free_windowed_array(result.windows)
            throw takeError(status: result.status, message: result.message)
        }

        return takeWindowedArray(result.windows)
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

    private var raw: UnsafeMutableRawPointer?

    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    public init(noPointer _: NoPointer) {
        do {
            raw = try Self.makeHandle(sampleRate: 11_025, channels: 1)
        } catch {
            preconditionFailure("Failed to create default StreamingFingerprinter handle: \(error)")
        }
    }

    public init(sampleRate: UInt32, channels: UInt16) throws {
        raw = try Self.makeHandle(sampleRate: sampleRate, channels: channels)
    }

    deinit {
        if let raw {
            fingerprint_ffi_streaming_free(raw)
        }
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw ?? UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    open func durationMs() -> UInt32 {
        fingerprint_ffi_streaming_duration_ms(raw)
    }

    open func flush() -> [UInt32] {
        takeU32Array(fingerprint_ffi_streaming_flush(raw))
    }

    open func pushSamples(samples: [Int16]) -> [UInt32] {
        let array = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_push_i16(raw, buffer.baseAddress, buffer.count)
        }
        return takeU32Array(array)
    }

    open func pushSamplesF32(samples: [Float], channels: UInt16) -> [UInt32] {
        let array = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_push_f32(raw, buffer.baseAddress, buffer.count, channels)
        }
        return takeU32Array(array)
    }

    open func reset() {
        fingerprint_ffi_streaming_reset(raw)
    }

    private static func makeHandle(sampleRate: UInt32, channels: UInt16) throws -> UnsafeMutableRawPointer {
        try takeHandleResult(fingerprint_ffi_streaming_new(sampleRate, channels))
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

    private var raw: UnsafeMutableRawPointer?

    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    public init(noPointer _: NoPointer) {
        do {
            raw = try Self.makeHandle(
                sampleRate: 11_025,
                channels: 1,
                windowDurationMs: 10_000,
                windowIntervalMs: 2_000
            )
        } catch {
            preconditionFailure("Failed to create default StreamingWindowedFingerprinter handle: \(error)")
        }
    }

    public init(
        sampleRate: UInt32,
        channels: UInt16,
        windowDurationMs: UInt32,
        windowIntervalMs: UInt32
    ) throws {
        raw = try Self.makeHandle(
            sampleRate: sampleRate,
            channels: channels,
            windowDurationMs: windowDurationMs,
            windowIntervalMs: windowIntervalMs
        )
    }

    deinit {
        if let raw {
            fingerprint_ffi_streaming_windowed_free(raw)
        }
    }

    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw ?? UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    open func durationMs() -> UInt32 {
        fingerprint_ffi_streaming_windowed_duration_ms(raw)
    }

    open func flush() -> [WindowedFingerprint] {
        takeWindowedArray(fingerprint_ffi_streaming_windowed_flush(raw))
    }

    open func pushSamples(samples: [Int16]) -> [WindowedFingerprint] {
        let windows = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_windowed_push_i16(raw, buffer.baseAddress, buffer.count)
        }
        return takeWindowedArray(windows)
    }

    open func pushSamplesF32(samples: [Float], channels: UInt16) -> [WindowedFingerprint] {
        let windows = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_windowed_push_f32(raw, buffer.baseAddress, buffer.count, channels)
        }
        return takeWindowedArray(windows)
    }

    open func reset() {
        fingerprint_ffi_streaming_windowed_reset(raw)
    }

    private static func makeHandle(sampleRate: UInt32, channels: UInt16, windowDurationMs: UInt32, windowIntervalMs: UInt32) throws -> UnsafeMutableRawPointer {
        try takeHandleResult(fingerprint_ffi_streaming_windowed_new(sampleRate, channels, windowDurationMs, windowIntervalMs))
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
    case InternalError(message: String)
}

extension FingerprintError: LocalizedError {
    public var errorDescription: String? {
        switch self {
        case let .DecodeError(message),
             let .UnsupportedFormat(message),
             let .InvalidInput(message),
             let .IoError(message),
             let .InternalError(message):
            return message
        }
    }
}

public func compareHashes(hashes1: [UInt32], hashes2: [UInt32]) -> Float {
    hashes1.withUnsafeBufferPointer { first in
        hashes2.withUnsafeBufferPointer { second in
            fingerprint_ffi_compare_hashes(first.baseAddress, first.count, second.baseAddress, second.count)
        }
    }
}

public func compareHashesWithDrift(hashes1: [UInt32], hashes2: [UInt32], maxDrift: UInt32) -> Float {
    hashes1.withUnsafeBufferPointer { first in
        hashes2.withUnsafeBufferPointer { second in
            fingerprint_ffi_compare_hashes_with_drift(first.baseAddress, first.count, second.baseAddress, second.count, maxDrift)
        }
    }
}

public func fingerprintFromBytes(data: Data) -> FingerprintData? {
    let result = data.withUnsafeBytes { rawBuffer in
        let pointer = rawBuffer.bindMemory(to: UInt8.self).baseAddress
        return fingerprint_ffi_from_bytes(pointer, rawBuffer.count)
    }
    guard result.found != 0 else {
        fingerprint_ffi_free_u32_array(result.hashes)
        return nil
    }
    return FingerprintData(hashes: takeU32Array(result.hashes), durationMs: result.duration_ms)
}

public func fingerprintToBytes(hashes: [UInt32], durationMs: UInt32) -> Data {
    let bytes = hashes.withUnsafeBufferPointer { buffer in
        fingerprint_ffi_to_bytes(buffer.baseAddress, buffer.count, durationMs)
    }
    return takeData(bytes)
}

public func fingerprintVersion() -> String {
    takeString(fingerprint_ffi_version())
}

private func takeData(_ bytes: FingerprintFfiBytes) -> Data {
    defer { fingerprint_ffi_free_bytes(bytes) }
    let count = Int(bytes.len)
    guard count > 0, let pointer = bytes.ptr else {
        return Data()
    }
    return Data(bytes: pointer, count: count)
}

private func takeString(_ bytes: FingerprintFfiBytes) -> String {
    String(data: takeData(bytes), encoding: .utf8) ?? ""
}

private func copyU32Array(_ array: FingerprintFfiU32Array) -> [UInt32] {
    let count = Int(array.len)
    guard count > 0, let pointer = array.ptr else {
        return []
    }
    return Array(UnsafeBufferPointer(start: pointer, count: count))
}

private func takeU32Array(_ array: FingerprintFfiU32Array) -> [UInt32] {
    defer { fingerprint_ffi_free_u32_array(array) }
    return copyU32Array(array)
}

private func takeMatchArray(_ array: FingerprintFfiMatchArray) -> [MatchResult] {
    defer { fingerprint_ffi_free_match_array(array) }
    let count = Int(array.len)
    guard count > 0, let pointer = array.ptr else {
        return []
    }
    return UnsafeBufferPointer(start: pointer, count: count).map {
        MatchResult(timestamp: $0.timestamp, score: $0.score)
    }
}

private func takeWindowedArray(_ array: FingerprintFfiWindowedArray) -> [WindowedFingerprint] {
    defer { fingerprint_ffi_free_windowed_array(array) }
    let count = Int(array.len)
    guard count > 0, let pointer = array.ptr else {
        return []
    }
    return UnsafeBufferPointer(start: pointer, count: count).map {
        WindowedFingerprint(
            timestampMs: $0.timestamp_ms,
            durationMs: $0.duration_ms,
            hashes: copyU32Array($0.hashes)
        )
    }
}

private func takeHandleResult(_ result: FingerprintFfiHandleResult) throws -> UnsafeMutableRawPointer {
    if result.status != fingerprintFfiStatusOk {
        throw takeError(status: result.status, message: result.message)
    }

    defer { fingerprint_ffi_free_bytes(result.message) }
    guard let handle = result.handle else {
        throw FingerprintError.InvalidInput(message: "constructor returned a null handle")
    }
    return handle
}

private func takeError(status: UInt32, message: FingerprintFfiBytes) -> FingerprintError {
    let text = takeString(message)
    switch status {
    case 1:
        return .DecodeError(message: text)
    case 2:
        return .UnsupportedFormat(message: text)
    case 3:
        return .InvalidInput(message: text)
    case 4:
        return .IoError(message: text)
    case 5:
        return .InternalError(message: text.isEmpty ? "internal Rust panic" : text)
    default:
        return .InvalidInput(message: text)
    }
}
