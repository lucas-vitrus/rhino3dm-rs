# Coding-agent handoff

This is a ready-to-use implementation brief, not a request that starts a new
agent automatically. The current checkout contains an executable P00/P01/P02
foundation, not merely an audit: retained source spans, bounded header reads,
direct UUID CRC checking, recoverable definition parse outcomes, partial
attribute coverage, an overload-aware ledger, and 122 paired-oracle foundation/geometry
operation mappings. No general decoder, editable document API or general writer
has been implemented.

## Mission

Implement observable API and behavior parity between the pure-Rust
`rhino3dm-rs` library and the locked Python `rhino3dm` distribution. Preserve
the existing scene, SVG and exact STEP consumers. Keep Python/OpenNURBS as
development oracles only: no Python subprocess, Python runtime, C++ kernel,
WebAssembly runtime wrapper or FFI geometry dependency in production Rust.
Pure-Rust dependencies may be reused behind tested private adapters.

Work in:

```text
/Users/lucas-vitrus/Documents/GitHub/rhino3dm-rs
```

Audited base: `34a8846b01db1d4aa9dafb11ab42f7a09776d30d`.
Do not implement in the older nested crate under
`vitrus-4/vitrus-convert/crates/rhino3dm-rs`. Treat that as a downstream
migration, not the authoritative package. Inspect live git state before work;
preserve all pre-existing edits and unrelated `.DS_Store` files. Do not reset
or overwrite changes to make the tree match this audit.

## Read before coding

1. [README.md](README.md): measured baseline, gap matrix and audit findings.
2. [oracle-lock.json](oracle-lock.json): artifact identity and external fixture.
3. [ARCHITECTURE.md](ARCHITECTURE.md): module boundaries and data contracts.
4. [CONFORMANCE.md](CONFORMANCE.md): obligations, corpus and acceptance rules.
5. [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md): P00–P12 dependencies.
6. Current `src/lib.rs`, `src/scene.rs`, `src/step.rs`, renderer and tests.

Current all-targets evidence is 22 passing workspace tests. Eight passing
primary-oracle foundation/geometry cases cover 122 of 3,229 ledger obligations. It is not a
1:1 package or complete 3DM writer. Scene RenderMesh is not the Python Mesh
model; the bridge's geometry census is not typed BRep support. Never change
these labels to claim progress.

## Exact oracle

Primary distribution: PyPI `rhino3dm==8.32.1`, initial CPython 3.10 macOS wheel
and SHA-256 in the lock. That wheel reports runtime version `8.32.2`; preserve
both fields and do not relabel or silently upgrade it. Its runtime inventory
has 212 exported classes including enums and binding iterator classes.

Clay regression: `rhino3dm==8.17.0`, 193 exported classes. The existing Clay
Python is `/Users/lucas-vitrus/miniconda3/envs/Clay/bin/python`; use it read-only.
Create isolated test environments for new installations. Pin platform-specific
wheel hashes before running other OS/architecture oracles.

The audit verified both primary wheel and sdist hashes. A matching upstream
release commit/build correspondence is still unresolved; P00 must establish it
or keep that source-provenance limitation explicit. Do not substitute the
moving GitHub branch. Consult Python-conditional binding code, not JavaScript
exports or RhinoCommon's larger API. The operation ledger now splits runtime
pybind overload docstrings and the differential runner has two executable math
cases, but it still lacks binding-source pointers, enum entries, fuzzing and CI.

## Start with these checks

Run these from the standalone repository. These commands exist today:

```bash
git status --short
git rev-parse HEAD
cargo test --workspace --all-targets
cargo run --bin rhino3dm-index -- fixtures/structural-benchmark-v1.3dm
```

Regenerate a class inventory only into a new destination, using the verified
oracle interpreter. The tool refuses an unexpected distribution or overwrite:

```bash
python tools/parity/inventory_python.py --help
python tools/parity/inventory_python.py --expect-distribution 8.32.1 --output /tmp/python-api-8.32.1-new.json
```

Here `python` means the isolated, hash-verified primary oracle, not whichever
interpreter happens to be on PATH. Compare fingerprints and structural content
to the checked-in snapshot; platform/environment paths can legitimately differ.
Do not make a temporary environment path part of the package's runtime setup.

## First implementation slice

Complete the remaining P00 and P01 work before starting broad geometry. The
first bounded P01 source-retention/definition-diagnostic slice and P02 math
seed have already landed.

1. Create the operation ledger. Merge class inventories, module-level exports,
   signatures/docstrings and pinned source. Enumerate overloaded constructors,
   property mutation, enum values and collection protocols. Mark unknown
   behavior unresolved, not compatible.
2. Implement the JSON operation protocol and a small Python oracle runner,
   then a Rust runner for existing points/transforms. Prove the comparator
   rejects an intentionally changed value. Store fixture/case provenance.
3. Add retained immutable source storage and checked spans to the archive
   model without breaking existing callers. Keep one result/diagnostic for
   every failed instance definition instead of silently dropping it.
4. Add small malformed-definition and corrupted/truncated-input regression
   cases. Update coverage to distinguish decoded, skipped, opaque and corrupt
   data. Do not mark a skipped attribute as fully preserved.
5. Run all existing tests plus new regressions and publish exact results.
   Document the deferred CRC/budget work as the next P01 slice.

Acceptance for this slice: public fixture behavior preserved; source bytes
retrievable for opaque spans; a failed definition remains visible; bounded
malformed inputs fail safely; one shared operation runs in both languages;
ledger links implemented mappings to actual passing cases. An empty type or
unsupported stub does not satisfy an operation.

Next vertical slice: P02/P03, create a point with a named layer and UserStrings
in Rust, write a new `.3dm`, read it in Python, edit and round-trip it. Then add
meshes, curves and instances using the same architecture. Avoid beginning a
large BRep rewrite before this complete testing loop exists.

## How to maintain progress

Create a machine-readable task ledger in P00 with stable phase/task IDs, exact
dependencies, owned modules, acceptance case IDs, source references, status,
result artifact paths and remaining blockers. Keep API-operation entries
separate from work-package entries. Update both with each completed change.

Use small reviewable changes aligned to the roadmap. Include the reason,
observable behavior added, tests run and known limitations. Do not modify a
golden to accept unexplained Rust behavior. Keep one coherent document/ID/
diagnostic/writer contract if work is delegated in a future authorized session.

A progress report must distinguish implemented, tested against the primary
oracle, tested only against 8.17, bridge-backed, opaque-only, and not tested.
Never infer a percentage from class names or record counts. A downstream
robotics use case can prioritize work, but cannot remove target Python APIs
from the 1:1 definition.

## Protect source files and consumers

The original R06 model is an external immutable fixture. Its absolute path,
SHA-256 and Python census are in the lock. Read it only; create test outputs in
new isolated directories. Verify its hash before and after qualification runs.
Do not copy private CAD into public fixtures without permission.

Keep existing strict STEP admission, texture-loss reporting, instance omission
reporting and material requirements intact. Fix fidelity at the source; do not
turn off a consumer's gate. Keep source units/UUIDs/original geometry distinct
from display projections. No mesh replacement for missing exact geometry.

General writing must preserve an existing destination on refusal or failure.
Opaque data cannot be replayed after arbitrary table/version edits without
reference-safety proof. Unknown plugin data must remain explicit. Performance
work follows equivalent-output proof; the old 42× partial-index comparison is
not evidence of a full conversion speedup.

## Definition of done

Use P12 and CONFORMANCE.md. Close every required operation and unresolved
target-discovery item, qualify supported read/write versions and platforms,
retain licenses and dependency provenance, and publish exact tested scope.
Until then release honestly named subsets. Updating the Vitrus/Clay converter
and proving robotics-output equivalence is a separate integration milestone.
