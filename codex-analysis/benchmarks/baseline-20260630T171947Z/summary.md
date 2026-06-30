# Fingerprint Benchmark Results

- Label: `baseline-20260630T171947Z`
- Timestamp: `2026-06-30T17:19:50Z`
- Configuration: `release`
- Fingerprint version: `fingerprint_uniffi 0.1.0`
- Swift: `Apple Swift version 6.2.4 (swiftlang-6.2.4.1.4 clang-1700.6.4.2)`
- OS: `Version 15.7.7 (Build 24G720)`
- CPUs: `14` active / `14` total
- Memory: `51539607552` bytes
- Iterations: `50` measured, `10` warmups per benchmark

| Benchmark | Category | Median ms | Mean ms | P90 ms | Min ms | Max ms | StdDev ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `windowed_wav_fingerprinting_six_seconds` | fingerprinting | 10.937728 | 10.949548 | 11.166795 | 10.644000 | 11.565083 | 0.184063 |
| `streaming_windowed_fingerprinter_stereo_resample_six_seconds` | fingerprinting | 10.122813 | 10.154499 | 10.406392 | 9.840417 | 10.806250 | 0.181881 |
| `streaming_fingerprinter_stereo_f32_resample_five_seconds` | fingerprinting | 3.825584 | 3.827468 | 3.984108 | 3.541125 | 4.687291 | 0.180373 |
| `streaming_fingerprinter_mono_f32_five_seconds` | fingerprinting | 3.513354 | 3.539042 | 3.832396 | 3.252167 | 4.044542 | 0.191538 |
| `checkpoint_matcher_add_and_query` | matching | 0.889875 | 0.903642 | 1.021175 | 0.836959 | 1.090667 | 0.061673 |
| `compare_hashes_with_drift` | comparison | 0.393771 | 0.390932 | 0.408938 | 0.360625 | 0.463458 | 0.021342 |
| `serialization_round_trip_large_fingerprint` | serialization | 0.124979 | 0.133916 | 0.151734 | 0.114584 | 0.308083 | 0.029144 |
| `compare_hashes_large_equal_inputs` | comparison | 0.023458 | 0.023485 | 0.024066 | 0.020083 | 0.031041 | 0.002119 |
| `compare_hashes_large_different_inputs` | comparison | 0.022667 | 0.023399 | 0.027037 | 0.020833 | 0.030375 | 0.002534 |
| `serialization_round_trip_small_fingerprint` | serialization | 0.001125 | 0.001246 | 0.001125 | 0.001041 | 0.007958 | 0.000959 |
| `mp3_unsupported_fast_path` | format | 0.000250 | 0.000258 | 0.000292 | 0.000250 | 0.000292 | 0.000017 |

## Workloads

- `serialization_round_trip_small_fingerprint`: 128 deterministic hashes, 60s duration
- `serialization_round_trip_large_fingerprint`: 16,384 deterministic hashes, 60m duration
- `compare_hashes_large_equal_inputs`: 65,536 hashes compared against identical input
- `compare_hashes_large_different_inputs`: 65,536 hashes compared against a different deterministic input
- `compare_hashes_with_drift`: 8,192 hashes searched with a 64-hash offset and max drift 64
- `checkpoint_matcher_add_and_query`: 1,000 checkpoints, 256 hashes each, drift 4, top 10 query
- `streaming_fingerprinter_mono_f32_five_seconds`: 5s mono Float32 synthetic tone at 11,025 Hz
- `streaming_fingerprinter_stereo_f32_resample_five_seconds`: 5s stereo Float32 synthetic tone at 44,100 Hz, downmixed and resampled
- `windowed_wav_fingerprinting_six_seconds`: 6s mono 16-bit PCM WAV at 11,025 Hz, 2s windows every 500ms
- `streaming_windowed_fingerprinter_stereo_resample_six_seconds`: 6s stereo Float32 synthetic tone at 44,100 Hz in 44,100-sample chunks
- `mp3_unsupported_fast_path`: 4,100-byte MP3-like payload, unsupported format error path
