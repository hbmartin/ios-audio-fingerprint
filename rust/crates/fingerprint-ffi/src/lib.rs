use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::sync::Mutex;

use fingerprint_core::{
    compare_hashes, compare_hashes_with_drift, fingerprint_from_bytes, fingerprint_to_bytes,
    fingerprint_version, CheckpointMatcher, FingerprintError, Fingerprinter, MatchResult,
    StreamingFingerprinter, StreamingWindowedFingerprinter, WindowedFingerprint,
};

const STATUS_OK: u32 = 0;
const STATUS_DECODE_ERROR: u32 = 1;
const STATUS_UNSUPPORTED_FORMAT: u32 = 2;
const STATUS_INVALID_INPUT: u32 = 3;
const STATUS_IO_ERROR: u32 = 4;
const STATUS_INTERNAL_ERROR: u32 = 5;

#[repr(C)]
pub struct FingerprintFfiBytes {
    ptr: *mut u8,
    len: usize,
    cap: usize,
}

#[repr(C)]
pub struct FingerprintFfiU32Array {
    ptr: *mut u32,
    len: usize,
    cap: usize,
}

#[repr(C)]
pub struct FingerprintFfiFingerprintResult {
    found: u8,
    duration_ms: u32,
    hashes: FingerprintFfiU32Array,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FingerprintFfiMatchResult {
    timestamp: f32,
    score: f32,
}

#[repr(C)]
pub struct FingerprintFfiMatchArray {
    ptr: *mut FingerprintFfiMatchResult,
    len: usize,
    cap: usize,
}

#[repr(C)]
pub struct FingerprintFfiWindowedFingerprint {
    timestamp_ms: u32,
    duration_ms: u32,
    hashes: FingerprintFfiU32Array,
}

#[repr(C)]
pub struct FingerprintFfiWindowedArray {
    ptr: *mut FingerprintFfiWindowedFingerprint,
    len: usize,
    cap: usize,
}

#[repr(C)]
pub struct FingerprintFfiWindowedArrayResult {
    status: u32,
    message: FingerprintFfiBytes,
    windows: FingerprintFfiWindowedArray,
}

#[repr(C)]
pub struct FingerprintFfiHandleResult {
    status: u32,
    message: FingerprintFfiBytes,
    handle: *mut c_void,
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_bytes(bytes: FingerprintFfiBytes) {
    ffi_guard((), || drop_u8_vec(bytes));
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_u32_array(array: FingerprintFfiU32Array) {
    ffi_guard((), || drop_u32_vec(array));
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_match_array(array: FingerprintFfiMatchArray) {
    ffi_guard((), || {
        if array.ptr.is_null() {
            return;
        }
        unsafe {
            drop(Vec::from_raw_parts(array.ptr, array.len, array.cap));
        }
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_windowed_array(array: FingerprintFfiWindowedArray) {
    ffi_guard((), || {
        if array.ptr.is_null() {
            return;
        }
        unsafe {
            let items = Vec::from_raw_parts(array.ptr, array.len, array.cap);
            for item in items {
                drop_u32_vec(item.hashes);
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_version() -> FingerprintFfiBytes {
    ffi_guard(FingerprintFfiBytes::empty(), || {
        vec_to_bytes(fingerprint_version().as_bytes().to_vec())
    })
}

/// # Safety
///
/// `hashes` must be null with `len == 0` or valid for `len` `u32` values.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_to_bytes(
    hashes: *const u32,
    len: usize,
    duration_ms: u32,
) -> FingerprintFfiBytes {
    ffi_guard(FingerprintFfiBytes::empty(), || {
        let hashes = unsafe { u32_slice(hashes, len) };
        vec_to_bytes(fingerprint_to_bytes(hashes, duration_ms))
    })
}

/// # Safety
///
/// `data` must be null with `len == 0` or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_from_bytes(
    data: *const u8,
    len: usize,
) -> FingerprintFfiFingerprintResult {
    ffi_guard(FingerprintFfiFingerprintResult::not_found(), || {
        let data = unsafe { u8_slice(data, len) };
        match fingerprint_from_bytes(data) {
            Some(fingerprint) => FingerprintFfiFingerprintResult {
                found: 1,
                duration_ms: fingerprint.duration_ms,
                hashes: vec_to_u32_array(fingerprint.hashes),
            },
            None => FingerprintFfiFingerprintResult::not_found(),
        }
    })
}

/// # Safety
///
/// `first` and `second` must be null with a zero matching length or valid for
/// their corresponding lengths.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_compare_hashes(
    first: *const u32,
    first_len: usize,
    second: *const u32,
    second_len: usize,
) -> f32 {
    ffi_guard(0.0, || {
        let first = unsafe { u32_slice(first, first_len) };
        let second = unsafe { u32_slice(second, second_len) };
        compare_hashes(first, second)
    })
}

/// # Safety
///
/// `first` and `second` must be null with a zero matching length or valid for
/// their corresponding lengths.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_compare_hashes_with_drift(
    first: *const u32,
    first_len: usize,
    second: *const u32,
    second_len: usize,
    max_drift: u32,
) -> f32 {
    ffi_guard(0.0, || {
        let first = unsafe { u32_slice(first, first_len) };
        let second = unsafe { u32_slice(second, second_len) };
        compare_hashes_with_drift(first, second, max_drift)
    })
}

/// # Safety
///
/// `data` must be null with `len == 0` or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_fingerprint_data_windowed(
    data: *const u8,
    len: usize,
    window_duration_ms: u32,
    window_interval_ms: u32,
) -> FingerprintFfiWindowedArrayResult {
    ffi_guard(FingerprintFfiWindowedArrayResult::internal_error(), || {
        let data = unsafe { u8_slice(data, len) };
        match Fingerprinter::new().fingerprint_data_windowed(
            data,
            window_duration_ms,
            window_interval_ms,
        ) {
            Ok(windows) => FingerprintFfiWindowedArrayResult {
                status: STATUS_OK,
                message: FingerprintFfiBytes::empty(),
                windows: vec_to_windowed_array(windows),
            },
            Err(error) => FingerprintFfiWindowedArrayResult {
                status: error_status(&error),
                message: vec_to_bytes(error.message().as_bytes().to_vec()),
                windows: FingerprintFfiWindowedArray::empty(),
            },
        }
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_new(max_drift: u32) -> *mut c_void {
    ffi_guard(ptr::null_mut(), || {
        Box::into_raw(Box::new(Mutex::new(CheckpointMatcher::with_drift(
            max_drift,
        )))) as *mut c_void
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_free(handle: *mut c_void) {
    ffi_guard((), || drop_mutex_handle::<CheckpointMatcher>(handle));
}

/// # Safety
///
/// `handle` must be a live checkpoint handle, and `hashes` must be null with
/// `len == 0` or valid for `len` `u32` values. The handle must not be freed
/// while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_checkpoint_add(
    handle: *mut c_void,
    timestamp: f32,
    hashes: *const u32,
    len: usize,
    duration: f32,
) {
    ffi_guard((), || {
        with_handle(handle, |matcher: &mut CheckpointMatcher| {
            matcher.add(
                timestamp,
                unsafe { u32_slice(hashes, len) }.to_vec(),
                duration,
            );
        });
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_clear(handle: *mut c_void) {
    ffi_guard((), || {
        with_handle(handle, |matcher: &mut CheckpointMatcher| matcher.clear());
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_count(handle: *mut c_void) -> u32 {
    ffi_guard(0, || {
        with_handle_result(handle, 0, |matcher: &mut CheckpointMatcher| matcher.count())
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_set_drift(handle: *mut c_void, max_drift: u32) {
    ffi_guard((), || {
        with_handle(handle, |matcher: &mut CheckpointMatcher| {
            matcher.set_drift(max_drift)
        });
    });
}

/// # Safety
///
/// `handle` must be a live checkpoint handle, and `query_hashes` must be null
/// with `len == 0` or valid for `len` `u32` values. The handle must not be
/// freed while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_checkpoint_find_top_matches(
    handle: *mut c_void,
    query_hashes: *const u32,
    len: usize,
    max_results: u32,
) -> FingerprintFfiMatchArray {
    ffi_guard(FingerprintFfiMatchArray::empty(), || {
        let query_hashes = unsafe { u32_slice(query_hashes, len) };
        let results = with_handle_result(handle, Vec::new(), |matcher: &mut CheckpointMatcher| {
            matcher.find_top_matches(query_hashes, max_results)
        });
        vec_to_match_array(results)
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_new(
    sample_rate: u32,
    channels: u16,
) -> FingerprintFfiHandleResult {
    ffi_guard(FingerprintFfiHandleResult::internal_error(), || {
        mutex_handle_result(StreamingFingerprinter::new(sample_rate, channels))
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_free(handle: *mut c_void) {
    ffi_guard((), || drop_mutex_handle::<StreamingFingerprinter>(handle));
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_duration_ms(handle: *mut c_void) -> u32 {
    ffi_guard(0, || {
        with_handle_result(handle, 0, |fingerprinter: &mut StreamingFingerprinter| {
            fingerprinter.duration_ms()
        })
    })
}

/// # Safety
///
/// `handle` must be a live streaming handle, and `samples` must be null with
/// `len == 0` or valid for `len` `i16` values. The handle must not be freed
/// while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_streaming_push_i16(
    handle: *mut c_void,
    samples: *const i16,
    len: usize,
) -> FingerprintFfiU32Array {
    ffi_guard(FingerprintFfiU32Array::empty(), || {
        let samples = unsafe { i16_slice(samples, len) };
        let hashes = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingFingerprinter| fingerprinter.push_samples(samples),
        );
        vec_to_u32_array(hashes)
    })
}

/// # Safety
///
/// `handle` must be a live streaming handle, and `samples` must be null with
/// `len == 0` or valid for `len` `f32` values. The handle must not be freed
/// while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_streaming_push_f32(
    handle: *mut c_void,
    samples: *const f32,
    len: usize,
    channels: u16,
) -> FingerprintFfiU32Array {
    ffi_guard(FingerprintFfiU32Array::empty(), || {
        let samples = unsafe { f32_slice(samples, len) };
        let hashes = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingFingerprinter| {
                fingerprinter.push_samples_f32(samples, channels)
            },
        );
        vec_to_u32_array(hashes)
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_flush(handle: *mut c_void) -> FingerprintFfiU32Array {
    ffi_guard(FingerprintFfiU32Array::empty(), || {
        let hashes = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingFingerprinter| fingerprinter.flush(),
        );
        vec_to_u32_array(hashes)
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_reset(handle: *mut c_void) {
    ffi_guard((), || {
        with_handle(handle, |fingerprinter: &mut StreamingFingerprinter| {
            fingerprinter.reset()
        });
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_new(
    sample_rate: u32,
    channels: u16,
    window_duration_ms: u32,
    window_interval_ms: u32,
) -> FingerprintFfiHandleResult {
    ffi_guard(FingerprintFfiHandleResult::internal_error(), || {
        mutex_handle_result(StreamingWindowedFingerprinter::new(
            sample_rate,
            channels,
            window_duration_ms,
            window_interval_ms,
        ))
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_free(handle: *mut c_void) {
    ffi_guard((), || {
        drop_mutex_handle::<StreamingWindowedFingerprinter>(handle)
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_duration_ms(handle: *mut c_void) -> u32 {
    ffi_guard(0, || {
        with_handle_result(
            handle,
            0,
            |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.duration_ms(),
        )
    })
}

/// # Safety
///
/// `handle` must be a live windowed streaming handle, and `samples` must be
/// null with `len == 0` or valid for `len` `i16` values. The handle must not be
/// freed while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_streaming_windowed_push_i16(
    handle: *mut c_void,
    samples: *const i16,
    len: usize,
) -> FingerprintFfiWindowedArray {
    ffi_guard(FingerprintFfiWindowedArray::empty(), || {
        let samples = unsafe { i16_slice(samples, len) };
        let windows = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingWindowedFingerprinter| {
                fingerprinter.push_samples(samples)
            },
        );
        vec_to_windowed_array(windows)
    })
}

/// # Safety
///
/// `handle` must be a live windowed streaming handle, and `samples` must be
/// null with `len == 0` or valid for `len` `f32` values. The handle must not be
/// freed while this call is in flight.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_streaming_windowed_push_f32(
    handle: *mut c_void,
    samples: *const f32,
    len: usize,
    channels: u16,
) -> FingerprintFfiWindowedArray {
    ffi_guard(FingerprintFfiWindowedArray::empty(), || {
        let samples = unsafe { f32_slice(samples, len) };
        let windows = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingWindowedFingerprinter| {
                fingerprinter.push_samples_f32(samples, channels)
            },
        );
        vec_to_windowed_array(windows)
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_flush(
    handle: *mut c_void,
) -> FingerprintFfiWindowedArray {
    ffi_guard(FingerprintFfiWindowedArray::empty(), || {
        let windows = with_handle_result(
            handle,
            Vec::new(),
            |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.flush(),
        );
        vec_to_windowed_array(windows)
    })
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_reset(handle: *mut c_void) {
    ffi_guard((), || {
        with_handle(
            handle,
            |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.reset(),
        );
    });
}

impl FingerprintFfiBytes {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

impl FingerprintFfiU32Array {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

impl FingerprintFfiMatchArray {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

impl FingerprintFfiWindowedArray {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

impl FingerprintFfiFingerprintResult {
    fn not_found() -> Self {
        Self {
            found: 0,
            duration_ms: 0,
            hashes: FingerprintFfiU32Array::empty(),
        }
    }
}

impl FingerprintFfiWindowedArrayResult {
    /// Fallback returned when a panic is caught at the boundary. The message is
    /// empty so the fallback allocates nothing (a forgotten buffer would leak on
    /// the success path).
    fn internal_error() -> Self {
        Self {
            status: STATUS_INTERNAL_ERROR,
            message: FingerprintFfiBytes::empty(),
            windows: FingerprintFfiWindowedArray::empty(),
        }
    }
}

impl FingerprintFfiHandleResult {
    /// Fallback returned when a panic is caught while constructing a handle.
    fn internal_error() -> Self {
        Self {
            status: STATUS_INTERNAL_ERROR,
            message: FingerprintFfiBytes::empty(),
            handle: ptr::null_mut(),
        }
    }
}

/// Run an FFI entry point, catching any Rust panic so it never unwinds across
/// the C ABI (which is undefined behavior). On a caught panic the `fallback` is
/// returned. `fallback` must own no heap buffers that would leak on success — use
/// the `empty()`/`not_found()`/`internal_error()` constructors, all of which are
/// allocation-free.
fn ffi_guard<R>(fallback: R, body: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => fallback,
    }
}

fn error_status(error: &FingerprintError) -> u32 {
    match error {
        FingerprintError::DecodeError { .. } => STATUS_DECODE_ERROR,
        FingerprintError::UnsupportedFormat { .. } => STATUS_UNSUPPORTED_FORMAT,
        FingerprintError::InvalidInput { .. } => STATUS_INVALID_INPUT,
        FingerprintError::IoError { .. } => STATUS_IO_ERROR,
    }
}

fn mutex_handle_result<T>(result: Result<T, FingerprintError>) -> FingerprintFfiHandleResult {
    match result {
        Ok(value) => FingerprintFfiHandleResult {
            status: STATUS_OK,
            message: FingerprintFfiBytes::empty(),
            handle: Box::into_raw(Box::new(Mutex::new(value))) as *mut c_void,
        },
        Err(error) => FingerprintFfiHandleResult {
            status: error_status(&error),
            message: vec_to_bytes(error.message().as_bytes().to_vec()),
            handle: ptr::null_mut(),
        },
    }
}

fn vec_to_bytes(mut data: Vec<u8>) -> FingerprintFfiBytes {
    let bytes = FingerprintFfiBytes {
        ptr: data.as_mut_ptr(),
        len: data.len(),
        cap: data.capacity(),
    };
    std::mem::forget(data);
    bytes
}

fn vec_to_u32_array(mut data: Vec<u32>) -> FingerprintFfiU32Array {
    let array = FingerprintFfiU32Array {
        ptr: data.as_mut_ptr(),
        len: data.len(),
        cap: data.capacity(),
    };
    std::mem::forget(data);
    array
}

fn vec_to_match_array(results: Vec<MatchResult>) -> FingerprintFfiMatchArray {
    let mut data: Vec<FingerprintFfiMatchResult> = results
        .into_iter()
        .map(|result| FingerprintFfiMatchResult {
            timestamp: result.timestamp,
            score: result.score,
        })
        .collect();
    let array = FingerprintFfiMatchArray {
        ptr: data.as_mut_ptr(),
        len: data.len(),
        cap: data.capacity(),
    };
    std::mem::forget(data);
    array
}

fn vec_to_windowed_array(windows: Vec<WindowedFingerprint>) -> FingerprintFfiWindowedArray {
    let mut data: Vec<FingerprintFfiWindowedFingerprint> = windows
        .into_iter()
        .map(|window| FingerprintFfiWindowedFingerprint {
            timestamp_ms: window.timestamp_ms,
            duration_ms: window.duration_ms,
            hashes: vec_to_u32_array(window.hashes),
        })
        .collect();
    let array = FingerprintFfiWindowedArray {
        ptr: data.as_mut_ptr(),
        len: data.len(),
        cap: data.capacity(),
    };
    std::mem::forget(data);
    array
}

fn drop_u8_vec(bytes: FingerprintFfiBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
    }
}

fn drop_u32_vec(array: FingerprintFfiU32Array) {
    if array.ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(array.ptr, array.len, array.cap));
    }
}

fn drop_mutex_handle<T>(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut Mutex<T>));
    }
}

/// Lock a handle's mutex, recovering the guarded value even if a previous panic
/// poisoned it. Poisoning is now unlikely because `ffi_guard` catches panics
/// before they escape, but recovering keeps a handle usable rather than turning
/// every subsequent call into a silent no-op.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_handle<T>(handle: *mut c_void, operation: impl FnOnce(&mut T)) {
    if handle.is_null() {
        return;
    }
    let mutex = unsafe { &*(handle as *mut Mutex<T>) };
    operation(&mut lock_recover(mutex));
}

fn with_handle_result<T, R>(
    handle: *mut c_void,
    fallback: R,
    operation: impl FnOnce(&mut T) -> R,
) -> R {
    if handle.is_null() {
        return fallback;
    }
    let mutex = unsafe { &*(handle as *mut Mutex<T>) };
    operation(&mut lock_recover(mutex))
}

unsafe fn u8_slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

unsafe fn u32_slice<'a>(ptr: *const u32, len: usize) -> &'a [u32] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

unsafe fn i16_slice<'a>(ptr: *const i16, len: usize) -> &'a [i16] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

unsafe fn f32_slice<'a>(ptr: *const f32, len: usize) -> &'a [f32] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_guard_returns_body_value_on_success() {
        assert_eq!(ffi_guard(0u32, || 42), 42);
    }

    #[test]
    fn ffi_guard_returns_fallback_on_panic() {
        assert_eq!(ffi_guard(7u32, || panic!("boom")), 7);
    }

    #[test]
    fn internal_error_status_is_distinct_from_io_errors() {
        assert_eq!(
            FingerprintFfiHandleResult::internal_error().status,
            STATUS_INTERNAL_ERROR
        );
        assert_eq!(
            FingerprintFfiWindowedArrayResult::internal_error().status,
            STATUS_INTERNAL_ERROR
        );
        assert_ne!(
            error_status(&FingerprintError::io("disk")),
            STATUS_INTERNAL_ERROR
        );
    }

    #[test]
    fn lock_recover_recovers_a_poisoned_mutex() {
        let mutex = Mutex::new(5u32);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("poison the lock while held");
        }));
        assert!(mutex.is_poisoned());
        *lock_recover(&mutex) += 1;
        assert_eq!(*lock_recover(&mutex), 6);
    }

    #[test]
    fn null_handle_operations_are_safe_no_ops() {
        assert_eq!(fingerprint_ffi_checkpoint_count(ptr::null_mut()), 0);
        assert_eq!(fingerprint_ffi_streaming_duration_ms(ptr::null_mut()), 0);
        fingerprint_ffi_checkpoint_clear(ptr::null_mut());
        fingerprint_ffi_checkpoint_free(ptr::null_mut());
        fingerprint_ffi_streaming_free(ptr::null_mut());
    }

    #[test]
    fn checkpoint_roundtrips_through_the_c_api() {
        let handle = fingerprint_ffi_checkpoint_new(1);
        assert!(!handle.is_null());

        let hashes = [0u32, 1, 2];
        unsafe {
            fingerprint_ffi_checkpoint_add(handle, 10.0, hashes.as_ptr(), hashes.len(), 1.0);
        }
        assert_eq!(fingerprint_ffi_checkpoint_count(handle), 1);

        let matches = unsafe {
            fingerprint_ffi_checkpoint_find_top_matches(handle, hashes.as_ptr(), hashes.len(), 5)
        };
        assert_eq!(matches.len, 1);
        fingerprint_ffi_free_match_array(matches);
        fingerprint_ffi_checkpoint_free(handle);
    }

    #[test]
    fn streaming_handle_still_usable_after_lock_poisoning() {
        // Poison the handle's mutex directly, then confirm the FFI recovers it
        // instead of degrading to a permanent no-op.
        let result = fingerprint_ffi_streaming_new(TARGET_RATE, 1);
        assert_eq!(result.status, STATUS_OK);
        let handle = result.handle;

        let mutex = unsafe { &*(handle as *mut Mutex<StreamingFingerprinter>) };
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("poison while held");
        }));
        assert!(mutex.is_poisoned());

        let samples = [0.1f32; 8_192];
        let hashes = unsafe {
            fingerprint_ffi_streaming_push_f32(handle, samples.as_ptr(), samples.len(), 1)
        };
        // The recovered handle keeps accumulating duration rather than no-oping.
        assert!(fingerprint_ffi_streaming_duration_ms(handle) > 0);
        fingerprint_ffi_free_u32_array(hashes);
        fingerprint_ffi_streaming_free(handle);
    }

    const TARGET_RATE: u32 = 11_025;
}
