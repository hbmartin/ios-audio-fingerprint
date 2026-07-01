#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${ROOT_DIR}/rust/target/apple"
INCLUDE_DIR="${ROOT_DIR}/rust/crates/fingerprint-ffi/include"
OUTPUT_XCFRAMEWORK="${OUTPUT_XCFRAMEWORK:-${ROOT_DIR}/Fingerprint.xcframework}"
REQUIRE_ALL_SLICES="${FINGERPRINT_REQUIRE_ALL_SLICES:-${CI:-0}}"
PROFILE_DIR="release"

IOS_DEVICE_TARGET="aarch64-apple-ios"
IOS_SIM_ARM64_TARGET="aarch64-apple-ios-sim"
IOS_SIM_X86_64_TARGET="x86_64-apple-ios"
MACOS_ARM64_TARGET="aarch64-apple-darwin"
MACOS_X86_64_TARGET="x86_64-apple-darwin"

SIM_DIR="${TARGET_DIR}/universal-ios-simulator/${PROFILE_DIR}"
SIM_LIB="${SIM_DIR}/libfingerprint_ffi.a"
MACOS_DIR="${TARGET_DIR}/universal-macos/${PROFILE_DIR}"
MACOS_LIB="${MACOS_DIR}/libfingerprint_ffi.a"

export CARGO_TARGET_DIR="${TARGET_DIR}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-13.0}"

require_tool() {
  local tool="$1"
  if ! command -v "${tool}" >/dev/null 2>&1; then
    echo "Missing required tool: ${tool}" >&2
    exit 1
  fi
}

target_path() {
  local target="$1"
  echo "${TARGET_DIR}/${target}/${PROFILE_DIR}/libfingerprint_ffi.a"
}

target_has_std() {
  local target="$1"
  local libdir
  libdir="$(rustc --print target-libdir --target "${target}" 2>/dev/null)" || return 1
  compgen -G "${libdir}/libstd-*.rlib" >/dev/null
}

build_target() {
  local target="$1"
  if ! target_has_std "${target}"; then
    if [[ "${REQUIRE_ALL_SLICES}" == "1" || "${REQUIRE_ALL_SLICES}" == "true" ]]; then
      echo "Missing Rust standard library for required target: ${target}" >&2
      echo "Install it with: rustup target add ${target}" >&2
      exit 1
    fi
    echo "Skipping ${target}: Rust standard library is not installed for this target." >&2
    return 1
  fi

  echo "Building fingerprint-ffi for ${target}"
  if ! cargo build \
    --manifest-path "${ROOT_DIR}/rust/Cargo.toml" \
    -p fingerprint-ffi \
    --release \
    --target "${target}"; then
    echo "ERROR: cargo build failed for ${target}" >&2
    exit 1
  fi
}

require_tool cargo
require_tool rustc
require_tool xcodebuild
require_tool lipo

XCFRAMEWORK_ARGS=()

if build_target "${IOS_DEVICE_TARGET}"; then
  XCFRAMEWORK_ARGS+=(
    -library "$(target_path "${IOS_DEVICE_TARGET}")"
    -headers "${INCLUDE_DIR}"
  )
fi

sim_arm64_available=false
sim_x86_64_available=false
if build_target "${IOS_SIM_ARM64_TARGET}"; then
  sim_arm64_available=true
fi
if build_target "${IOS_SIM_X86_64_TARGET}"; then
  sim_x86_64_available=true
fi

if [[ "${sim_arm64_available}" == true && "${sim_x86_64_available}" == true ]]; then
  mkdir -p "${SIM_DIR}"
  lipo -create \
    "$(target_path "${IOS_SIM_ARM64_TARGET}")" \
    "$(target_path "${IOS_SIM_X86_64_TARGET}")" \
    -output "${SIM_LIB}"
  XCFRAMEWORK_ARGS+=(-library "${SIM_LIB}" -headers "${INCLUDE_DIR}")
elif [[ "${sim_arm64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "$(target_path "${IOS_SIM_ARM64_TARGET}")"
    -headers "${INCLUDE_DIR}"
  )
elif [[ "${sim_x86_64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "$(target_path "${IOS_SIM_X86_64_TARGET}")"
    -headers "${INCLUDE_DIR}"
  )
fi

macos_arm64_available=false
macos_x86_64_available=false
if build_target "${MACOS_ARM64_TARGET}"; then
  macos_arm64_available=true
fi
if build_target "${MACOS_X86_64_TARGET}"; then
  macos_x86_64_available=true
fi

if [[ "${macos_arm64_available}" == true && "${macos_x86_64_available}" == true ]]; then
  mkdir -p "${MACOS_DIR}"
  lipo -create \
    "$(target_path "${MACOS_ARM64_TARGET}")" \
    "$(target_path "${MACOS_X86_64_TARGET}")" \
    -output "${MACOS_LIB}"
  XCFRAMEWORK_ARGS+=(-library "${MACOS_LIB}" -headers "${INCLUDE_DIR}")
elif [[ "${macos_arm64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "$(target_path "${MACOS_ARM64_TARGET}")"
    -headers "${INCLUDE_DIR}"
  )
elif [[ "${macos_x86_64_available}" == true ]]; then
  XCFRAMEWORK_ARGS+=(
    -library "$(target_path "${MACOS_X86_64_TARGET}")"
    -headers "${INCLUDE_DIR}"
  )
fi

if [[ "${#XCFRAMEWORK_ARGS[@]}" -eq 0 ]]; then
  echo "No Rust Apple targets were built; cannot create Fingerprint.xcframework" >&2
  exit 1
fi

rm -rf "${OUTPUT_XCFRAMEWORK}"
xcodebuild -create-xcframework \
  "${XCFRAMEWORK_ARGS[@]}" \
  -output "${OUTPUT_XCFRAMEWORK}"
