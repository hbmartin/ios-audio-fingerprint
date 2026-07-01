#!/usr/bin/env bash
set -euo pipefail

XCFRAMEWORK_PATH="${1:-Fingerprint.xcframework}"

if [[ ! -d "${XCFRAMEWORK_PATH}" ]]; then
  echo "Missing xcframework: ${XCFRAMEWORK_PATH}" >&2
  exit 1
fi

/usr/bin/python3 - "${XCFRAMEWORK_PATH}" <<'PY'
import plistlib
import re
import subprocess
import sys
from pathlib import Path

xcframework = Path(sys.argv[1])
info_plist = xcframework / "Info.plist"

if not info_plist.is_file():
    raise SystemExit(f"Missing Info.plist at {info_plist}")

with info_plist.open("rb") as handle:
    info = plistlib.load(handle)

libraries = info.get("AvailableLibraries", [])
if not isinstance(libraries, list):
    raise SystemExit("Info.plist AvailableLibraries is missing or malformed")

required_libraries = [
    ("iOS device", "ios", None, {"arm64"}),
    ("iOS simulator", "ios", "simulator", {"arm64", "x86_64"}),
    ("macOS", "macos", None, {"arm64", "x86_64"}),
]


def lipo_architectures(path: Path) -> set[str]:
    output = subprocess.check_output(["xcrun", "lipo", "-info", str(path)], text=True)
    if "are:" in output:
        return set(output.rsplit("are:", 1)[1].strip().split())
    match = re.search(r"is architecture:\s+(\S+)", output)
    if match:
        return {match.group(1)}
    raise RuntimeError(f"Could not parse lipo output for {path}: {output}")


def find_entry(platform: str, variant, archs: set[str]) -> dict:
    matches = []
    for entry in libraries:
        if entry.get("SupportedPlatform") != platform:
            continue
        if entry.get("SupportedPlatformVariant") != variant:
            continue
        plist_archs = set(entry.get("SupportedArchitectures", []))
        if archs.issubset(plist_archs):
            matches.append(entry)
    if not matches:
        variant_label = f" {variant}" if variant else ""
        raise SystemExit(f"Missing {platform}{variant_label} library with architectures: {sorted(archs)}")
    return matches[0]


for label, platform, variant, required_archs in required_libraries:
    entry = find_entry(platform, variant, required_archs)
    identifier = entry.get("LibraryIdentifier")
    library_path = entry.get("LibraryPath")
    headers_path = entry.get("HeadersPath")

    if not identifier:
        raise SystemExit(f"{label}: missing LibraryIdentifier")
    if library_path != "libfingerprint_ffi.a":
        raise SystemExit(f"{label}: expected LibraryPath libfingerprint_ffi.a, got {library_path!r}")
    if headers_path != "Headers":
        raise SystemExit(f"{label}: expected HeadersPath Headers, got {headers_path!r}")

    root = xcframework / identifier
    library = root / library_path
    header = root / headers_path / "FingerprintFFI.h"
    modulemap = root / headers_path / "module.modulemap"

    for path in (library, header, modulemap):
        if not path.is_file():
            raise SystemExit(f"{label}: missing {path}")

    actual_archs = lipo_architectures(library)
    missing_archs = required_archs - actual_archs
    if missing_archs:
        raise SystemExit(f"{label}: {library} is missing architectures: {sorted(missing_archs)}")

    header_text = header.read_text(encoding="utf-8")
    modulemap_text = modulemap.read_text(encoding="utf-8")
    if "fingerprint_ffi_version" not in header_text:
        raise SystemExit(f"{label}: FingerprintFFI.h does not expose expected FFI symbols")
    if "module FingerprintFFI" not in modulemap_text:
        raise SystemExit(f"{label}: module.modulemap does not declare module FingerprintFFI")

print(f"Verified {xcframework}")
PY
