<p align="center">
  <img src="assets/rhino3dm-rs-hero.svg" alt="rhino3dm-rs — native 3DM archives, decoded in pure Rust" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

`rhino3dm-rs` is a source-read-only foundation for inspecting native Rhino
`.3dm` archives without Rhino, OpenNURBS, CPython, WebAssembly, or FFI. It
indexes archive structure, decodes selected typed records, and reports losses
explicitly instead of presenting incomplete geometry as a successful decode.

> **Early-stage API:** archive framing and the typed records below are usable;
> complete curve, mesh, Brep, and extrusion APIs are still in progress.

## Why this exists

Most 3DM tooling is backed by OpenNURBS or Rhino. This project explores a
portable Rust-native boundary for servers, robots, build pipelines, and other
systems where deterministic inspection and explicit unsupported-data reporting
matter more than pretending every file was decoded completely.

## Current support

| Capability | Status |
| --- | --- |
| Native 3DM signature and archive version | Implemented and tested |
| Top-level table and direct-record index | Implemented and tested |
| Object UUID, name, layer index, and user strings | Implemented for supported modern attributes |
| Points | Typed decode implemented |
| Instance references and 4×4 transforms | Typed decode implemented |
| v6+ instance definitions and ordered member UUIDs | Typed decode implemented |
| Curves, meshes, Breps, and extrusions | Classified; native typed decode incomplete |
| Rhino PBR materials and shader parameters | Not decoded; retained source data is not parity |
| Texture files, embedded bytes, and content hashes | Not exposed |
| Mesh UV channels and texture mapping transforms | Not exposed by the public API |
| Geometry census through the Rust `cadmpeg` bridge | Available as an explicit loss report |
| Writing or mutating 3DM files | Not supported |

Unsupported or malformed structures return an error or carry a per-record
diagnostic. Callers that require complete geometry must check the admission
gate rather than relying on object counts alone.

## Install

Install directly from GitHub until the first crates.io release:

```toml
[dependencies]
rhino3dm-rs = { git = "https://github.com/lucas-vitrus/rhino3dm-rs" }
```

## Library example

```rust
use rhino3dm_rs::File3dm;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = File3dm::read("model.3dm")?;

    println!("archive version: {}", model.archive_version());
    println!("objects: {}", model.archive().object_count());
    println!(
        "instance definitions: {}",
        model.archive().instance_definitions.len()
    );

    Ok(())
}
```

## CLI inspection

The included `rhino3dm-index` binary prints a deterministic structural and
typed-decoder summary:

```bash
cargo run --release --bin rhino3dm-index -- model.3dm
```

Example fields include archive version, table counts, object-record framing,
attribute completeness, user strings, geometry classes, points, instance
references, and parse errors.

## Geometry admission

`File3dm::probe_geometry` uses the packaged pure-Rust `cadmpeg` geometry bridge
to produce a census and loss report:

```rust
let probe = rhino3dm_rs::File3dm::probe_geometry("model.3dm")?;
if !probe.complete_object_decode() {
    for warning in &probe.warnings {
        eprintln!("{warning}");
    }
    return Err("incomplete geometry decode".into());
}
```

This is intentionally a gate, not a claim that all geometry has a stable
native `rhino3dm-rs` representation.

The same rule applies to appearance. A renderer must not substitute a gray
material or discard textures silently. See
[`docs/pbr-material-parity.md`](docs/pbr-material-parity.md) for the required
material, texture, UV, and render-reconstruction gate.

## Design principles

- Read-only by default: source files are never rewritten.
- Fail closed: incomplete decode is observable and blocks strict consumers.
- Preserve provenance: source ranges remain attached to archive records.
- Typed where supported: no fabricated placeholder geometry.
- Small public surface: stabilize APIs only after fixture-backed parity tests.

## Development

```bash
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Contributions should add a compact redistributable fixture or construct one in
the test when extending the decoder. Do not commit proprietary production
`.3dm` files.

## Benchmark

The repository includes a redistributable 3DM fixture and a benchmark comparing
the same structural-inspection result contract in Rust and Python. It measures
archive loading plus traversal of objects, attributes, user strings, and
instance definitions. It is not a full geometry-parity benchmark: the Python
package exposes substantially more geometry and document APIs today.

```bash
python3 -m venv .venv
.venv/bin/pip install -r benchmarks/requirements.txt
.venv/bin/python fixtures/generate_benchmark_fixture.py
cargo build --release --bin rhino3dm-bench
.venv/bin/python benchmarks/compare.py --iterations 30 --warmups 5
```

The comparison script writes the raw samples and environment metadata to
`benchmarks/results/`.

Current Apple Silicon baseline (30 measured iterations after 5 warmups):

| Runtime | Median | Mean | p95 |
| --- | ---: | ---: | ---: |
| `rhino3dm-rs` | 2.30 ms | 2.44 ms | 3.12 ms |
| Python `rhino3dm` 8.17.0 | 32.90 ms | 32.91 ms | 34.24 ms |

The Rust median was 14.3× faster for this structural subset. See the
[raw result](benchmarks/results/latest.json). This must not be interpreted as
full API, geometry, or PBR-material parity.

## Roadmap

- Expand object-attribute version coverage.
- Add native curve and polycurve representations.
- Decode render meshes and mesh attributes.
- Add Brep and extrusion topology incrementally.
- Add public, redistributable conformance fixtures.
- Benchmark warm-cache indexing and geometry decoding independently.

## License and naming

Licensed under the [MIT License](LICENSE).

Rhino and Rhinoceros are trademarks of Robert McNeel & Associates. This is an
independent project and is not affiliated with or endorsed by McNeel.
