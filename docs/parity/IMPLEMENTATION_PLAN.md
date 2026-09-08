# Implementation roadmap and acceptance gates

These are ordered work packages for a coding agent, not completed features.
See the [audited baseline](README.md), [architecture](ARCHITECTURE.md) and
[test contract](CONFORMANCE.md). Never report a phase complete solely because
it compiles or a file renders. Every phase updates the per-symbol ledger and
passes the cases named below. Estimates are relative: BRep, NURBS and SubD are
major geometry-library work, not small binding exercises.

## Dependency order

```text
P00 Oracle + operation ledger
  └─ P01 Archive integrity + retained source + diagnostics
       └─ P02 Math / IDs / units
            └─ P03 Typed document, tables and attributes
                 ├─ P04 Meshes + point collections + first round trip
                 ├─ P05 Curves + NURBS
                 └─ P06 Instances + scene consumer integration (after P04)
                      └─ P07 Appearance + embedded assets (after P04)
P03 + P04 + P05 + P06 ─ P08 General document writing
P05 + P08 ─ P09 Surfaces + BReps + extrusions
P07 + P08 ─ P10 Annotations + views + remaining document APIs
P04 + P08 + P09 ─ P11 SubD + Draco + remaining operation closure
P00..P11 ─ P12 Cross-platform, conformance and release
```

Work may overlap after interfaces are stable. Keep one owner for changes to
the document model, diagnostics and writer. An implementation agent can use
parallel workers only if authorized in its session; this document is a work
map, not an instruction to spawn tasks now. Ship a tested vertical slice for
each geometry family rather than waiting until the last phase to write any
3DM data.

## P00 — Freeze the oracle and turn the surface inventory into obligations

Inputs: the two checked-in runtime inventories and oracle lock. Own
`tools/parity/`, `tests/conformance/`, `docs/parity/` and future CI wiring.

Deliver:

- Obtain source matching the locked wheel, record the release source archive
  hash/build flags and applicable OpenNURBS identity. Resolve the distribution
  versus runtime version mismatch explicitly. The verified sdist is the source
  reference until an exact release commit/build correspondence is established.
- Add an operation ledger with stable IDs for every target class member,
  constructor overload, property getter/setter, enum value and collection
  protocol. Deduplicate inherited implementations while retaining inherited
  exposure obligations. Do not silently omit the BND iterator classes.
- Merge `.pyi`, runtime docstrings and `src/bindings/bnd_*.cpp/.h` Python
  exports. Store source pointers, defaults and behavior-unknown entries.
- Create a language-neutral operation runner protocol and small Python oracle
  generator. Create Rust runner skeleton that reports unsupported operations
  distinctly. It is a test utility, not a new production backend.
- Add explicit CI fixtures and a target-free unit-test job; Python-dependent
  conformance jobs use isolated, hash-pinned environments.

Exit: the ledger covers all observed exports, unresolved overloads are listed,
oracle snapshots are reproducible, and at least one point/transform case runs
both languages with an intentional mismatch proving the comparator can fail.

## P01 — Make archive results trustworthy

Own `src/archive/*`, `src/diagnostics.rs`, and migrate `src/lib.rs` internals.

Deliver:

- Retained ByteStore/spans, checked cursor, archive/version metadata, header-only
  read, explicit document/record/count/string/decompression/depth budgets.
- CRC verification by actual OpenNURBS scope, including nested chunks and
  class UUID exceptions. Verify EOF size/termination and trailing-data policy.
- One result per object/definition; remove silent definition parse drops.
  Separate consumed/skipped/opaque/decoded data in coverage.
- Replace warning-string census parsing with structured diagnostics/census.
  Preserve old APIs through clearly documented wrappers until consumers move.
- Start property/fuzz testing on truncation, mutated lengths, counts, CRCs,
  unknown classes and malformed userdata; record failure offsets.

Exit: damaged input never panics or allocates beyond limits; neighboring good
records remain inspectable in recovery mode; strict operations reject the
damaged required data; unchanged opaque bytes can be retrieved from the store.
All previous public fixture and R06 index facts remain accounted for.

First PR scope: source retention + definition diagnostics + corruption cases.
Second PR: CRC/EOF/version/budget coverage. Avoid a single parser rewrite.

## P02 — Numerical value types, UUIDs and source units

Own `src/math/*` and shared ID/unit contracts.

Deliver Point2/3/4 float/double variants, vectors, intervals, Plane,
BoundingBox, Line/Arc/Circle/Ellipse/Box/Sphere/Cone/Cylinder as exposed, and
Transform operations and classification. Add methods according to the ledger;
not all primitive types have the same constructor set.

Document source-unit behavior, OpenNURBS unset values, tolerances, signed zero,
non-finite inputs, precision conversion and degenerate geometry. Implement
canonical/wire UUID conversions once and use them everywhere. Probe arithmetic,
comparisons, vector normalization, inverse failures, bounds and intersections
only for operations the oracle exposes.

Exit: asymmetric noncommuting transforms, singular/inverse failure, mirrored
and nonuniform scale, tiny affine-row residuals, zero vectors, degenerate
intervals/bounds and unit-scale fixtures pass. Coordinates and IDs are not
silently normalized or renamed. Each exposed primitive has creation/query
cases; no math API gets blanket parity from `Transform::IDENTITY`.

## P03 — Mutable File3dm, attributes and basic tables

Own `src/document/*`, `src/attributes.rs` and identity integration.

Deliver File3dm/File3dmObject and geometry handles; typed object, layer,
material, group and instance-definition tables with find/iterate/add/delete.
Decode metadata/settings/units/tolerances/document strings and complete modern
object attributes, including visibility, colors/material sources, groups,
names, IDs and object/geometry/document UserStrings. Older attribute layouts
follow the archive matrix. Keep plugin userdata opaque until supported.

Define live view semantics, index stability, duplicate/nil ID behavior,
ordering, nil versus missing fields, locked/default layer behavior, Unicode,
user-string replacement/deletion and cache invalidation. Add typed read/edit
coverage reports. Start a minimal point+layer+metadata writer with correct
reference tables; P08 broadens writing rather than starting from zero.

Exit: create a point on a named layer with metadata in Rust; Python reads exact
IDs/coordinates/layer/strings; Python-authored fixture reads identically in
Rust; deleting/editing a view has expected owner behavior. Unrecognized
attribute fields remain visible in coverage.

## P04 — Native Mesh, cached mesh data and point collections

Own `geometry/mesh.rs`, `geometry/pointcloud.rs` and their codecs.

Deliver compressed/raw array decoding with bounded expansion and checksums;
native triangles/quads, float/double vertices, normals, UVs, colors, topology,
ngon-related source data, point clouds/grids/lists and supported modifiers.
Add Mesh and point-cloud creation/query/mutation methods from the ledger.
Read/write standalone Mesh payloads. Prepare cache helpers for BrepFace.GetMesh
and Extrusion.GetMesh; full owning-class integration lands in P09.

Exit: a mixed quad/triangle mesh with duplicate positions, UV seam, vertex
colors, double vertices and invalid-index variant matches the oracle. After
normal/face/vertex edits, bounding/topology/cache data remains consistent.
Missing cache and malformed cache are distinguishable. A display triangulation
never erases the original modeling representation.

## P05 — Curves and NURBS evaluation

Own curve codecs and `geometry/{curve,nurbs_curve,polycurve}.rs`.

Deliver LineCurve, ArcCurve, Polyline/PolylineCurve, PolyCurve, BezierCurve,
NurbsCurve, CurveProxy; control-point/knot collections, rational weights,
domains, dimensions, orientation, exposed constructors and mutation methods.
Implement evaluation/derivatives/tangents, conversion, reversal, trimming and
other operations actually present in the pinned ledger. Writer support ships
with each curve family.

Exit: degree 1/2/3/higher, rational circle, repeated knots, endpoint knots,
closed/periodic, reversed/nested polycurves, 2D trim curves and degenerate
fixtures compare sampled values and query outcomes. Preserve exact knot/control
data on round trip and verify post-edit evaluations. Segment boundaries and
derivative discontinuities get explicit one-sided cases.

## P06 — Assembly semantics and existing consumer integration

Own instance resolution and scene/renderer integration; depends on P03/P04.

Deliver resolved definition/member references, occurrence paths, parent/child
matrix composition, shared geometry caching, ByParent/ByLayer presentation,
linked-block metadata and missing/external reference policy. External files
are referenced; resolving them requires caller-supplied path policy.
Add graph diagnostics for cycles, duplicates and missing members. Integrate
block edits with writer tracking.

Migrate SceneDocument to the typed document/bridge adapter. Remove whole-IR
JSON copying and repeated file decode where practical. Fix declared versus
actual units. Export a complete occurrence graph in the Three.js adapter.
Make the SVG renderer draw resolved occurrences and preserve explicit omissions.

Exit: a nested asymmetric block repeated twice with rotation/translation,
reflection and nonuniform scale has the same world coordinates and inherited
styles as Python-composed reference values. Definition members are not double
drawn. Cycles/missing definitions report bounded failures. R06 compares all
definition/member IDs and per-reference matrices, not counts alone.

## P07 — Appearance, assets and material inheritance

Own `appearance/*`; reuse current scene PBR/texture descriptors.

Deliver legacy Material and PhysicallyBasedMaterial exposed methods/properties;
textures/mapping transforms/channels/wrapping, embedded file bytes and paths,
layer/object/face/instance assignments, source-vs-resolved material distinction,
and supported RDK fields. Geometry-dependent UV operations depend on P04/P06.
Preserve unknown render-content parameters with diagnostics and extend writer
coverage. Use the existing [PBR requirements](../pbr-material-parity.md).

Exit: non-default material scalar values, checker texture, seams, multiple
mapping channels, embedded/external assets and instanced copies match Python
field values and asset hashes. Absent data is not silently defaulted into a
claim of parity. Visual rendering is an additional consumer test, not the
definition of package API parity.

## P08 — General File3dm writing and object serialization

Own `archive/writer.rs`, serialization, write options and version policy.

Broaden earlier writers to document metadata, all supported tables, geometry,
definitions/references, attributes/userdata and caches. Implement exposed
File3dm byte-array/Encode/Decode methods and CommonObject Encode/Decode, with
version/save-userdata/default/error behavior measured against Python.
Do not mistake `import_exact_step` for a general writer.

Introduce dirty-graph handling and opaque retention rules. Probe source UUID
and table-index rewrite behavior on insert/delete. Probe exactly which target
versions Python writes and what it intentionally drops; publish the tested
read/write matrix rather than a broad unsupported version claim.

Exit: Python→Rust→Python and Rust→Python→Rust edit round trips pass for P02–P07.
Existing destination survives every refusal/error. Unknown records round-trip
only where references/version remain safe; otherwise lossless mode refuses
with the specific dependency. Byte-identical file output is not the semantic
parity criterion.

## P09 — Surfaces, BRep topology and extrusions

Own surface/BRep/extrusion APIs/codecs. This is one of the largest work packages.

Deliver Surface/SurfaceProxy, NurbsSurface control points and knots,
PlaneSurface/RevSurface, Extrusion profile/path/caps and exposed conversions;
Brep/BrepFace/BrepEdge/BrepVertex plus current BrepTrim/BrepLoop collections.
Preserve surfaces/2D trims/3D curves, proxy domains, adjacency, seam/singular
trims, orientation and source cache ownership. Implement exposed constructors,
queries, edits, validity/manifold/solid results and writer round trips.

Exit: planar trimmed face with a hole, cylinder seam, sphere singularity,
multi-face closed solid, open shell, flipped face, rational surface and capped/
uncapped extrusion compare topology and sampled evaluations. Each cached
render/analysis mesh stays attached to its original face and is compared
separately. Invalid BReps preserve inspectable invalidity and failure behavior.
Geometry is never accepted merely because its bbox/counts look correct.

## P10 — Remaining document/modeling families

Own annotations, dictionaries, settings, views and remaining appearance APIs.

Deliver Text/TextDot/Leader/Dimension subclasses/Centermark, fonts and dimension
styles, Hatch; linetypes and all remaining File3dm tables; viewports/cameras/
projections/named views, lights, sun/environment/ground plane/workflow/render
settings; RenderContent child trees, post effects, decals and mesh-modifier
data; ArchivableDictionary and all remaining exposed common-object methods.

This phase is driven by the operation ledger: every unassigned export must be
assigned to a family and ticket. Do not stop at a hand-picked list of familiar
classes. Include getters/setters, defaults, collections, serialization and
failure cases for each family. Missing font/platform-dependent behavior is
declared and tested; no CI skip counts as passing.

Exit: every non-SubD/Draco export has an implemented and tested mapping or an
explicit unresolved ticket. Final parity still requires closing those tickets.

## P11 — SubD, Draco and remaining operation closure

Own SubD topology/evaluation/tags/iterators and compression adapter work.

Implement current SubDVertex/SubDEdge/SubDFace access, incidence iterators,
control-net operations, tags, cache invalidation, subdivision and exposed
validity/serialization. Test boundaries, creases, extraordinary vertices,
closed solids and both directions of iteration. Assess a compatible pure-Rust
Draco implementation against the Python wrapper; bitstream/API interoperability
must pass before admitting it. An FFI wrapper is outside the selected runtime
architecture. If no suitable implementation exists, native Draco work remains
a visible parity blocker.

Exit: all primary target operation tickets close, including platform/type
oddities and protocol behavior. No “not used by robotics” omission qualifies
as 1:1 Python parity.

## P12 — Release qualification and integration

Run full inventory-to-ledger closure, source-version fixture matrix,
differential operation suite, fuzz/regression corpus and cross-language edit
round trips. Qualify macOS arm64/x86_64, Linux x86_64/aarch64, Windows x86_64
with pinned oracle artifacts; list additional targets separately. Match each
platform's expected behavior without assuming wheel inventories are identical.

Publish tested runtime/dependency/MSRV/platform support, a capability matrix,
remaining deviations (must be zero for an unqualified 1:1 release), changelog,
licenses and runnable Rust migration examples. Benchmarks compare identical
output contracts in release mode after conformance passes. Keep the native
backend ledger separate if bridge code remains.

Then update the Vitrus consumer to the standalone crate's pinned version and
run the separate Clay robotics parity suite. A full package-parity release and
full robotics-converter parity are different deliverables with different
acceptance tests.

## First concrete agent assignment

Implement P00's operation ledger/runner and P01's source-retention plus
definition-diagnostic slice. Start with existing source and 11 passing tests.
Do not begin a full BRep port first. Deliver one Rust-created point+layer+string
model read by the Python oracle next (P02/P03). This establishes the complete
create/read/edit/write testing loop that every later geometry family reuses.
