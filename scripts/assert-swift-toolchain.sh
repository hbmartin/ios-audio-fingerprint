#!/usr/bin/env bash
# Fail unless the active Swift toolchain is 6.2 or newer (the package's
# swift-tools-version). Shared by the CI jobs that invoke SwiftPM.
set -euo pipefail

swift --version
version="$(swift -version 2>&1 | sed -nE 's/.*Swift version ([0-9]+\.[0-9]+).*/\1/p' | head -n 1)"
major="${version%%.*}"
minor="${version#*.}"
if [ -z "${version}" ] || [ "${major}" -lt 6 ] || { [ "${major}" -eq 6 ] && [ "${minor%%.*}" -lt 2 ]; }; then
  echo "Swift 6.2 or newer is required; found '${version:-unknown}'" >&2
  exit 1
fi
