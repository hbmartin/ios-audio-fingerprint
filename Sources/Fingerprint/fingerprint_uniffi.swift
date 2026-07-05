internal import FingerprintFFI
public import Foundation

/// Requirements of a checkpoint store that ranks stored fingerprints against a query.
///
/// ``CheckpointMatcher`` is the packaged implementation; the protocol exists so tests and
/// callers can substitute their own.
public protocol CheckpointMatcherProtocol: AnyObject {
    /// Stores a checkpoint fingerprint at `timestamp` seconds with the given `duration` in seconds.
    func add(timestamp: Float, hashes: [UInt32], duration: Float)
    /// Removes every stored checkpoint.
    func clear()
    /// Number of stored checkpoints.
    func count() -> UInt32
    /// The best-scoring checkpoints for `queryHashes`, at most `maxResults` of them, sorted by
    /// score (descending), then timestamp (ascending), then insertion order.
    func findTopMatches(queryHashes: [UInt32], maxResults: UInt32) -> [MatchResult]
    /// Sets the maximum hash-position misalignment tolerated when scoring (see
    /// ``compareHashesWithDrift(hashes1:hashes2:maxDrift:)``).
    func setDrift(maxDrift: UInt32)
}

/// Stores timestamped checkpoint fingerprints and ranks them against a query.
///
/// Add fingerprints with ``add(timestamp:hashes:duration:)`` and retrieve the
/// best candidates for a query with ``findTopMatches(queryHashes:maxResults:)``.
/// Use ``withDrift(maxDrift:)`` (or ``setDrift(maxDrift:)``) to tolerate timing
/// misalignment between the query and stored checkpoints.
///
/// Thread safety: `raw` is immutable after initialization and the Rust side
/// guards all mutable state behind a `Mutex` (see fingerprint-ffi/src/lib.rs),
/// so instances may be shared across concurrency domains.
public final class CheckpointMatcher: CheckpointMatcherProtocol, @unchecked Sendable {
    /// Marker for the designated initializer that allocates its own Rust handle.
    public struct NoPointer: Sendable {
        public init() {}
    }

    private let raw: UnsafeMutableRawPointer

    /// Adopts `pointer` as a live Rust checkpoint handle and takes ownership of freeing it.
    ///
    /// Only pass a pointer previously obtained from this library. This initializer exists for
    /// binding interoperability; prefer ``init()`` or ``withDrift(maxDrift:)``.
    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    /// Creates an empty matcher with drift tolerance `0`.
    public init(noPointer _: NoPointer) {
        guard let pointer = fingerprint_ffi_checkpoint_new(0) else {
            preconditionFailure("fingerprint_ffi_checkpoint_new returned a null handle")
        }
        raw = pointer
    }

    /// Creates an empty matcher with drift tolerance `0`.
    public convenience init() {
        self.init(noPointer: NoPointer())
    }

    private init(maxDrift: UInt32) {
        guard let pointer = fingerprint_ffi_checkpoint_new(maxDrift) else {
            preconditionFailure("fingerprint_ffi_checkpoint_new returned a null handle")
        }
        raw = pointer
    }

    deinit {
        fingerprint_ffi_checkpoint_free(raw)
    }

    /// The underlying Rust handle. The handle stays owned by this instance; the returned pointer
    /// must not outlive it. Exists for binding interoperability.
    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw
    }

    /// Creates an empty matcher that tolerates up to `maxDrift` hash positions of misalignment
    /// when scoring queries.
    public static func withDrift(maxDrift: UInt32) -> CheckpointMatcher {
        CheckpointMatcher(maxDrift: maxDrift)
    }

    /// Stores a checkpoint fingerprint at `timestamp` seconds with the given `duration` in seconds.
    public func add(timestamp: Float, hashes: [UInt32], duration: Float) {
        hashes.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_checkpoint_add(raw, timestamp, buffer.baseAddress, buffer.count, duration)
        }
    }

    /// Removes every stored checkpoint.
    public func clear() {
        fingerprint_ffi_checkpoint_clear(raw)
    }

    /// Number of stored checkpoints.
    public func count() -> UInt32 {
        fingerprint_ffi_checkpoint_count(raw)
    }

    /// Scores every stored checkpoint against `queryHashes` and returns at most `maxResults`
    /// of the best, sorted by score (descending), then timestamp (ascending), then insertion order.
    public func findTopMatches(queryHashes: [UInt32], maxResults: UInt32) -> [MatchResult] {
        let ffiMatches = queryHashes.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_checkpoint_find_top_matches(raw, buffer.baseAddress, buffer.count, maxResults)
        }
        return takeMatchArray(ffiMatches)
    }

    /// Sets the maximum hash-position misalignment tolerated when scoring queries.
    public func setDrift(maxDrift: UInt32) {
        fingerprint_ffi_checkpoint_set_drift(raw, maxDrift)
    }
}

/// Requirements of a one-shot fingerprinter for encoded audio bytes.
public protocol FingerprinterProtocol: AnyObject {
    /// Decodes `data` (WAV or MP3) and fingerprints it in overlapping windows.
    func fingerprintDataWindowed(
        data: Data, windowDurationMs: UInt32, windowIntervalMs: UInt32
    ) throws(FingerprintError) -> [WindowedFingerprint]
}

/// One-shot windowed fingerprinting of encoded audio bytes.
///
/// The container is auto-detected: RIFF/WAVE (PCM 8/16/24/32-bit and 32-bit
/// IEEE float) and MP3 (ID3 tag or MPEG frame sync) are supported; anything
/// else throws ``FingerprintError/UnsupportedFormat(message:)``. The
/// instance is stateless, so it can be reused and shared freely.
public final class Fingerprinter: FingerprinterProtocol, Sendable {
    /// Marker for the designated initializer. ``Fingerprinter`` holds no Rust handle.
    public struct NoPointer: Sendable {
        public init() {}
    }

    /// Equivalent to ``init()``; the pointer is ignored because ``Fingerprinter`` is stateless.
    /// Exists for binding interoperability.
    public required init(unsafeFromRawPointer _: UnsafeMutableRawPointer) {}
    /// Creates a fingerprinter.
    public init(noPointer _: NoPointer) {}
    /// Creates a fingerprinter.
    public init() {}

    /// An opaque pointer to `self`. Exists for binding interoperability.
    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
    }

    /// Decodes `data` (WAV or MP3), downmixes and resamples it to the 11,025 Hz
    /// mono analysis rate, and fingerprints it in overlapping windows.
    ///
    /// - Parameters:
    ///   - data: Encoded audio bytes (whole file contents).
    ///   - windowDurationMs: Length of each window; must cover at least one FFT
    ///     frame (~372 ms) or the call throws
    ///     ``FingerprintError/InvalidInput(message:)``.
    ///   - windowIntervalMs: Hop between window starts; must be non-zero.
    /// - Returns: One ``WindowedFingerprint`` per complete window, in order.
    ///   Input shorter than one window yields an empty array. All windows are
    ///   cut from a single short-time transform, so overlapping windows share
    ///   their underlying analysis and streaming windows
    ///   (``StreamingWindowedFingerprinter``) match one-shot windows for
    ///   identical input.
    public func fingerprintDataWindowed(
        data: Data, windowDurationMs: UInt32, windowIntervalMs: UInt32
    ) throws(FingerprintError) -> [WindowedFingerprint] {
        let result = data.withUnsafeBytes { rawBuffer in
            let pointer = rawBuffer.bindMemory(to: UInt8.self).baseAddress
            return fingerprint_ffi_fingerprint_data_windowed(pointer, rawBuffer.count, windowDurationMs, windowIntervalMs)
        }

        if result.status != 0 {
            fingerprint_ffi_free_windowed_array(result.windows)
            throw takeError(status: result.status, message: result.message)
        }

        return takeWindowedArray(result.windows)
    }
}

/// Requirements of an incremental hash producer for raw PCM.
public protocol StreamingFingerprinterProtocol: AnyObject {
    /// Milliseconds of audio processed so far (measured at the analysis rate).
    func durationMs() -> UInt32
    /// Hashes from any remaining complete frames. Does not finalize the stream.
    func flush() -> [UInt32]
    /// Pushes interleaved 16-bit PCM (using the channel count from `init`) and returns any
    /// newly completed hashes.
    func pushSamples(samples: [Int16]) -> [UInt32]
    /// Pushes interleaved float PCM with an explicit per-call channel count and returns any
    /// newly completed hashes.
    func pushSamplesF32(samples: [Float], channels: UInt16) -> [UInt32]
    /// Clears all buffered state so the next push starts a fresh stream.
    func reset()
}

/// Emits fingerprint hashes incrementally from raw PCM pushed as it arrives.
///
/// Streaming does not decode containers: supply interleaved samples plus the
/// source sample rate and channel count up front. Input is downmixed and
/// resampled to the 11,025 Hz analysis rate with filter state carried across
/// pushes, so chunk boundaries do not affect the produced hashes.
///
/// Thread safety: `raw` is immutable after initialization and the Rust side
/// guards all mutable state behind a `Mutex` (see fingerprint-ffi/src/lib.rs),
/// so instances may be shared across concurrency domains.
public final class StreamingFingerprinter: StreamingFingerprinterProtocol, @unchecked Sendable {
    /// Marker for the designated initializer that allocates its own Rust handle.
    public struct NoPointer: Sendable {
        public init() {}
    }

    private let raw: UnsafeMutableRawPointer

    /// Adopts `pointer` as a live Rust streaming handle and takes ownership of freeing it.
    ///
    /// Only pass a pointer previously obtained from this library. This initializer exists for
    /// binding interoperability; prefer ``init(sampleRate:channels:)``.
    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    /// Creates a fingerprinter for mono input already at the 11,025 Hz analysis rate.
    public init(noPointer _: NoPointer) {
        do {
            raw = try Self.makeHandle(sampleRate: 11_025, channels: 1)
        } catch {
            preconditionFailure("Failed to create default StreamingFingerprinter handle: \(error)")
        }
    }

    /// Creates a fingerprinter for interleaved PCM at `sampleRate` with `channels` channels.
    ///
    /// - Throws: ``FingerprintError/InvalidInput(message:)`` when `sampleRate` or `channels`
    ///   is zero.
    public init(sampleRate: UInt32, channels: UInt16) throws(FingerprintError) {
        raw = try Self.makeHandle(sampleRate: sampleRate, channels: channels)
    }

    deinit {
        fingerprint_ffi_streaming_free(raw)
    }

    /// The underlying Rust handle. The handle stays owned by this instance; the returned pointer
    /// must not outlive it. Exists for binding interoperability.
    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw
    }

    /// Milliseconds of audio processed so far (measured at the analysis rate).
    public func durationMs() -> UInt32 {
        fingerprint_ffi_streaming_duration_ms(raw)
    }

    /// Hashes from any remaining complete frames. Does not finalize the stream; pushing more
    /// samples afterwards continues where the stream left off.
    public func flush() -> [UInt32] {
        takeU32Array(fingerprint_ffi_streaming_flush(raw))
    }

    /// Pushes interleaved 16-bit PCM (using the channel count from `init`) and returns any
    /// newly completed hashes.
    public func pushSamples(samples: [Int16]) -> [UInt32] {
        let array = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_push_i16(raw, buffer.baseAddress, buffer.count)
        }
        return takeU32Array(array)
    }

    /// Pushes interleaved float PCM with an explicit per-call channel count and returns any
    /// newly completed hashes. Passing `channels: 0` is a no-op that returns `[]`.
    public func pushSamplesF32(samples: [Float], channels: UInt16) -> [UInt32] {
        let array = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_push_f32(raw, buffer.baseAddress, buffer.count, channels)
        }
        return takeU32Array(array)
    }

    /// Clears all buffered state (including resampler context) so the next push starts a
    /// fresh stream.
    public func reset() {
        fingerprint_ffi_streaming_reset(raw)
    }

    private static func makeHandle(sampleRate: UInt32, channels: UInt16) throws(FingerprintError) -> UnsafeMutableRawPointer {
        try takeHandleResult(fingerprint_ffi_streaming_new(sampleRate, channels))
    }
}

/// Requirements of an incremental window producer for raw PCM.
public protocol StreamingWindowedFingerprinterProtocol: AnyObject {
    /// Milliseconds of audio processed so far (measured at the analysis rate).
    func durationMs() -> UInt32
    /// Any windows that completed but have not been emitted. Does not finalize the stream.
    func flush() -> [WindowedFingerprint]
    /// Pushes interleaved 16-bit PCM (using the channel count from `init`) and returns any
    /// newly completed windows.
    func pushSamples(samples: [Int16]) -> [WindowedFingerprint]
    /// Pushes interleaved float PCM with an explicit per-call channel count and returns any
    /// newly completed windows.
    func pushSamplesF32(samples: [Float], channels: UInt16) -> [WindowedFingerprint]
    /// Clears all buffered state so the next push starts a fresh stream.
    func reset()
}

/// Emits complete ``WindowedFingerprint`` values incrementally from raw PCM.
///
/// Windows are cut from the same global analysis grid as
/// ``Fingerprinter/fingerprintDataWindowed(data:windowDurationMs:windowIntervalMs:)``,
/// so streaming a signal produces the same windows as fingerprinting it in one
/// shot. A window is emitted as soon as its full duration has been pushed.
///
/// Thread safety: `raw` is immutable after initialization and the Rust side
/// guards all mutable state behind a `Mutex` (see fingerprint-ffi/src/lib.rs),
/// so instances may be shared across concurrency domains.
public final class StreamingWindowedFingerprinter: StreamingWindowedFingerprinterProtocol, @unchecked Sendable {
    /// Marker for the designated initializer that allocates its own Rust handle.
    public struct NoPointer: Sendable {
        public init() {}
    }

    private let raw: UnsafeMutableRawPointer

    /// Adopts `pointer` as a live Rust windowed-streaming handle and takes ownership of
    /// freeing it.
    ///
    /// Only pass a pointer previously obtained from this library. This initializer exists for
    /// binding interoperability; prefer
    /// ``init(sampleRate:channels:windowDurationMs:windowIntervalMs:)``.
    public required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) {
        raw = pointer
    }

    /// Creates a windowed fingerprinter for mono 11,025 Hz input with 10 s windows every 2 s.
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

    /// Creates a windowed fingerprinter for interleaved PCM at `sampleRate` with `channels`
    /// channels, emitting `windowDurationMs`-long windows every `windowIntervalMs`.
    ///
    /// - Throws: ``FingerprintError/InvalidInput(message:)`` when `sampleRate` or `channels`
    ///   is zero, when `windowDurationMs` does not cover at least one FFT frame (~372 ms), or
    ///   when `windowIntervalMs` converts to zero samples.
    public init(
        sampleRate: UInt32,
        channels: UInt16,
        windowDurationMs: UInt32,
        windowIntervalMs: UInt32
    ) throws(FingerprintError) {
        raw = try Self.makeHandle(
            sampleRate: sampleRate,
            channels: channels,
            windowDurationMs: windowDurationMs,
            windowIntervalMs: windowIntervalMs
        )
    }

    deinit {
        fingerprint_ffi_streaming_windowed_free(raw)
    }

    /// The underlying Rust handle. The handle stays owned by this instance; the returned pointer
    /// must not outlive it. Exists for binding interoperability.
    public func uniffiClonePointer() -> UnsafeMutableRawPointer {
        raw
    }

    /// Milliseconds of audio processed so far (measured at the analysis rate).
    public func durationMs() -> UInt32 {
        fingerprint_ffi_streaming_windowed_duration_ms(raw)
    }

    /// Any windows that completed but have not been emitted. Does not finalize the stream; a
    /// partially filled trailing window is never emitted.
    public func flush() -> [WindowedFingerprint] {
        takeWindowedArray(fingerprint_ffi_streaming_windowed_flush(raw))
    }

    /// Pushes interleaved 16-bit PCM (using the channel count from `init`) and returns any
    /// newly completed windows.
    public func pushSamples(samples: [Int16]) -> [WindowedFingerprint] {
        let windows = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_windowed_push_i16(raw, buffer.baseAddress, buffer.count)
        }
        return takeWindowedArray(windows)
    }

    /// Pushes interleaved float PCM with an explicit per-call channel count and returns any
    /// newly completed windows. Passing `channels: 0` is a no-op that returns `[]`.
    public func pushSamplesF32(samples: [Float], channels: UInt16) -> [WindowedFingerprint] {
        let windows = samples.withUnsafeBufferPointer { buffer in
            fingerprint_ffi_streaming_windowed_push_f32(raw, buffer.baseAddress, buffer.count, channels)
        }
        return takeWindowedArray(windows)
    }

    /// Clears all buffered state (including resampler context) so the next push starts a
    /// fresh stream.
    public func reset() {
        fingerprint_ffi_streaming_windowed_reset(raw)
    }

    private static func makeHandle(
        sampleRate: UInt32, channels: UInt16, windowDurationMs: UInt32, windowIntervalMs: UInt32
    ) throws(FingerprintError) -> UnsafeMutableRawPointer {
        try takeHandleResult(fingerprint_ffi_streaming_windowed_new(sampleRate, channels, windowDurationMs, windowIntervalMs))
    }
}

/// A deserialized fingerprint: its hashes plus the duration of audio they cover.
public struct FingerprintData: Equatable, Hashable, Sendable {
    /// The fingerprint hashes; each spans roughly one second of source audio.
    public var hashes: [UInt32]
    /// Duration of the fingerprinted audio in milliseconds.
    public var durationMs: UInt32

    public init(hashes: [UInt32], durationMs: UInt32) {
        self.hashes = hashes
        self.durationMs = durationMs
    }
}

/// One ranked candidate returned by ``CheckpointMatcher/findTopMatches(queryHashes:maxResults:)``.
public struct MatchResult: Equatable, Hashable, Sendable {
    /// The stored checkpoint's timestamp in seconds.
    public var timestamp: Float
    /// Similarity of the checkpoint to the query in `[0.0, 1.0]`.
    public var score: Float

    public init(timestamp: Float, score: Float) {
        self.timestamp = timestamp
        self.score = score
    }
}

/// The fingerprint of one window of audio.
public struct WindowedFingerprint: Equatable, Hashable, Sendable {
    /// Start of the window, in milliseconds from the beginning of the input.
    public var timestampMs: UInt32
    /// The requested window duration in milliseconds.
    public var durationMs: UInt32
    /// The window's fingerprint hashes. Short windows that cover too few
    /// analysis frames produce an empty array.
    public var hashes: [UInt32]

    public init(timestampMs: UInt32, durationMs: UInt32, hashes: [UInt32]) {
        self.timestampMs = timestampMs
        self.durationMs = durationMs
        self.hashes = hashes
    }
}

/// Errors thrown by fingerprinting entry points.
///
/// Each case carries a human-readable `message` (also surfaced through
/// `LocalizedError`).
public enum FingerprintError: Error, Equatable, Hashable, Sendable {
    /// The input was recognized but could not be decoded.
    case DecodeError(message: String)
    /// The input is not a supported WAV or MP3 container.
    case UnsupportedFormat(message: String)
    /// A parameter was invalid (zero sample rate or channels, window too short, …).
    case InvalidInput(message: String)
    /// An input/output failure inside the decoder.
    case IoError(message: String)
    /// An unexpected failure inside the Rust core (a caught panic).
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

/// Similarity of two hash sequences at offset zero, in `[0.0, 1.0]`.
///
/// The score is the fraction of agreeing bits across the overlapping prefix of
/// the two sequences (the comparison stops at the shorter one). Either
/// sequence being empty scores `0.0`.
public func compareHashes(hashes1: [UInt32], hashes2: [UInt32]) -> Float {
    hashes1.withUnsafeBufferPointer { first in
        hashes2.withUnsafeBufferPointer { second in
            fingerprint_ffi_compare_hashes(first.baseAddress, first.count, second.baseAddress, second.count)
        }
    }
}

/// The best ``compareHashes(hashes1:hashes2:)`` score across relative shifts of
/// up to `maxDrift` hash positions in either direction, tolerating timing
/// misalignment between the two sequences.
public func compareHashesWithDrift(hashes1: [UInt32], hashes2: [UInt32], maxDrift: UInt32) -> Float {
    hashes1.withUnsafeBufferPointer { first in
        hashes2.withUnsafeBufferPointer { second in
            fingerprint_ffi_compare_hashes_with_drift(first.baseAddress, first.count, second.baseAddress, second.count, maxDrift)
        }
    }
}

/// Deserializes a fingerprint previously produced by
/// ``fingerprintToBytes(hashes:durationMs:)``.
///
/// - Returns: The decoded fingerprint, or `nil` for malformed or truncated
///   input (this function never throws). Trailing bytes past the declared
///   payload are ignored.
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

/// Serializes hashes into the compact little-endian binary fingerprint format:
/// `u32 durationMs`, `u32 hashCount`, then the hashes.
public func fingerprintToBytes(hashes: [UInt32], durationMs: UInt32) -> Data {
    let bytes = hashes.withUnsafeBufferPointer { buffer in
        fingerprint_ffi_to_bytes(buffer.baseAddress, buffer.count, durationMs)
    }
    return takeData(bytes)
}

/// Version string of the underlying Rust core (e.g. `"fingerprint_core 0.2.0"`).
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

private func takeHandleResult(_ result: FingerprintFfiHandleResult) throws(FingerprintError) -> UnsafeMutableRawPointer {
    if result.status != 0 {
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
