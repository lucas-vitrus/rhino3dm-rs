# Audit verification receipt

Date: 2026-09-08. Audited base:
`34a8846b01db1d4aa9dafb11ab42f7a09776d30d`. This receipt includes the
subsequent P00/P01/P02 foundation changes; it is not a claim of completed
Python package parity.

## P03 point/document slice receipt

The current bounded P03 slice adds a mutable `File3dm` authoring projection
for layers and point objects, native point writing through the Rust Rhino
encoder, point readback, object/layer lookup and mutation, and explicit
UserString mutation helpers. A Rust-authored point archive was read by the
installed Python `rhino3dm` 8.17.0 runtime with exact coordinates
`(1.25, 2.5, 3.75)`. A Python-authored archive containing a named point, one
layer, and one UserString was read by Rust with the layer name, point, object
name, and UserString count recovered.

This is a bounded slice, not package parity. Rust writing still refuses custom
layer records, layer assignment, explicit object IDs, and UserStrings until
their native presentation/userdata wire contracts have exact round-trip tests.
Decoded metadata failures are exposed through `File3dm::metadata_error()` and
are never represented as an empty successful table.

## Executed checks

## P04 mesh read projection slice

The first bounded P04 slice exposes native Rhino mesh tessellations decoded by
the existing pure-Rust bridge through `File3dm::meshes()`. The projection
preserves document-unit vertex positions and indexed triangle data while
remaining explicit about its boundary: it is not yet the mutable Python
`Mesh`, vertex/face collection, quad/ngon, UV/color, or mesh-writer API.

A Python-authored `rhino3dm` 8.17.0 fixture containing five vertices, one quad
face and one triangle face was read by the Rust indexer as one mesh with five
vertices and three display triangles. The quad-to-triangle expansion is
reported as bridge tessellation data; the original native quad face is not
claimed to be preserved by this projection.

## P04 native mesh mutation seed

Rust-created `Mesh` values now preserve triangle versus quad face arity and
provide bounded vertex/face access plus add, replace, and clear operations.
The installed Python `rhino3dm` 8.17.0 oracle establishes that
`Mesh.Faces.AddFace` retains an invalid face while returning `-1`; valid
triangle and quad counts exclude that retained invalid face. Rust matches that
contract for the supported collection methods. This does not yet establish
native source-mesh quad recovery, normals, UVs, colors, topology, or writing.

`Mesh.Vertices.Clear()` was also compared directly: Python retains the face
records, clears the vertex collection, and makes those faces ineligible for
valid triangle/quad counts. Rust now has the same vertex-clear and retained
face behavior.

Native bridge mesh projections now retain per-vertex normals. Rust-created
meshes expose normal addition, clearing, flipping, unitization, and averaged
normal computation for valid triangle/quad faces. The mixed Python-authored
fixture reports five vertices, three display triangles, and five normals.
Normals remain a bounded slice: source normal seams, UVs, colors, topology,
and mesh writing are not yet complete.

Vertex colors are now retained when the native bridge exposes the four-byte
color channel. Rust-created meshes provide indexed RGBA color storage with
add and clear operations; the Python 8.17.0 collection probe confirms the
three-channel `Add(red, green, blue)` shape and indexed return behavior. A
Python-authored color fixture recovers three native vertex colors in Rust.

The next five P04 checks extend this bounded slice. UV channels are recovered
from the bridge's two-float vertex channel and can be stored/cleared on Rust
meshes. Deterministic undirected topology edges and edge lines are derived
from valid triangle/quad faces. A bounded `PointCloud` core supports point
addition, count, indexed query, and clear, but its optional normal/color/
hidden/value item channels are not yet integrated. `Mesh::write` now emits a
standalone native mesh with normals, UVs, and colors. Its Rust-authored quad
path writes Rhino's native four-index face form and recomputes the exact
class-data checksum scope; Python 8.17.0 reads it as one quad (zero triangles),
with four vertices, four colors, and four normals. This writer is deliberately
limited to source-less Rust meshes: imported mesh projections still expose the
bridge's triangle view and cannot yet be written back without losing native
source topology.
The Python 8.17.0 `Hide`/`Show` mesh-vertex probe produced no observable state
change, so hidden-state parity remains an explicit unresolved runtime finding.

## P05 curve read projection seed

`File3dm::curves()` now exposes the bridge's typed curve carriers without
sampling them into display points. A Python-authored fixture containing a line
curve, polyline curve, and quadratic NURBS curve was decoded as three typed
curve carriers. This is still read-only: mutable curve wrappers, evaluation
parity, and curve writing remain separate obligations.

| Check | Result |
| --- | --- |
| `cargo test --workspace --all-targets` | PASS: 24 library tests + 3 renderer tests; binary targets had no unit tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --all` and `git diff --check` | PASS |
| Paired `math-basics-v1` Python/Rust conformance | PASS with `atol=rtol=1e-12`; covers noncommuting composition, inverse fallback, Point3d transform, Vector3d mutation and observed f32 narrowing in `Translation(Vector3d)` |
| Paired `math-operations-and-boundaries-v1` Python/Rust conformance | PASS with `atol=rtol=1e-12`; covers point arithmetic, vector operations/degeneracy, transform sentinels/predicates, transpose and axis-angle rotation |
| Paired `math-value-object-state-v1` Python/Rust conformance | PASS with exact numeric comparison; covers Point3d/Vector3d equality, mutation, coordinate getters and `Encode`, plus `Point3d.Unset` |
| Paired `foundation-2d-and-interval-v1` Python/Rust conformance | PASS with `atol=rtol=1e-12`; covers Point2d construction/distance/addition/mutation/encoding, Vector2d construction/mutation/encoding, and mutable Interval endpoints/equality |
| Paired `foundation-point3f-v1` Python/Rust conformance | PASS with exact numeric comparison; covers Point3f f32 construction, addition, mutable coordinates, equality, and encoding |
| Paired `foundation-vector3f-v1` Python/Rust conformance | PASS with exact numeric comparison; covers Vector3f f32 construction, mutable coordinates, equality, and encoding |
| Paired `foundation-point4d-v1` Python/Rust conformance | PASS with exact numeric comparison; covers Point4d construction, four mutable coordinates, equality, and encoding |
| Paired `geometry-line-v1` Python/Rust conformance | PASS with `atol=rtol=1e-12`; covers mutable endpoints, direction/length/tangent/validity, extrapolating `PointAt`, degenerate behavior, and in-place transform |
| Overload-aware operation ledger generation | PASS: 3,229 obligations, 122 paired-case-backed `passing` mappings, 1,044 unassessed and 2,063 runtime/stub divergences |
| `cargo run --quiet --bin rhino3dm-index -- fixtures/structural-benchmark-v1.3dm` | PASS: 2,305 framed objects; 2,049 points; 256 instance references; one definition with one member; no reported framing/geometry parse errors |
| P03 Rust point writer → Python `rhino3dm` readback | PASS: one point; exact coordinates `(1.25, 2.5, 3.75)`; named-point readback also preserves `NamedPoint` |
| P03 Python-authored layer/name/UserString fixture → Rust `File3dm` read projection | PASS: one layer, one point, one object name, one UserString; metadata diagnostics remain explicit |
| P04 Python-authored mixed mesh fixture → Rust `File3dm::meshes()` projection | PASS: one mesh, five vertices, three indexed triangles; quad expansion explicitly reported |
| P04 mesh collection mutation against Python 8.17.0 | PASS: vertex addition, valid triangle/quad counts, invalid-face retention with `-1`, replacement and face clearing |
| P04 vertex clear against Python 8.17.0 | PASS: vertices clear while face records remain; valid face counts become zero |
| P04 mesh normals against Python 8.17.0 and native fixture | PASS: `ComputeNormals`, `Flip`, `UnitizeNormals`, `Clear`, and five native normals observed |
| P04 mesh vertex colors against Python 8.17.0 and native fixture | PASS: indexed color addition/clear and three native four-byte color entries recovered |
| P04 UV channel and topology slice | PASS: four native UV entries recovered; deterministic valid-face edge derivation and edge-line query tested |
| P04 PointCloud core slice | PASS: point add/count/index/clear tested; optional item channels remain incomplete |
| P04 source-less mesh writer → Python 8.17.0 readback | PASS: one Rust mesh read by Python as four vertices, two triangles, four colors, and four normals; quad arity is intentionally triangulated |
| P04 hidden vertex probe | INCOMPLETE finding: Python 8.17.0 `Hide`/`Show` calls produced no observable hidden-state change in the tested mesh |
| P05 Python-authored line/polyline/NURBS fixture → Rust `File3dm::curves()` projection | PASS: three typed curve carriers; no sampled-point flattening |
| Primary Python runtime inspection | Distribution 8.32.1, runtime 8.32.2; 212 exported classes including 49 enums |
| Clay Python runtime inspection | Distribution/runtime 8.17.0; 193 exported classes including 46 enums |
| Primary inventory regenerated in a fresh output directory | PASS: exact JSON equality with checked-in primary snapshot |
| Inventory overwrite refusal | PASS: second write failed and previous output hash remained unchanged |
| Unexpected distribution refusal | PASS: rejected before creating the output file |
| Python tool syntax and JSON artifact parsing | PASS |
| HTML regeneration/freshness and local link/anchor validation | PASS via `tools/parity/render_handoff.py --check` |
| Root tracked diff whitespace | PASS via `git diff --check`; new files also inspected separately |
| Primary wheel and sdist download SHA-256 | PASS: values in oracle-lock.json verified against downloaded bytes |
| R06 external fixture | Read-only current Rust index: 2,511 framed objects, 224 decoded definitions, 2,436 definition members, 356 decoded instance references, and zero structural framing/attribute/geometry parse errors; SHA-256 before and after was the locked `5ecaa195…735e0e` |

The index's `attribute_records_complete` field is an existing parser-consumed
flag, not a full field-preservation guarantee; see the audit findings. Neither
this CLI run nor the 11 tests establishes full geometry or API equivalence.

HTML is generated with Python Markdown 3.8.2 and has no external runtime assets.
Automatic visual inspection was not completed: the in-app browser's URL policy
refused the local `file:` URL. No workaround was attempted. Open the adjacent
`index.html` directly in your browser to review its layout. Local links and
anchor targets were checked programmatically; external URLs were not all
re-requested by the HTML validator.

## Scope and untouched data

Changes include documentation/oracle tooling plus a bounded production Rust
foundation: immutable source retention, per-definition parse outcomes,
attribute coverage states, direct class-UUID CRC validation, bounded header
reads, a default/explicit source-byte admission limit, and a tested
Point2d/Point3d/Point3f/Point4d/Vector2d/Vector3d/Vector3f/Interval/Line/Transform slice. `crc32fast` is the only
new direct Rust dependency. There is no general 3DM writer, full geometry
decoder, published release, or equal-work full-converter benchmark. The existing
Clay environment and original R06 `.3dm` were not modified. New Python packages
were installed only into an isolated temporary oracle environment.

The initial primary oracle is CPython 3.10 on macOS arm64 using the locked
universal2 wheel. Linux/Windows inventories and differential operation coverage
are future work. The exact release-source build correspondence remains a P00
provenance task even though the wheel and sdist artifact hashes are verified.
