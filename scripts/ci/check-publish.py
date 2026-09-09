#!/usr/bin/env python3
"""Require the release allowlist to match the declared package boundary."""

from __future__ import annotations

import json
import subprocess
import sys

PUBLISHABLE = {
    "quenchant",
    "quenchant-anodized",
    "quenchant-arith",
    "quenchant-shape",
    "quenchant-dylints",
    "quenchant-gates",
    "quenchant-spec-macros",
}
UNPUBLISHED = {"quenchant-fixture-macros"}


def main() -> int:
    """Reject missing packages, unknown packages, or incorrect registry eligibility."""
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = json.loads(result.stdout)["packages"]
    actual = {package["name"]: package["publish"] for package in packages}
    expected = {name: ["crates-io"] for name in PUBLISHABLE}
    expected.update({name: [] for name in UNPUBLISHED})
    if actual != expected:
        for name in sorted(actual.keys() | expected.keys()):
            if name not in expected:
                print(f"publish guard FAILED: unclassified package {name}", file=sys.stderr)
            elif name not in actual:
                print(f"publish guard FAILED: missing package {name}", file=sys.stderr)
            elif actual[name] != expected[name]:
                print(
                    f"publish guard FAILED: {name}: publish={actual[name]!r}, expected {expected[name]!r}",
                    file=sys.stderr,
                )
        return 1
    print(f"publish guard OK: {len(PUBLISHABLE)} crates.io packages; fixture macros remain unpublished")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
