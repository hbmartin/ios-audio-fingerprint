# Benchmarking

Benchmark coverage exists in two forms:

- `Benchmarks/FingerprintBenchmarkRunner/main.swift` is the repeatable baseline
  runner. It writes JSON, CSV, and Markdown artifacts for future comparison.
- `Tests/FingerprintTests/FingerprintBenchmarkTests.swift` keeps the same
  workloads available through XCTest.

## Stored Baselines

The initial stored baseline is:

`codex-analysis/benchmarks/baseline-20260630T171947Z/`

It contains:

- `results.json`: full report with system metadata and every measured sample.
- `results.csv`: summary table for spreadsheet comparison.
- `summary.md`: human-readable report sorted by slowest median runtime.

## Repeatable Runner

Run the standalone benchmark runner in release mode:

```sh
swift run -c release FingerprintBenchmarkRunner \
  --output-dir codex-analysis/benchmarks \
  --label baseline-$(date -u +%Y%m%dT%H%M%SZ) \
  --iterations 50 \
  --warmups 10
```

For consistent comparisons, keep the iteration and warmup counts fixed and run
on an otherwise idle machine. Compare `results.csv` for high-level changes and
`results.json` when sample-level variance matters.

## XCTest Performance Checks

Run benchmark tests in release mode:

```sh
swift test -c release --filter FingerprintBenchmarkTests
```

The suite measures:

- small and large fingerprint serialization round trips,
- large direct hash comparisons,
- drift-compensated comparisons,
- checkpoint insertion and top-match query,
- mono streaming fingerprinting,
- stereo resampling plus streaming fingerprinting,
- WAV decode plus windowed fingerprinting,
- streaming windowed fingerprinting,
- MP3 unsupported-format fast path.

Use release mode for meaningful numbers. Debug-mode timings are dominated by
compiler settings and XCTest overhead.
