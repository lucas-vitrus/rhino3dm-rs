#!/usr/bin/env python3
"""Compare one Rust/Python conformance result using the case's named policy.

This deliberately compares structure, ordering, strings and booleans exactly.
Only finite computed numeric leaves use the narrow case-specific absolute and
relative tolerances declared by the checked-in case.
"""

import argparse
import json
import math
from pathlib import Path


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def numeric_policy(case):
    numeric = case.get("comparison", {}).get("numeric", {})
    atol = numeric.get("atol", 0.0)
    rtol = numeric.get("rtol", 0.0)
    if (
        isinstance(atol, bool)
        or isinstance(rtol, bool)
        or not isinstance(atol, (int, float))
        or not isinstance(rtol, (int, float))
        or not math.isfinite(atol)
        or not math.isfinite(rtol)
        or atol < 0
        or rtol < 0
    ):
        raise ValueError("comparison.numeric atol and rtol must be finite non-negative numbers")
    return float(atol), float(rtol)


def compare(expected, actual, *, path, atol, rtol, mismatches):
    if isinstance(expected, bool) or isinstance(actual, bool):
        if expected is not actual:
            mismatches.append(f"{path}: expected {expected!r}, got {actual!r}")
        return
    if isinstance(expected, (int, float)) and isinstance(actual, (int, float)):
        if not math.isfinite(expected) or not math.isfinite(actual):
            if expected != actual:
                mismatches.append(f"{path}: non-finite values differ: {expected!r} != {actual!r}")
        elif not math.isclose(expected, actual, abs_tol=atol, rel_tol=rtol):
            mismatches.append(
                f"{path}: expected {expected!r}, got {actual!r} "
                f"(atol={atol:g}, rtol={rtol:g})"
            )
        return
    if type(expected) is not type(actual):
        mismatches.append(
            f"{path}: type differs: {type(expected).__name__} != {type(actual).__name__}"
        )
        return
    if isinstance(expected, dict):
        if list(expected) != list(actual):
            mismatches.append(f"{path}: keys/order differ: {list(expected)!r} != {list(actual)!r}")
            return
        for key in expected:
            compare(expected[key], actual[key], path=f"{path}.{key}", atol=atol, rtol=rtol, mismatches=mismatches)
        return
    if isinstance(expected, list):
        if len(expected) != len(actual):
            mismatches.append(f"{path}: list lengths differ: {len(expected)} != {len(actual)}")
            return
        for index, (left, right) in enumerate(zip(expected, actual)):
            compare(left, right, path=f"{path}[{index}]", atol=atol, rtol=rtol, mismatches=mismatches)
        return
    if expected != actual:
        mismatches.append(f"{path}: expected {expected!r}, got {actual!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", type=Path, required=True)
    parser.add_argument("--python", dest="python_result", type=Path, required=True)
    parser.add_argument("--rust", dest="rust_result", type=Path, required=True)
    args = parser.parse_args()
    case = load_json(args.case)
    python_result = load_json(args.python_result)
    rust_result = load_json(args.rust_result)
    if case.get("case_id") != python_result.get("case_id") or case.get("case_id") != rust_result.get("case_id"):
        parser.error("case and result case_id values must agree")
    atol, rtol = numeric_policy(case)
    mismatches = []
    compare(python_result, rust_result, path="$", atol=atol, rtol=rtol, mismatches=mismatches)
    result = {
        "schema": "rhino3dm-rs.conformance-comparison.v1",
        "case_id": case["case_id"],
        "equal": not mismatches,
        "numeric": {"atol": atol, "rtol": rtol},
        "mismatches": mismatches,
    }
    print(json.dumps(result, allow_nan=False, sort_keys=True))
    if mismatches:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
