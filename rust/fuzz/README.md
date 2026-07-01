# Fuzzing

Coverage-guided fuzz targets for the untrusted-input parsers, built with
[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) / libFuzzer.

This crate is intentionally **excluded** from the main `rust/` workspace: it
needs a nightly toolchain and libFuzzer, so it never runs in the normal
`cargo build`/`test`/`clippy` sweep. A fast, always-on approximation runs in CI
as the `decoder_never_panics_on_malformed_input` test in `fingerprint-core`.

## Targets

| Target | Fuzzes |
| --- | --- |
| `decode_audio` | `decode_audio_bytes` — WAV + MP3 detection and decoding |
| `fingerprint_from_bytes` | the length-prefixed fingerprint deserializer |

## Running

```bash
cargo install cargo-fuzz          # one time
cd rust/fuzz
cargo +nightly fuzz run decode_audio
cargo +nightly fuzz run fingerprint_from_bytes
```

Reproduce a crash artifact:

```bash
cargo +nightly fuzz run decode_audio artifacts/decode_audio/crash-<hash>
```

`target/`, `corpus/`, `artifacts/`, and `coverage/` are git-ignored.
