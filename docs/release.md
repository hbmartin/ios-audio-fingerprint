# Releasing the Swift Package

This package ships Swift sources plus a prebuilt Rust FFI binary target. The
repository keeps `Package.swift` pointed at the local `Fingerprint.xcframework`
for development, then the release workflow creates a tag-only release commit
where `FingerprintFFI` points at the uploaded GitHub Release asset URL and
checksum.

## Release Model

Mainline development uses:

```swift
.binaryTarget(
    name: "FingerprintFFI",
    path: "Fingerprint.xcframework"
)
```

Released SwiftPM tags use:

```swift
.binaryTarget(
    name: "FingerprintFFI",
    url: "https://github.com/<owner>/<repo>/releases/download/<version>/Fingerprint.xcframework.zip",
    checksum: "<swift-package-checksum>"
)
```

The release workflow creates that patched `Package.swift` in a temporary commit,
tags it, pushes only the tag, uploads `Fingerprint.xcframework.zip`, then clones
the tag and verifies SwiftPM can consume the uploaded binary.

## Versioning

Use SemVer tags with a leading `v`:

```text
vMAJOR.MINOR.PATCH
vMAJOR.MINOR.PATCH-prerelease
vMAJOR.MINOR.PATCH+build
```

Examples:

```text
v0.1.0
v0.2.0-beta.1
```

The workflow rejects tags that do not match this shape and also rejects versions
that already exist on the remote.

## Prerequisites

The release job runs on `macos-26` and installs Rust stable plus all Apple Rust
standard libraries:

```text
aarch64-apple-darwin
x86_64-apple-darwin
aarch64-apple-ios
aarch64-apple-ios-sim
x86_64-apple-ios
```

The workflow uses the `release` environment and requires `contents: write`
permissions inside the release job so it can push the tag and create the GitHub
Release. Configure any required reviewers or wait timers on that environment in
GitHub repository settings.

Release builds intentionally do not use `actions/cache`; release artifacts must
be built from clean state.

## Pre-Release Checklist

Before triggering a release, merge all intended source changes and make sure the
default branch is in the state you want to ship.

Run the same validation locally when possible:

```bash
cargo test --manifest-path rust/Cargo.toml --workspace --locked
FINGERPRINT_REQUIRE_ALL_SLICES=1 scripts/build-rust-xcframework.sh
scripts/verify-xcframework.sh Fingerprint.xcframework
swift test --filter 'FingerprintTests\.FingerprintTests'
xcodebuild -scheme Fingerprint -destination 'generic/platform=iOS' CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
xcodebuild -scheme Fingerprint -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
```

`FINGERPRINT_REQUIRE_ALL_SLICES=1` is important. Without it, the build script can
skip platforms whose Rust standard libraries are missing, which is useful for
local development but not acceptable for releases.

## Release Workflow

Trigger the workflow manually from GitHub Actions:

1. Open `Fingerprint Release`.
2. Choose `Run workflow`.
3. Enter the version, for example `v0.1.0`.
4. Set `prerelease` when publishing a preview build.

The workflow then performs the full release lifecycle:

1. Checks out the repository with full history.
2. Validates the version string and confirms the remote tag does not exist.
3. Installs Rust stable and all Apple targets.
4. Runs Rust workspace tests.
5. Builds `Fingerprint.xcframework` from the Rust FFI crate.
6. Verifies the xcframework has iOS device, iOS simulator, and macOS slices.
7. Zips the xcframework as `Fingerprint.xcframework.zip`.
8. Runs Swift package tests on macOS.
9. Builds the Swift package for iOS device and iOS simulator.
10. Computes the SwiftPM checksum for the zip.
11. Patches `Package.swift` to use the release asset URL and checksum.
12. Commits that patched manifest and creates the annotated version tag.
13. Pushes only the version tag.
14. Creates the GitHub Release and uploads `Fingerprint.xcframework.zip`.
15. Waits until the uploaded asset is downloadable.
16. Clones the version tag, runs `swift build`, and runs the package tests
    against the uploaded binary.

## Release Artifacts

Each successful release publishes:

- An annotated git tag named like `v0.1.0`.
- A GitHub Release with matching title.
- `Fingerprint.xcframework.zip`.
- A tagged `Package.swift` that references the uploaded zip URL and checksum.

The branch does not need to contain the URL/checksum manifest. Consumers resolve
the package at the release tag, and that tag points at the release commit.

## Consumer Verification

After release, verify from a clean directory:

```bash
tmpdir="$(mktemp -d)"
git clone --depth 1 --branch v0.1.0 https://github.com/<owner>/<repo>.git "$tmpdir/repo"
cd "$tmpdir/repo"
swift package reset
swift build
swift test --filter 'FingerprintTests\.FingerprintTests'
```

Also test from an external app by adding the package URL and selecting the new
tag. SwiftPM should download `Fingerprint.xcframework.zip`, verify its checksum,
and build without using the local `Fingerprint.xcframework` path.

## Failure Handling

If the workflow fails before the tag is pushed, fix the issue and rerun with the
same version.

If the tag was pushed but the GitHub Release or asset upload failed, inspect the
tag before deciding what to do:

```bash
git ls-remote --tags origin v0.1.0
```

For an unpublished or broken tag, delete it only after confirming no consumer can
depend on it yet:

```bash
git push origin :refs/tags/v0.1.0
git tag -d v0.1.0
```

Then rerun the workflow with the same version.

If the GitHub Release exists but the final clone/build validation failed, do not
silently replace the asset after consumers may have fetched it. Prefer deleting
the failed release and tag immediately if it has not been announced, or publish a
new patch version.

## Manual Recovery

The workflow is the source of truth. Manual releases should be avoided, but the
manual sequence is:

```bash
version=v0.1.0
asset=Fingerprint.xcframework.zip

cargo test --manifest-path rust/Cargo.toml --workspace --locked
FINGERPRINT_REQUIRE_ALL_SLICES=1 scripts/build-rust-xcframework.sh
scripts/verify-xcframework.sh Fingerprint.xcframework
ditto -c -k --keepParent Fingerprint.xcframework "$asset"
checksum="$(swift package compute-checksum "$asset")"
url="https://github.com/<owner>/<repo>/releases/download/${version}/${asset}"
```

Patch `Package.swift` to replace the local binary target with the URL/checksum
target, commit it, tag it, push the tag, create the GitHub Release, upload the
asset, then clone the tag and run SwiftPM validation.

## Security Notes

- Keep release action references pinned to full commit SHAs.
- Keep `contents: write` scoped to the release job.
- Keep the `release` environment protection enabled.
- Do not add dependency or build-output caches to the release workflow.
- Recompute the SwiftPM checksum for every zip; never reuse a checksum from a
  previous release.
