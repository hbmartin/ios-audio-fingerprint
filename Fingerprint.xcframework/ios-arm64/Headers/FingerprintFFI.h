#ifndef FINGERPRINT_FFI_H
#define FINGERPRINT_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FingerprintFfiBytes {
    uint8_t *ptr;
    size_t len;
    size_t cap;
} FingerprintFfiBytes;

typedef struct FingerprintFfiU32Array {
    uint32_t *ptr;
    size_t len;
    size_t cap;
} FingerprintFfiU32Array;

typedef struct FingerprintFfiFingerprintResult {
    uint8_t found;
    uint32_t duration_ms;
    FingerprintFfiU32Array hashes;
} FingerprintFfiFingerprintResult;

typedef struct FingerprintFfiMatchResult {
    float timestamp;
    float score;
} FingerprintFfiMatchResult;

typedef struct FingerprintFfiMatchArray {
    FingerprintFfiMatchResult *ptr;
    size_t len;
    size_t cap;
} FingerprintFfiMatchArray;

typedef struct FingerprintFfiWindowedFingerprint {
    uint32_t timestamp_ms;
    uint32_t duration_ms;
    FingerprintFfiU32Array hashes;
} FingerprintFfiWindowedFingerprint;

typedef struct FingerprintFfiWindowedArray {
    FingerprintFfiWindowedFingerprint *ptr;
    size_t len;
    size_t cap;
} FingerprintFfiWindowedArray;

typedef struct FingerprintFfiWindowedArrayResult {
    uint32_t status;
    FingerprintFfiBytes message;
    FingerprintFfiWindowedArray windows;
} FingerprintFfiWindowedArrayResult;

typedef struct FingerprintFfiHandleResult {
    uint32_t status;
    FingerprintFfiBytes message;
    void *handle;
} FingerprintFfiHandleResult;

/*
 Status codes returned by FingerprintFfiWindowedArrayResult and
 FingerprintFfiHandleResult:
 0 = success, 1 = decode error, 2 = unsupported format, 3 = invalid input,
 4 = I/O error, 5 = internal Rust panic caught at the FFI boundary.
 */

/*
 Returned FingerprintFfiBytes, FingerprintFfiU32Array, FingerprintFfiMatchArray,
 FingerprintFfiWindowedArray, and FingerprintFfiHandleResult.message values own
 their buffers. Release each owned value exactly once with the matching
 fingerprint_ffi_free_* function.
 */
void fingerprint_ffi_free_bytes(FingerprintFfiBytes bytes);
void fingerprint_ffi_free_u32_array(FingerprintFfiU32Array array);
void fingerprint_ffi_free_match_array(FingerprintFfiMatchArray array);
void fingerprint_ffi_free_windowed_array(FingerprintFfiWindowedArray array);

FingerprintFfiBytes fingerprint_ffi_version(void);
FingerprintFfiBytes fingerprint_ffi_to_bytes(const uint32_t *hashes, size_t len, uint32_t duration_ms);
FingerprintFfiFingerprintResult fingerprint_ffi_from_bytes(const uint8_t *data, size_t len);
float fingerprint_ffi_compare_hashes(const uint32_t *first, size_t first_len, const uint32_t *second, size_t second_len);
float fingerprint_ffi_compare_hashes_with_drift(const uint32_t *first, size_t first_len, const uint32_t *second, size_t second_len, uint32_t max_drift);
FingerprintFfiWindowedArrayResult fingerprint_ffi_fingerprint_data_windowed(const uint8_t *data, size_t len, uint32_t window_duration_ms, uint32_t window_interval_ms);

/*
 Opaque handles returned by fingerprint_ffi_*_new are owned by the caller and
 must be released exactly once with the matching fingerprint_ffi_*_free
 function. Do not call a handle's free function while another call using that
 same handle is still in flight.
 */
void *fingerprint_ffi_checkpoint_new(uint32_t max_drift);
void fingerprint_ffi_checkpoint_free(void *handle);
void fingerprint_ffi_checkpoint_add(void *handle, float timestamp, const uint32_t *hashes, size_t len, float duration);
void fingerprint_ffi_checkpoint_clear(void *handle);
uint32_t fingerprint_ffi_checkpoint_count(void *handle);
void fingerprint_ffi_checkpoint_set_drift(void *handle, uint32_t max_drift);
FingerprintFfiMatchArray fingerprint_ffi_checkpoint_find_top_matches(void *handle, const uint32_t *query_hashes, size_t len, uint32_t max_results);

/*
 FingerprintFfiHandleResult succeeds with status == 0 and a non-null handle.
 Callers must check status == 0 before passing handle to APIs such as
 fingerprint_ffi_streaming_free or fingerprint_ffi_streaming_push_i16. On
 failure, handle is null and message contains the error text.

 Streaming push_i16 APIs use the channel count captured when the handle was
 created. Provide interleaved samples that match that layout. Streaming push_f32
 APIs accept an explicit channels value for each call.
 */
FingerprintFfiHandleResult fingerprint_ffi_streaming_new(uint32_t sample_rate, uint16_t channels);
void fingerprint_ffi_streaming_free(void *handle);
uint32_t fingerprint_ffi_streaming_duration_ms(void *handle);
FingerprintFfiU32Array fingerprint_ffi_streaming_push_i16(void *handle, const int16_t *samples, size_t len);
FingerprintFfiU32Array fingerprint_ffi_streaming_push_f32(void *handle, const float *samples, size_t len, uint16_t channels);
FingerprintFfiU32Array fingerprint_ffi_streaming_flush(void *handle);
void fingerprint_ffi_streaming_reset(void *handle);

FingerprintFfiHandleResult fingerprint_ffi_streaming_windowed_new(uint32_t sample_rate, uint16_t channels, uint32_t window_duration_ms, uint32_t window_interval_ms);
void fingerprint_ffi_streaming_windowed_free(void *handle);
uint32_t fingerprint_ffi_streaming_windowed_duration_ms(void *handle);
FingerprintFfiWindowedArray fingerprint_ffi_streaming_windowed_push_i16(void *handle, const int16_t *samples, size_t len);
FingerprintFfiWindowedArray fingerprint_ffi_streaming_windowed_push_f32(void *handle, const float *samples, size_t len, uint16_t channels);
FingerprintFfiWindowedArray fingerprint_ffi_streaming_windowed_flush(void *handle);
void fingerprint_ffi_streaming_windowed_reset(void *handle);

#ifdef __cplusplus
}
#endif

#endif
