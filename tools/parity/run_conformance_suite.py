#!/usr/bin/env python3
"""Verify checked-in oracle goldens and compare every case against Rust.

The primary wheel is an explicit command-line input. This tool never installs
or upgrades it, and writes all transient results to a private temporary
directory. It therefore works in CI with a hash-pinned oracle environment and
cannot accidentally turn the developer's default Python into the oracle.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "tools/parity/run_python_cases.py"
COMPARATOR = ROOT / "tools/parity/compare_cases.py"
DEFAULT_MANIFEST = ROOT / "tests/conformance/golden/manifest.json"
DEFAULT_RUST_BINARY = ROOT / "target/debug/rhino3dm-conformance"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command, description):
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if completed.returncode != 0:
        raise RuntimeError(
            f"{description} failed with exit {completed.returncode}:\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-python", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--rust-binary", type=Path, default=DEFAULT_RUST_BINARY)
    args = parser.parse_args()
    manifest_path = args.manifest.resolve()
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "rhino3dm-rs.conformance-golden-manifest.v1":
        parser.error("unsupported conformance golden manifest")
    distribution_version = manifest.get("oracle", {}).get("distribution_version")
    if not isinstance(distribution_version, str):
        parser.error("manifest must specify oracle.distribution_version")
    if not args.oracle_python.is_file():
        parser.error(f"oracle Python does not exist: {args.oracle_python}")
    if not args.rust_binary.is_file():
        parser.error(f"Rust conformance binary does not exist: {args.rust_binary}; build it first")

    verified = []
    with tempfile.TemporaryDirectory(prefix="rhino3dm-conformance-") as temp_dir:
        temp = Path(temp_dir)
        for record in manifest.get("cases", []):
            case = (manifest_path.parent / record["case"]).resolve()
            golden = (manifest_path.parent / record["oracle_result"]).resolve()
            if sha256(case) != record["case_sha256"]:
                raise RuntimeError(f"case SHA-256 differs from manifest: {case}")
            if sha256(golden) != record["oracle_result_sha256"]:
                raise RuntimeError(f"oracle-result SHA-256 differs from manifest: {golden}")
            golden_result = json.loads(golden.read_text(encoding="utf-8"))
            if golden_result.get("case_id") != record["case_id"]:
                raise RuntimeError(f"golden case_id differs from manifest: {golden}")

            oracle_output = run(
                [
                    str(args.oracle_python),
                    str(RUNNER),
                    "--expect-distribution",
                    distribution_version,
                    str(case),
                ],
                f"Python oracle case {record['case_id']}",
            )
            oracle_result = json.loads(oracle_output)
            if oracle_result != golden_result:
                raise RuntimeError(
                    f"pinned Python oracle result differs from checked-in golden for {record['case_id']}"
                )
            rust_path = temp / f"{record['case_id']}.rust.json"
            rust_path.write_text(
                run([str(args.rust_binary), str(case)], f"Rust case {record['case_id']}"),
                encoding="utf-8",
            )
            comparison = json.loads(
                run(
                    [
                        str(args.oracle_python),
                        str(COMPARATOR),
                        "--case",
                        str(case),
                        "--python",
                        str(golden),
                        "--rust",
                        str(rust_path),
                    ],
                    f"Rust/Python comparison {record['case_id']}",
                )
            )
            if not comparison.get("equal"):
                raise RuntimeError(f"comparison reported a mismatch: {record['case_id']}")
            verified.append({
                "case_id": record["case_id"],
                "case_sha256": record["case_sha256"],
                "oracle_result_sha256": record["oracle_result_sha256"],
            })
    print(json.dumps({"status": "ok", "verified": verified}, sort_keys=True))


if __name__ == "__main__":
    main()
