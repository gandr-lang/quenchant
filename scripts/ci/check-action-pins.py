#!/usr/bin/env python3
"""Require every external GitHub Action to be allowed and SHA-pinned."""

from __future__ import annotations

import pathlib
import re
import sys

ALLOWED = {
    "Swatinem/rust-cache",
    "actions/cache/restore",
    "actions/cache/save",
    "actions/checkout",
    "actions/upload-artifact",
    "jdx/mise-action",
    "taiki-e/install-action",
}
USES = re.compile(r"^\s*uses:\s*([^#\s]+)")
SHA = re.compile(r"[0-9a-f]{40}")


def main() -> int:
    """Check workflow and composite-action references against the closed policy."""
    failures: list[str] = []
    files = sorted(
        path
        for path in pathlib.Path(".github").rglob("*")
        if path.suffix in {".yml", ".yaml"}
    )
    for path in files:
        for line_number, line in enumerate(path.read_text().splitlines(), start=1):
            match = USES.match(line)
            if match is None:
                continue
            reference = match.group(1)
            if reference.startswith("./"):
                continue
            action, separator, revision = reference.partition("@")
            if not separator or action not in ALLOWED or SHA.fullmatch(revision) is None:
                failures.append(f"{path}:{line_number}: {reference}")
    if failures:
        print("action pin check FAILED:", file=sys.stderr)
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("action pin check OK: every external action is allowed and SHA-pinned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
