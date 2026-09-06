<p align="center">
  <img src="assets/rhino3dm-rs-hero.svg" alt="rhino3dm-rs — native 3DM archives, decoded in pure Rust" width="100%">
</p>

<p align="center">
  <a href="https://github.com/lucas-vitrus/rhino3dm-rs/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/lucas-vitrus/rhino3dm-rs/actions/workflows/ci.yml/badge.svg"></a>
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

CI runs the same checks on Linux, macOS, and Windows. Contributions should add
a compact redistributable fixture or construct one in the test when extending
the decoder. Do not commit proprietary production `.3dm` files.

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
