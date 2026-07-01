use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::Mutex;

use fingerprint_core::{
    compare_hashes, compare_hashes_with_drift, fingerprint_from_bytes, fingerprint_to_bytes,
    fingerprint_version, CheckpointMatcher, FingerprintError, Fingerprinter, MatchResult,
    StreamingFingerprinter, StreamingWindowedFingerprinter, WindowedFingerprint,
};

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
    drop_u8_vec(bytes);
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_u32_array(array: FingerprintFfiU32Array) {
    drop_u32_vec(array);
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_match_array(array: FingerprintFfiMatchArray) {
    if array.ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(array.ptr, array.len, array.cap));
    }
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_free_windowed_array(array: FingerprintFfiWindowedArray) {
    if array.ptr.is_null() {
        return;
    }
    unsafe {
        let items = Vec::from_raw_parts(array.ptr, array.len, array.cap);
        for item in &items {
            drop_u32_vec(ptr::read(&item.hashes));
        }
        drop(items);
    }
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_version() -> FingerprintFfiBytes {
    vec_to_bytes(fingerprint_version().as_bytes().to_vec())
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
    let hashes = unsafe { u32_slice(hashes, len) };
    vec_to_bytes(fingerprint_to_bytes(hashes, duration_ms))
}

/// # Safety
///
/// `data` must be null with `len == 0` or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fingerprint_ffi_from_bytes(
    data: *const u8,
    len: usize,
) -> FingerprintFfiFingerprintResult {
    let data = unsafe { u8_slice(data, len) };
    match fingerprint_from_bytes(data) {
        Some(fingerprint) => FingerprintFfiFingerprintResult {
            found: 1,
            duration_ms: fingerprint.duration_ms,
            hashes: vec_to_u32_array(fingerprint.hashes),
        },
        None => FingerprintFfiFingerprintResult {
            found: 0,
            duration_ms: 0,
            hashes: FingerprintFfiU32Array::empty(),
        },
    }
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
    let first = unsafe { u32_slice(first, first_len) };
    let second = unsafe { u32_slice(second, second_len) };
    compare_hashes(first, second)
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
    let first = unsafe { u32_slice(first, first_len) };
    let second = unsafe { u32_slice(second, second_len) };
    compare_hashes_with_drift(first, second, max_drift)
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
    let data = unsafe { u8_slice(data, len) };
    match Fingerprinter::new().fingerprint_data_windowed(
        data,
        window_duration_ms,
        window_interval_ms,
    ) {
        Ok(windows) => FingerprintFfiWindowedArrayResult {
            status: 0,
            message: FingerprintFfiBytes::empty(),
            windows: vec_to_windowed_array(windows),
        },
        Err(error) => FingerprintFfiWindowedArrayResult {
            status: error_status(&error),
            message: vec_to_bytes(error.message().as_bytes().to_vec()),
            windows: FingerprintFfiWindowedArray::empty(),
        },
    }
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_new(max_drift: u32) -> *mut c_void {
    Box::into_raw(Box::new(Mutex::new(CheckpointMatcher::with_drift(
        max_drift,
    )))) as *mut c_void
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_free(handle: *mut c_void) {
    drop_mutex_handle::<CheckpointMatcher>(handle);
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
    with_handle(handle, |matcher: &mut CheckpointMatcher| {
        matcher.add(
            timestamp,
            unsafe { u32_slice(hashes, len) }.to_vec(),
            duration,
        );
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_clear(handle: *mut c_void) {
    with_handle(handle, |matcher: &mut CheckpointMatcher| matcher.clear());
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_count(handle: *mut c_void) -> u32 {
    with_handle_result(handle, 0, |matcher: &mut CheckpointMatcher| matcher.count())
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_checkpoint_set_drift(handle: *mut c_void, max_drift: u32) {
    with_handle(handle, |matcher: &mut CheckpointMatcher| {
        matcher.set_drift(max_drift)
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
    let query_hashes = unsafe { u32_slice(query_hashes, len) };
    let results = with_handle_result(handle, Vec::new(), |matcher: &mut CheckpointMatcher| {
        matcher.find_top_matches(query_hashes, max_results)
    });
    vec_to_match_array(results)
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_new(
    sample_rate: u32,
    channels: u16,
) -> FingerprintFfiHandleResult {
    mutex_handle_result(StreamingFingerprinter::new(sample_rate, channels))
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_free(handle: *mut c_void) {
    drop_mutex_handle::<StreamingFingerprinter>(handle);
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_duration_ms(handle: *mut c_void) -> u32 {
    with_handle_result(handle, 0, |fingerprinter: &mut StreamingFingerprinter| {
        fingerprinter.duration_ms()
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
    let samples = unsafe { i16_slice(samples, len) };
    let hashes = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingFingerprinter| fingerprinter.push_samples(samples),
    );
    vec_to_u32_array(hashes)
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
    let samples = unsafe { f32_slice(samples, len) };
    let hashes = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingFingerprinter| {
            fingerprinter.push_samples_f32(samples, channels)
        },
    );
    vec_to_u32_array(hashes)
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_flush(handle: *mut c_void) -> FingerprintFfiU32Array {
    let hashes = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingFingerprinter| fingerprinter.flush(),
    );
    vec_to_u32_array(hashes)
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_reset(handle: *mut c_void) {
    with_handle(handle, |fingerprinter: &mut StreamingFingerprinter| {
        fingerprinter.reset()
    });
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_new(
    sample_rate: u32,
    channels: u16,
    window_duration_ms: u32,
    window_interval_ms: u32,
) -> FingerprintFfiHandleResult {
    mutex_handle_result(StreamingWindowedFingerprinter::new(
        sample_rate,
        channels,
        window_duration_ms,
        window_interval_ms,
    ))
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_free(handle: *mut c_void) {
    drop_mutex_handle::<StreamingWindowedFingerprinter>(handle);
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_duration_ms(handle: *mut c_void) -> u32 {
    with_handle_result(
        handle,
        0,
        |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.duration_ms(),
    )
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
    let samples = unsafe { i16_slice(samples, len) };
    let windows = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.push_samples(samples),
    );
    vec_to_windowed_array(windows)
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
    let samples = unsafe { f32_slice(samples, len) };
    let windows = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingWindowedFingerprinter| {
            fingerprinter.push_samples_f32(samples, channels)
        },
    );
    vec_to_windowed_array(windows)
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_flush(
    handle: *mut c_void,
) -> FingerprintFfiWindowedArray {
    let windows = with_handle_result(
        handle,
        Vec::new(),
        |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.flush(),
    );
    vec_to_windowed_array(windows)
}

#[no_mangle]
pub extern "C" fn fingerprint_ffi_streaming_windowed_reset(handle: *mut c_void) {
    with_handle(
        handle,
        |fingerprinter: &mut StreamingWindowedFingerprinter| fingerprinter.reset(),
    );
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

impl FingerprintFfiWindowedArray {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

fn error_status(error: &FingerprintError) -> u32 {
    match error {
        FingerprintError::DecodeError { .. } => 1,
        FingerprintError::UnsupportedFormat { .. } => 2,
        FingerprintError::InvalidInput { .. } => 3,
        FingerprintError::IoError { .. } => 4,
    }
}

fn mutex_handle_result<T>(result: Result<T, FingerprintError>) -> FingerprintFfiHandleResult {
    match result {
        Ok(value) => FingerprintFfiHandleResult {
            status: 0,
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

fn with_handle<T>(handle: *mut c_void, operation: impl FnOnce(&mut T)) {
    if handle.is_null() {
        return;
    }
    let mutex = unsafe { &*(handle as *mut Mutex<T>) };
    if let Ok(mut value) = mutex.lock() {
        operation(&mut value);
    }
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
    match mutex.lock() {
        Ok(mut value) => operation(&mut value),
        Err(_) => fallback,
    }
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
