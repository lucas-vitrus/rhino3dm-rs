# Python parity: audited baseline and scope

Audit: 2026-09-07. Implementation baseline: commit
`34a8846b01db1d4aa9dafb11ab42f7a09776d30d` in the standalone
[`lucas-vitrus/rhino3dm-rs`](https://github.com/lucas-vitrus/rhino3dm-rs)
repository. This is an implementation specification, not a claim of completed
parity. No production decoder changes were made for this audit.

Start with [AGENT_HANDOFF.md](AGENT_HANDOFF.md), then
[ARCHITECTURE.md](ARCHITECTURE.md), [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md),
and [CONFORMANCE.md](CONFORMANCE.md). [index.html](index.html) is the offline
reading copy. The [oracle lock](oracle-lock.json) records exact references.
The [audit verification receipt](VERIFICATION.md) distinguishes checks executed
now from the future conformance suite.

## What 1:1 means

Target observable behavior of the **Python `rhino3dm` distribution**, including
geometry creation/query/editing, document tables, serialization, enums,
collection operations, overloads, defaults, and failure results. Rust spelling
and memory ownership can be idiomatic, but every target operation must have an
explicit Rust mapping and equivalent observable results. Mapping a Python
`None` to Rust `Option::None` is acceptable; substituting a placeholder object is
not. Document any unavoidable language differences and test them.

Runtime production code must stay pure Rust. Python and upstream OpenNURBS are
development references and test oracles. Existing Rust `cadmpeg` functionality
can be reused behind a private adapter when its behavior passes conformance.
Full ownership of every decoder implementation is a separate backend goal;
wrapping a dependency does not establish semantic parity.

The target is not all of RhinoCommon, Rhino's application GUI, Rhino Compute,
Grasshopper, a photorealistic renderer, or a robotics compiler. Those have
different APIs. In the audited Python wheel, `BrepFace.GetMesh` exists and
`Mesh.CreateFromBrep` does not. Cached render-mesh access is required; a general
new BRep tessellator is an optional extension unless a pinned target method
actually needs it. Boolean/fillet/meshing operations must be scoped from the
inventory, not copied from RhinoCommon documentation.

## Versioned target and actual API size

The latest PyPI distribution observed during the audit is **8.32.1**. The
official CPython 3.10 macOS wheel reports `rhino3dm.__version__ == "8.32.2"`.
Both versions and the verified wheel SHA-256 are recorded in
[oracle-lock.json](oracle-lock.json). Pin that artifact for the initial target;
record separate hashes/platform inventories when enabling Linux and Windows.
Keep 8.17.0 as the existing Clay regression oracle. Do not install over the
user's Clay environment.

| Runtime inventory | 8.17.0 regression | 8.32.1 distribution target |
| --- | ---: | ---: |
| Exported classes, including enums and binding iterator classes | 193 | 212 |
| Enum classes | 46 | 49 |
| Public class-member slots, including inherited slots | 3,159 | 3,359 |
| Declared members plus selected language protocols | 2,391 | 2,645 |

These are inventory counts, **not unique methods, overload counts, or a parity
percentage**. The earlier 105-class estimate came from an incomplete `.pyi`.
The generated operation ledger now contains 3,229 callable/property
obligations. Runtime-writable properties create explicit setter obligations
even when the shipped stub omits them. One hundred twenty-two obligations are backed by
eight passing, pinned-oracle foundation/geometry cases and every
other entry remains incomplete. Existing Rust types with the same name are not
automatically compatible.

Newer exported types include BrepLoop/BrepTrim collections, SubDEdge/SubDFace/
SubDVertex and tags, and nine `BND_SubD…Iterator…` classes. Investigate those
binding-named iterators explicitly: reproduce their public iteration behavior
or record a reviewed exception; do not silently remove them from the target.

The machine-readable inventories include constructors, operators, inherited
member names, property mutability, enums, and callable docstrings:

- [python-api-8.32.1.json](python-api-8.32.1.json)
- [python-api-8.17.0.json](python-api-8.17.0.json)
- Generator: [tools/parity/inventory_python.py](../../tools/parity/inventory_python.py)

Introspection does not discover every overload or error condition. P00 merges
these snapshots with release binding source and behavior probes.

To regenerate the offline guide, install
`tools/parity/requirements-docs.txt` in an isolated development environment,
then run `python tools/parity/render_handoff.py` from the repository root.
`python tools/parity/render_handoff.py --check` checks freshness and local
links/anchors. The installed Rust library has no dependency on these tools.

## What the current standalone Rust repository actually has

| Family | Current evidence | Missing to reach Python parity | Plan |
| --- | --- | --- | --- |
| Archive / File3dm | Signature, version, tables, object framing; retained immutable source spans; 1 GiB default / explicit source-byte limit; header-only version read; direct class-UUID CRC check | Whole-chunk CRC scope, declared-version matrix, nested resource budgets, reader/byte-array APIs, metadata, general writing | P01, P03, P04, P08 |
| Points / vectors / transforms / primitives | Point/vector fields; tested Point2d/Point3d/Point3f/Point4d/Vector2d/Vector3d/Vector3f construction, mutable coordinates and encoding; tested Point2d distance/addition; tested f32 narrowing; tested interval endpoint state; tested Line endpoint/property/interpolation/transform behavior; tested transform composition, inversion, predicates, transpose, axis-angle rotation; ON_Point data; instance matrix read | Full constructors, operators, bounds, planes, intervals, primitives and behavior | P02 |
| Object attributes / tables | Native ID/name/layer/UserStrings subset; scene projection has more presentation fields | Editable table model, complete fields, properties, groups, linetypes, views, settings, document strings and userdata | P03, P07, P10 |
| Instances | Native definition IDs/names/members and reference matrices; bridge occurrences | Shared typed IDs, resolved nested occurrences, cycles/duplicates, linked blocks, edit/write, ownership/lifetime semantics | P03, P06, P08 |
| Curves | Bridge exact-carrier census; public native types absent | Line/arc/polyline/polycurve/Bezier/NURBS, proxies, knots/control points, evaluation and exposed edits | P05 |
| Surfaces / BReps / extrusions | Bridge carriers and some display data; limited class classification | Typed analytic/NURBS surfaces, BRep topology including trims/loops, extrusion APIs, validity and cached mesh access | P09 |
| Meshes / point collections | Scene RenderMesh triangles, normals, raw channels | Rhino-native quads, precision arrays, topology, UVs/colors, point clouds/grids, creation/edit/query/write | P04 |
| SubD | No matching public Rust topology API | Control net/topology/tags, iterators, subdivision and exposed queries/serialization | P11 |
| Materials / textures / RDK | Scene layers, legacy/PBR scalars, texture transforms, asset descriptors | Exact embedded bytes, mapping/UV semantics, source inheritance, editable material/render-content trees and exposed APIs | P07, P10 |
| Text / dimensions / annotations / hatches | Object-level metadata only | Typed annotation classes, styles, formatting, geometry, font information and write support | P10 |
| Views / lights / render settings | No general Python-equivalent table API | Cameras/projections/clipping, named views, light/sun/environment/ground-plane/workflow/post-effect data and methods | P10 |
| Serialization / dictionaries / compression | Narrow exact STEP-to-3DM writer via Rust bridge | General File3dm.Write, Encode/Decode, FromByteArray, ArchivableDictionary and Draco wrappers | P08, P10, P11 |
| Extensions | Technical SVG renderer, Three.js scene projection, exact STEP admission | Preserve these consumers; they do not count toward Python parity | P06, P07, P12 |

The standalone checkout is ahead of the old nested crate at
`vitrus-4/vitrus-convert/crates/rhino3dm-rs`. Its scene/material work and narrow
STEP writer must be preserved. Build the parity work in the standalone
repository; migrate the Vitrus consumer to a pinned release later.

## Current high-priority limitations

The first P01/P02 changes remove the prior silent definition drop, distinguish
partially preserved attributes, retain source bytes, bound header reads, and
verify direct class-UUID CRCs. They are deliberately not a claim that archive
integrity or document editing is complete. The next limitations are:

1. Container/object CRC scope, EOF/trailing-data policy, archive-version
   coverage, and explicit document/count/decompression/depth budgets still need
   parser-level implementation and malformed-input conformance.
2. `GeometryProbe::complete_object_decode` parses human-readable warning text
   and checks only a census plus error severity. That cannot prove all object
   fields, topology, textures or valid references survived.
3. `SceneDocument::read` decodes again and serializes the entire bridge IR into
   JSON. This duplicates work/memory and creates a fragile schema dependency.
4. Scene capabilities are static booleans; several JSON helpers default absent
   data. Library support, observed document coverage, and required output
   capabilities must be distinct.
5. The scene code documents bridge coordinates normalized to millimeters, but
   `threejs_manifest` labels them document units. There is no end-to-end unit
   contract or equivalent cross-unit fixture yet.
6. Source UUIDs occur as raw wire arrays, strings, and internal bridge record
    IDs. Their conversion and identity joins need one explicit contract.
7. The renderer omits instance references; `ThreejsManifest` lacks a complete
    definition/occurrence transform graph. Resolve membership before drawing
    definition members as independent world geometry.
8. The all-targets suite passes **20 tests** (17 core, 3 renderer), with three
   additional Python/Rust differential math cases. That is a tested foundation,
   not a per-operation conformance suite. No checked-in `.github` CI workflow
   was present in this checkout.

P01 owns archive/diagnostic fixes; P02/P03 own math/ID/unit contracts; P06 owns
scene joins and occurrences. Do not fix a fidelity failure by loosening a
consumer's admission check.

## R06 and the earlier speed claim

The external source hash was refreshed during this audit and still matches the
lock. Python 8.17.0 reports 2,511 objects: 1,040 Breps, 673 NURBS curves, 356
instances, 229 lines, 61 polyline curves, 48 polycurves, 38 points, 32 arcs, 25
extrusions, 4 text dots, 3 linear dimensions and 2 meshes. Earlier equal counts
for metadata/block membership are a useful census, not a value-level proof.

The 36 R06 instance matrices previously called “projective” differ from an
exact affine last row by at most **1.8070621780539365e-18**, measured through
Python. The earlier nil-UUID explanation was also unsupported. Preserve matrix
values, test Python's predicates and composition behavior, and avoid a blanket
rejection based on exact floating-point equality.

The earlier 42× R06 claim compared a partial Rust index with Python loading a
full geometry model. It is not an equal-work full-converter speedup. The newer
checked-in [structural benchmark](../../benchmarks/results/latest.json) clearly
labels its subset; even matching its aggregate census does not prove geometry
equivalence. Geometry/API performance must be measured after the corresponding
value-level contract passes. No full Rust-versus-Clay conversion benchmark is
established by this audit.

## References and ownership

- [McNeel rhino3dm](https://github.com/mcneel/rhino3dm): bindings and geometry API.
- [Python release metadata](https://pypi.org/pypi/rhino3dm/8.32.1/json): primary wheel/source artifact identity.
- [Python API documentation](https://mcneel.github.io/rhino3dm/python/api/): navigation; verify version against the oracle.
- [8.17 source commit](https://github.com/mcneel/rhino3dm/tree/1e0c80e6b9cf7ff3c1579d287e178b21634a2a6d): regression bindings and tests.
- Source files in the pinned sdist: `src/bindings/bnd_*.cpp`, headers, and
  bundled `src/lib/opennurbs`. Check binding build flags; Python and JavaScript
  exports can differ. Do not use the moving `8.x` branch as a frozen oracle.

Preserve applicable MIT/OpenNURBS/cadmpeg/Draco notices when translating or
reusing code. Python signatures in the inventory originate from McNeel's MIT
licensed package. Unknown plugin behavior is outside what even the Python
package can interpret; preserve its opaque data and match exposed behavior.
