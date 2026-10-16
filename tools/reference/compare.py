#!/usr/bin/env python3
"""Compare native WMIC captures without normalizing streams or statuses."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import sys


def read_stream(case: dict, stream: str, root: Path | None = None) -> bytes:
    inline = stream + "_base64"
    if inline in case:
        return base64.b64decode(case[inline], validate=True)
    if root is None:
        raise ValueError("stored stream requires its capture directory")
    entry = case[stream]
    path = (root / entry["file"]).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError("stream path escapes the capture directory")
    data = path.read_bytes()
    if len(data) != entry["bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
        raise ValueError("stream integrity failure: " + str(path))
    return data


def compare_cases(expected: dict, actual: dict, expected_root: Path | None = None,
                  actual_root: Path | None = None) -> list[str]:
    differences = []
    for key in ("id", "arguments_utf16_base64", "state", "exit_status"):
        if expected.get(key) != actual.get(key):
            differences.append(f"{expected.get('id')}: {key}: {expected.get(key)!r} != {actual.get(key)!r}")
    for stream in ("stdout", "stderr"):
        left = read_stream(expected, stream, expected_root)
        right = read_stream(actual, stream, actual_root)
        for rule in expected.get("normalizations", []):
            if rule.get("kind") != "wmic-command-starttime" or rule.get("stream") not in ("stdout", "stderr"):
                raise ValueError("unknown reference normalization policy")
            if rule["stream"] == stream:
                pattern = rb'\A(<COMMAND\b[^>\r\n]*\bSTARTTIME=")[^"]*(")'
                replace = lambda match: match.group(1) + b"{STARTTIME}" + match.group(2)
                left = re.sub(pattern, replace, left, count=1)
                right = re.sub(pattern, replace, right, count=1)
        if left != right:
            first = next((i for i, (a, b) in enumerate(zip(left, right)) if a != b), min(len(left), len(right)))
            differences.append(f"{expected['id']}: {stream}: {len(left)} != {len(right)} bytes; first difference at {first}")
    return differences


def compare_captures(expected: dict, actual: dict, expected_root: Path, actual_root: Path) -> list[str]:
    def indexed(payload):
        cases = payload["cases"]
        result = {case["id"]: case for case in cases}
        if len(result) != len(cases):
            raise ValueError("duplicate capture case IDs")
        return result
    left, right = indexed(expected), indexed(actual)
    differences = []
    for name in sorted(left.keys() | right.keys()):
        if name not in left or name not in right:
            differences.append(f"{name}: {'unexpected' if name not in left else 'missing'} case")
        else:
            differences.extend(compare_cases(left[name], right[name], expected_root, actual_root))
    return differences


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", type=Path)
    parser.add_argument("actual", type=Path)
    args = parser.parse_args()
    try:
        differences = compare_captures(json.loads(args.expected.read_text()),
                                       json.loads(args.actual.read_text()),
                                       args.expected.parent, args.actual.parent)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print("invalid capture: " + str(exc), file=sys.stderr)
        return 2
    for difference in differences:
        print(difference)
    if not differences:
        print("All selected case bytes, states, arguments and exit statuses match.")
    return int(bool(differences))


if __name__ == "__main__":
    raise SystemExit(main())
