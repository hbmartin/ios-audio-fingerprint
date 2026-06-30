#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${ROOT_DIR}/rust/target/apple"
INCLUDE_DIR="${ROOT_DIR}/rust/crates/fingerprint-ffi/include"
SIM_DIR="${TARGET_DIR}/universal-ios-simulator/release"
SIM_LIB="${SIM_DIR}/libfingerprint_ffi.a"

export CARGO_TARGET_DIR="${TARGET_DIR}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-13.0}"

target_has_std() {
  local target="$1"
  local libdir
  libdir="$(rustc --print target-libdir --target "${target}" 2>/dev/null)" || return 1
  compgen -G "${libdir}/libstd-*.rlib" >/dev/null
}

build_target() {
  local target="$1"
  if ! target_has_std "${target}"; then
    echo "Skipping ${target}: Rust standard library is not installed for this target." >&2
    return 1
  fi

  cargo build \
    --manifest-path "${ROOT_DIR}/rust/Cargo.toml" \
    -p fingerprint-ffi \
    --release \
    --target "${target}"
}

build_target aarch64-apple-darwin

XCFRAMEWORK_ARGS=(
  -library "${TARGET_DIR}/aarch64-apple-darwin/release/libfingerprint_ffi.a"
  -headers "${INCLUDE_DIR}"
)

if build_target aarch64-apple-ios; then
  XCFRAMEWORK_ARGS+=(
    -library "${TARGET_DIR}/aarch64-apple-ios/release/libfingerprint_ffi.a"
    -headers "${INCLUDE_DIR}"
  )
fi

sim_arm64_available=false
sim_x86_64_available=false
if build_target aarch64-apple-ios-sim; then
  sim_arm64_available=true
fi
if build_target x86_64-apple-ios; then
  sim_x86_64_available=true
fi

if [[ "${sim_arm64_available}" == true && "${sim_x86_64_available}" == true ]]; then
  mkdir -p "${SIM_DIR}"
  lipo -create \
    "${TARGET_DIR}/aarch64-apple-ios-sim/release/libfingerprint_ffi.a" \
    "${TARGET_DIR}/x86_64-apple-ios/release/libfingerprint_ffi.a" \
    -output "${SIM_LIB}"
  XCFRAMEWORK_ARGS+=(-library "${SIM_LIB}" -headers "${INCLUDE_DIR}")
elif [[ "${sim_arm64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "${TARGET_DIR}/aarch64-apple-ios-sim/release/libfingerprint_ffi.a"
    -headers "${INCLUDE_DIR}"
  )
elif [[ "${sim_x86_64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "${TARGET_DIR}/x86_64-apple-ios/release/libfingerprint_ffi.a"
    -headers "${INCLUDE_DIR}"
  )
fi

rm -rf "${ROOT_DIR}/Fingerprint.xcframework"
xcodebuild -create-xcframework \
  "${XCFRAMEWORK_ARGS[@]}" \
  -output "${ROOT_DIR}/Fingerprint.xcframework"
