#!/usr/bin/env python3
"""Compare equivalent structural inspection in rhino3dm-rs and rhino3dm.py."""

import argparse
import hashlib
import json
import platform
from pathlib import Path
import statistics
import subprocess
import sys
import time

import rhino3dm


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_FIXTURE = ROOT / "fixtures" / "structural-benchmark-v1.3dm"
RUST_BINARY = ROOT / "target" / "release" / "rhino3dm-bench"


def python_inspect(path: Path):
    model = rhino3dm.File3dm.Read(str(path))
    if model is None:
        raise RuntimeError(f"rhino3dm failed to read {path}")
    attributed = 0
    named = 0
    user_strings = 0
    for item in model.Objects:
        attributes = item.Attributes
        if attributes is not None:
            attributed += 1
            named += int(bool(attributes.Name))
            user_strings += attributes.UserStringCount
    definitions = list(model.InstanceDefinitions)
    return {
        "archive_version": model.ArchiveVersion,
        "objects": len(model.Objects),
        "attributed_objects": attributed,
        "named_objects": named,
        "user_strings": user_strings,
        "instance_definitions": len(definitions),
        "instance_definition_members": sum(len(item.GetObjectIds()) for item in definitions),
    }


def summary(samples):
    ordered = sorted(samples)
    return {
        "min": ordered[0],
        "median": statistics.median(ordered),
        "mean": statistics.fmean(ordered),
        "p95": ordered[round((len(ordered) - 1) * 0.95)],
        "max": ordered[-1],
    }


def benchmark_python(path: Path, iterations: int, warmups: int):
    for _ in range(warmups):
        python_inspect(path)
    samples = []
    expected = None
    for _ in range(iterations):
        start = time.perf_counter_ns()
        census = python_inspect(path)
        samples.append((time.perf_counter_ns() - start) / 1_000_000)
        if expected is not None and census != expected:
            raise RuntimeError("Python fixture census changed between iterations")
        expected = census
    return {
        "runtime": "rhino3dm.py",
        "version": rhino3dm.__version__,
        "iterations": iterations,
        "warmups": warmups,
        "samples_ms": samples,
        "summary_ms": summary(samples),
        "census": expected,
    }


def benchmark_rust(path: Path, iterations: int, warmups: int):
    completed = subprocess.run(
        [str(RUST_BINARY), str(path), str(iterations), str(warmups)],
        check=True,
        text=True,
        capture_output=True,
    )
    return json.loads(completed.stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    parser.add_argument("--iterations", type=int, default=30)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.iterations < 1 or args.warmups < 0:
        parser.error("iterations must be positive and warmups non-negative")
    if not args.fixture.is_file():
        parser.error(f"fixture does not exist: {args.fixture}")
    if not RUST_BINARY.is_file():
        parser.error("build the Rust benchmark first: cargo build --release --bin rhino3dm-bench")

    fixture_bytes = args.fixture.read_bytes()
    rust = benchmark_rust(args.fixture, args.iterations, args.warmups)
    python = benchmark_python(args.fixture, args.iterations, args.warmups)
    comparable = rust["census"] == python["census"]
    result = {
        "schema": "rhino3dm-rs.structural-benchmark.v1",
        "scope": "file read plus structural object/attribute/user-string/instance traversal",
        "geometry_api_parity": False,
        "census_equal": comparable,
        "fixture": {
            "path": str(args.fixture.relative_to(ROOT)),
            "bytes": len(fixture_bytes),
            "sha256": hashlib.sha256(fixture_bytes).hexdigest(),
        },
        "environment": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": sys.version.split()[0],
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        },
        "rust": rust,
        "python": python,
    }
    if not comparable:
        print(json.dumps(result, indent=2))
        raise SystemExit("census mismatch: benchmark workloads are not equivalent")

    output = args.output or ROOT / "benchmarks" / "results" / "latest.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({
        "output": str(output),
        "census_equal": True,
        "rust_median_ms": rust["summary_ms"]["median"],
        "python_median_ms": python["summary_ms"]["median"],
        "python_over_rust_median": python["summary_ms"]["median"] / rust["summary_ms"]["median"],
    }, indent=2))


if __name__ == "__main__":
    main()
