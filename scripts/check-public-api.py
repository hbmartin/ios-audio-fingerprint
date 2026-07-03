#!/usr/bin/env python3
"""Guard the Swift package's public API surface.

Extracts every public/open declaration (plus protocol requirements and public
enum cases) from Sources/Fingerprint and compares the result against the
checked-in baseline at docs/public-api.txt.

    python3 scripts/check-public-api.py            # verify (CI)
    python3 scripts/check-public-api.py --update   # regenerate the baseline

Intentional API changes are made by regenerating the baseline in the same PR,
so the API diff is part of the reviewed change set.

The extractor is a lightweight scanner, not a compiler: it assumes the code
style used in this package (attributes inline with declarations, no brace or
comment characters inside string literals in Sources/Fingerprint).
"""

import argparse
import difflib
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SOURCE_DIR = REPO_ROOT / "Sources" / "Fingerprint"
BASELINE = REPO_ROOT / "docs" / "public-api.txt"

DECL_START = re.compile(r"^(?:@\w+(?:\([^)]*\))?\s+)*(?:public|open)\b")
TYPE_DECL = re.compile(
    r"^(?:@\w+(?:\([^)]*\))?\s+)*(?:public|open)\s+(?:final\s+)?"
    r"(class|struct|enum|protocol|extension|actor)\s+(\w+)"
)
PLAIN_EXTENSION = re.compile(r"^extension\s+(\w+)")
PROTOCOL_MEMBER = re.compile(r"^(?:func|var|let|init|subscript|associatedtype)\b")
ENUM_CASE = re.compile(r"^(?:indirect\s+)?case\s+\w")


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return "\n".join(re.sub(r"//.*", "", line) for line in text.splitlines())


def extract_api(text: str) -> list[str]:
    entries: list[str] = []
    # Stack of (name, kind, is_public, depth_at_open) for type-like scopes.
    scopes: list[tuple[str, str, bool, int]] = []
    depth = 0
    pending: list[str] | None = None

    def innermost():
        return scopes[-1] if scopes else None

    def context() -> str:
        return ".".join(s[0] for s in scopes)

    def flush(decl_lines: list[str]) -> None:
        joined = " ".join(part.strip() for part in decl_lines)
        joined = joined.split("{")[0].strip().rstrip(",")
        joined = re.sub(r"\s+", " ", joined)
        if not joined:
            return
        prefix = context()
        entries.append(f"{prefix} :: {joined}" if prefix else joined)

    for raw in strip_comments(text).splitlines():
        line = raw.strip()
        if pending is not None:
            pending.append(line)
            blob = " ".join(pending)
            balanced = blob.count("(") == blob.count(")") and blob.count("[") == blob.count("]")
            if balanced and ("{" in blob or not line.endswith(("(", ","))):
                type_match = TYPE_DECL.match(" ".join(p.strip() for p in pending))
                flush(pending)
                if type_match and "{" in blob:
                    scopes.append((type_match.group(2), type_match.group(1), True, depth))
                pending = None
        else:
            scope = innermost()
            starts_decl = False
            if DECL_START.match(line):
                starts_decl = True
            elif scope and scope[2]:
                if scope[1] == "protocol" and PROTOCOL_MEMBER.match(line):
                    starts_decl = True
                elif scope[1] == "enum" and ENUM_CASE.match(line) and depth == scope[3] + 1:
                    starts_decl = True
            ext_match = PLAIN_EXTENSION.match(line)

            if starts_decl:
                blob = line
                balanced = blob.count("(") == blob.count(")") and blob.count("[") == blob.count("]")
                if balanced and ("{" in blob or not line.endswith(("(", ","))):
                    type_match = TYPE_DECL.match(line)
                    flush([line])
                    if type_match and "{" in blob:
                        scopes.append((type_match.group(2), type_match.group(1), True, depth))
                else:
                    pending = [line]
            elif ext_match and "{" in line:
                entries.append(re.sub(r"\s+", " ", line.split("{")[0].strip()))
                scopes.append((ext_match.group(1), "extension", True, depth))

        depth += raw.count("{") - raw.count("}")
        while scopes and depth <= scopes[-1][3]:
            scopes.pop()

    return entries


def current_api() -> str:
    parts: list[str] = []
    for path in sorted(SOURCE_DIR.glob("*.swift")):
        parts.append(f"== {path.relative_to(REPO_ROOT)} ==")
        parts.extend(extract_api(path.read_text()))
    return "\n".join(parts) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true", help="rewrite the baseline")
    args = parser.parse_args()

    api = current_api()
    if args.update:
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        BASELINE.write_text(api)
        print(f"wrote {BASELINE.relative_to(REPO_ROOT)}")
        return 0

    if not BASELINE.exists():
        print(f"missing baseline {BASELINE.relative_to(REPO_ROOT)}; "
              "run: python3 scripts/check-public-api.py --update", file=sys.stderr)
        return 1

    baseline = BASELINE.read_text()
    if baseline == api:
        print("public API surface matches the baseline")
        return 0

    print("public API surface differs from docs/public-api.txt:\n", file=sys.stderr)
    sys.stderr.writelines(
        difflib.unified_diff(
            baseline.splitlines(keepends=True),
            api.splitlines(keepends=True),
            fromfile="docs/public-api.txt",
            tofile="current sources",
        )
    )
    print(
        "\nIf this change is intentional, regenerate the baseline in this PR:\n"
        "    python3 scripts/check-public-api.py --update",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
