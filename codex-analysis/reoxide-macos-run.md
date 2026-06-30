# ReOxide macOS Run

## Tool Paths

The successful run used the local ReOxide virtual environment at:

```text
/Users/haroldmartin/Downloads/reoxide/venv
```

Key binaries:

```text
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxided
/Users/haroldmartin/Downloads/reoxide/venv/lib/python3.12/site-packages/reoxide/data/bin/decompile
```

The native ReOxide binaries are arm64 Mach-O files.

## Configuration

Created:

```text
/Users/haroldmartin/Library/Application Support/reoxide/reoxide.toml
```

Config content:

```toml
data-directory = "/Users/haroldmartin/Library/Application Support/reoxide"

[[ghidra-install]]
enabled = true
root-dir = "/opt/homebrew/Cellar/ghidra/12.1.2/libexec"
```

The daemon copied packaged plugins into:

```text
/Users/haroldmartin/Library/Application Support/reoxide/plugins/
```

Loaded plugins:

```text
libcore.dylib
libprintmir.dylib
libprintrust.dylib
```

Reported output languages:

```text
core: c-language
core: java-language
printmir: mir-language
printrust: rust-language
```

## Ghidra Link

Before linking, the stock Ghidra decompiler was:

```text
/opt/homebrew/Cellar/ghidra/12.1.2/libexec/Ghidra/Features/Decompiler/os/mac_arm_64/decompile
```

Stock SHA-256:

```text
7fd57be3310c88777efabf4f71a9af7151453a8d41407aa9a935f6753646b9d0
```

ReOxide decompiler SHA-256:

```text
f0f0bd574c8dd1c1c391cb12c95d2095e66eb5706680b9940401eef8a0742624
```

`reoxide link-ghidra` renamed the stock binary to `decompile.orig` and created
a symlink from Ghidra's `decompile` to ReOxide's packaged `decompile`.

After the headless export completed, the ReOxide output language was reset to
default, Ghidra was unlinked from ReOxide, and the stock decompiler was restored.
The restored Ghidra decompiler SHA-256 again matched:

```text
7fd57be3310c88777efabf4f71a9af7151453a8d41407aa9a935f6753646b9d0
```

## Commands Run

```sh
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxided
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide list-languages
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide list-actions
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide list-rules
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide link-ghidra
/Users/haroldmartin/Downloads/reoxide/venv/bin/reoxide force-output-language rust-language
/opt/homebrew/Cellar/ghidra/12.1.2/libexec/support/analyzeHeadless \
  /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/codex-analysis/ghidra-projects \
  FingerprintReOxide \
  -import /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/Fingerprint.xcframework/ios-arm64/FingerprintFFI.framework/FingerprintFFI \
  -scriptPath /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/codex-analysis/scripts \
  -postScript ExportSelectedCore.java /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/codex-analysis/exports/reoxide \
  -deleteProject \
  -log /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/codex-analysis/logs/reoxide-ghidra-core-export.log \
  -scriptlog /Users/haroldmartin/Downloads/pocket-casts-ios-fingerprint-trunk/codex-analysis/logs/reoxide-ghidra-core-export-script.log
```

## Outputs

Focused exports:

```text
codex-analysis/exports/reoxide/core-index.tsv
codex-analysis/exports/reoxide/core-decompile.c
codex-analysis/exports/reoxide/core-disassembly.tsv
```

Logs:

```text
codex-analysis/logs/reoxide-ghidra-core-export.log
codex-analysis/logs/reoxide-ghidra-core-export-script.log
```

The focused export script found and decompiled 27 core functions.

## Findings

ReOxide produced Rust-shaped pseudocode rather than ordinary C-style Ghidra
pseudocode. It did not recover original Rust source, but it improved several
signals over the earlier Oxidizer-only pass:

- `fingerprint_core::matching::compare_with_drift` is decompiled with return
  type `f32` and explicit empty-input returns of `0.0`.
- `Fingerprint::from_bytes` clearly validates `len >= 8`, reads the second
  little-endian `u32` as the hash count, requires `count * 4 + 8 <= len`, then
  copies the declared hashes.
- `Fingerprint::to_bytes` clearly writes duration first, then hash count, then
  hashes as `u32` values.
- `encode_fingerprint` confirms 8 chroma frames per hash and the final-window
  inclusion behavior.
- `compute_hash` confirms a `0.05` positive-delta threshold and the final
  coarse-energy operation:

```text
hash ^ ((sum(first_chroma_frame) * 4.0 as i32) << 28)
```

- `decode_bytes` calls `decode_mp3_bytes`, reinforcing that the original Rust
  implementation handled MP3 via a real decode path rather than reporting it as
  unsupported.

## Limitations

- The output is still decompiler pseudocode, not compilable Rust.
- The `compute_hash` delta-bit packing is vectorized and still needs fixture
  tests or additional constant decoding before exact bit mapping is guaranteed.
- Ghidra still reports `Unknown calling convention: __rustcall` on many
  functions.
- The run depends on a live `reoxided` daemon while Ghidra's decompiler is
  linked to ReOxide.
