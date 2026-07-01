//! Core-level microbenchmarks.
//!
//! These measure the Rust fingerprinting hot paths directly (the existing
//! benchmarks live on the Swift side and go through the FFI), which makes it
//! possible to quantify the effect of output-preserving performance work such
//! as reusing the FFT plan across windows or precomputing the chroma table.
//!
//! Run with: `cargo bench -p fingerprint-core`

use std::f32::consts::PI;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fingerprint_core::fingerprint::streaming::StreamingFingerprinter;
use fingerprint_core::{
    compare_hashes, compare_hashes_with_drift, fingerprint_from_bytes, fingerprint_samples,
    fingerprint_to_bytes, fingerprint_windows, TARGET_SAMPLE_RATE,
};

fn signal(sample_rate: u32, seconds: f32) -> Vec<f32> {
    let count = (sample_rate as f32 * seconds) as usize;
    (0..count)
        .map(|index| {
            let t = index as f32 / sample_rate as f32;
            let chirp = (2.0 * PI * (200.0 + 300.0 * t) * t).sin();
            let tone = 0.5 * (2.0 * PI * 660.0 * t).sin();
            0.4 * (chirp + tone)
        })
        .collect()
}

fn bench_samples(c: &mut Criterion) {
    let mut group = c.benchmark_group("fingerprint_samples");
    for seconds in [1.0_f32, 5.0] {
        let samples = signal(TARGET_SAMPLE_RATE, seconds);
        group.throughput(Throughput::Elements(samples.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{seconds}s")),
            &samples,
            |b, samples| {
                b.iter(|| fingerprint_samples(std::hint::black_box(samples), 1_000));
            },
        );
    }
    group.finish();
}

fn bench_windows(c: &mut Criterion) {
    // The overlapping-window path is where a per-call FFT plan hurts most.
    let samples = signal(TARGET_SAMPLE_RATE, 30.0);
    c.bench_function("fingerprint_windows/30s_1000ms_500ms", |b| {
        b.iter(|| fingerprint_windows(std::hint::black_box(&samples), 1_000, 500).unwrap());
    });
}

fn bench_streaming(c: &mut Criterion) {
    let samples = signal(44_100, 10.0);
    c.bench_function("streaming_push/44100_stereo_10s", |b| {
        b.iter(|| {
            let mut streaming = StreamingFingerprinter::new(44_100, 2).unwrap();
            let mut hashes = 0usize;
            for chunk in samples.chunks(8_192) {
                hashes += streaming
                    .push_samples_f32(std::hint::black_box(chunk), 2)
                    .len();
            }
            hashes + streaming.flush().len()
        });
    });
}

fn bench_matching(c: &mut Criterion) {
    let a: Vec<u32> = (0..4_096).map(|i| i as u32).collect();
    let b_hashes: Vec<u32> = (0..4_096).map(|i| (i as u32) ^ 0x5555_5555).collect();
    c.bench_function("compare_hashes/4096", |b| {
        b.iter(|| compare_hashes(std::hint::black_box(&a), std::hint::black_box(&b_hashes)));
    });
    c.bench_function("compare_hashes_with_drift/4096_drift16", |b| {
        b.iter(|| {
            compare_hashes_with_drift(
                std::hint::black_box(&a),
                std::hint::black_box(&b_hashes),
                16,
            )
        });
    });
}

fn bench_serialization(c: &mut Criterion) {
    let hashes: Vec<u32> = (0..4_096).map(|i| i as u32).collect();
    let encoded = fingerprint_to_bytes(&hashes, 1_000);
    c.bench_function("fingerprint_to_bytes/4096", |b| {
        b.iter(|| fingerprint_to_bytes(std::hint::black_box(&hashes), 1_000));
    });
    c.bench_function("fingerprint_from_bytes/4096", |b| {
        b.iter(|| fingerprint_from_bytes(std::hint::black_box(&encoded)));
    });
}

criterion_group!(
    benches,
    bench_samples,
    bench_windows,
    bench_streaming,
    bench_matching,
    bench_serialization
);
criterion_main!(benches);
