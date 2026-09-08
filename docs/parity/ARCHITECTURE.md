# Architecture for Python API parity

Status: proposed target architecture. Paths and signatures below are not claims
that the APIs exist today. Implement in the standalone repository and preserve
its current clients through adapters/deprecations.

## Module boundaries

Keep one core crate initially. Splitting every module into another crate would
add coordination before its interfaces are stable. Keep the existing renderer
as its own workspace package. Make STEP and the temporary bridge optional
features once their dependency boundaries are isolated.

```text
                    Rust application / CLI / future language bindings
                                      |
                           rhino3dm_rs::File3dm
                        typed document and geometry
                        /                         \
          document, attributes, tables       geometry, math
                        \                         /
                         archive reader / writer
                           |                |
                    native class codecs   private bridge adapter
                           |                |  (per-operation coverage)
                           +---- ByteStore -+

  document -> scene adapter -> rhino3dm-render / Three.js adapter
  document <-> STEP adapter                  (optional extension)
  Python oracle -> conformance fixtures -> Rust operation runner (tests only)
```

```text
src/
  lib.rs                         public facade and reexports
  archive/
    mod.rs                       read modes and archive version policies
    store.rs                     immutable source storage and checked spans
    cursor.rs                    bounded primitive reads, counts, offsets
    chunks.rs                    chunk framing, parent bounds, CRC rules
    reader.rs                    tables, class registry, read reports
    writer.rs                    chunk sizes, CRCs, versioned records
    extensions.rs                raw userdata and plugin records
    limits.rs                    bytes, counts, recursion, decompression
    codecs/                      per-class versioned read/write payloads
  document/
    mod.rs                       File3dm, file metadata, edit revision
    object.rs                    File3dmObject, Geometry handle, attributes
    tables.rs                    stable identity, iteration, find/add/delete
    settings.rs                  units, tolerances, file flags
    instances.rs                 definitions, references, graph resolution
  geometry/
    mod.rs                       typed Geometry enum and common operations
    point.rs, pointcloud.rs
    curve.rs, nurbs_curve.rs, polycurve.rs
    surface.rs, nurbs_surface.rs, extrusion.rs
    mesh.rs, brep.rs, subd.rs
    annotation.rs, hatch.rs
  math/                          vectors, transforms, primitives, tolerances
  attributes.rs                  complete object fields + extension data
  appearance/                    materials, PBR, textures, RDK, assets
  views/                         camera, viewport, lights, render settings
  serialization/                 object/dictionary encoding, Draco adapters
  compatibility/                 pinned Python behavior mappings
  diagnostics.rs                 structured failures, coverage and provenance
  bridge/cadmpeg.rs               private IR projection and loss mapping
  scene.rs                       existing renderer API, migrated incrementally
  step.rs                        existing optional exact STEP extension
tools/parity/                    oracle inventory and operation harness
tests/conformance/               language-neutral cases and expected facts
fixtures/conformance/            small redistributable models
```

## A document owns one source and one typed model

Use an immutable `ByteStore` with `Arc<[u8]>` as the initial backend. A span is
valid only against its owning store. A later streaming/file-backed reader can
implement the same contract; defer memory mapping until its platform and unsafe
boundary is justified. Checked offsets use `u64`; conversion to `usize` is
fallible. Header-only version reads consume only the header.

The document keeps metadata, tables and objects with stable handles, plus the
original archive index and diagnostics. Lazy payload decoding may cache typed
geometry keyed by source span and decode revision. Incomplete decode must be
stored as a result, never an absent object. Repeated reads of the same cached
payload must not repeat decompression or duplicate buffers. Immutable shared
reads can later be `Send + Sync`; mutable editing starts with `&mut File3dm`.

Geometry has typed variants for every supported class family and an explicit
opaque variant carrying class ID, raw payload, userdata and diagnostics. Shared
operations such as bounds/transform/duplicate/validity dispatch on the enum;
do not expose the bridge's IR or `serde_json::Value` as the long-term modeling
contract. JSON remains useful for diagnostics, inventories and adapters.

BRep, surface, mesh and annotation objects can hold component handles into
owned arenas. A BrepFace/BrepEdge/CurveProxy must remain associated with its
parent and orientation. Mutating through a face/point-list/table view must
update the owning document exactly once, invalidate dependent caches and avoid
detached copies. Define what happens to handles after deletion/reindexing.
Use generation-checked handles or scoped borrows; never expose dangling views.

## Separate compatibility, decode coverage and consumer requirements

Proposed contracts (illustrative, not implemented):

```rust
pub struct ReadOptions {
    pub mode: ReadMode,             // inspect/recover or strict per contract
    pub limits: DecodeLimits,
    pub preserve_unknown: bool,
    pub verify_checksums: bool,
}

pub enum FieldCoverage {
    DecodedExact,
    DecodedEquivalent { evidence: EvidenceId },
    RetainedOpaque,
    Unsupported,
    Corrupt,
}

pub struct ReadReport {
    pub archive: ArchiveIntegrity,
    pub objects: Vec<ObjectCoverage>,
    pub tables: Vec<TableCoverage>,
    pub diagnostics: Vec<Diagnostic>,
}

impl File3dm {
    pub fn read_with_options(path: impl AsRef<Path>, options: ReadOptions)
        -> Result<(Self, ReadReport), ReadError>;
    pub fn from_bytes(bytes: Arc<[u8]>, options: ReadOptions)
        -> Result<(Self, ReadReport), ReadError>;
    pub fn write(&self, writer: impl Write, options: WriteOptions)
        -> Result<WriteReport, WriteError>;
}
```

A library capability says a decoder exists. Document coverage says what was
actually recovered. A consumer requirement says what the requested operation
needs. Keep all three separate. A display consumer may use cached triangles;
a BRep query needs its topology and surfaces; a lossless edit/write also needs
unknown-data retention. There is no universal `complete=true` derived from
equal record counts. Keep legacy `complete_object_decode` as a deprecated
census helper until clients migrate, and clearly document its limited meaning.

Diagnostics carry stable codes, severity, object UUID when available, table,
class ID, source span, field, archive/writer version and the affected
capability. Preserve one result for every source object/definition, including
failures. Recovery mode reports corruption; strict mode rejects the requested
operation. Structural parse success, valid geometry and complete public API
support are distinct. Match Python's ability to represent invalid geometry
and `IsValidWithLog`; do not prohibit all invalid or unset values at read time.

## Identity, units, transforms and inheritance

Use one UUID type with tested `from_wire_le`, `to_wire_le`, canonical string and
nil operations. The first 4/2/2 UUID byte groups have wire endianness semantics;
hex-encoding the entire raw array is not a canonical UUID. Keep internal record
IDs separate from source UUIDs. Detect duplicate source IDs without overwriting
records. Define lookup ambiguity and Python-equivalent insertion behavior.

The core document retains **source units** and tolerances, matching Python.
No implicit millimeter/meter normalization occurs in its geometry APIs. A
bridge that normalizes units must explicitly reverse or tag that normalization
before producing a document value. Scene/robotics adapters request an output
unit and record the scale. Core geometry stays Rhino Z-up. Do not rotate axes
inside a decoder.

Represent `Transform` as a documented row-major 4×4 matrix with a fixed vector
convention, verified against Python. Test composition with rotation plus
translation (noncommuting), homogeneous division, reflection, nonuniform scale,
singular transforms, inversion and normals. Preserve every stored coefficient.
`IsAffine`, `IsValid`, etc. follow the oracle's predicates, including tolerance
behavior. R06's tiny nonzero last-row coefficients are a dedicated regression.

Resolve a block through definition membership and an occurrence path; compose
parent and child matrices in the verified order. Apply transform once. Resolve
visibility, ByLayer/ByObject/ByParent color/material rules at the occurrence,
not by changing the reusable definition. Bound recursion/expansion, detect
cycles and missing definitions, and preserve negative-determinant orientation.
Only roots appear as independent world objects; definition-owned objects stay
local unless instantiated. Identity includes occurrence path so two instances
of one source mesh cannot overwrite each other.

## Mesh and exact geometry are different representations

`Mesh` is a Rhino modeling object: original vertex precision, native triangle/
quad faces, normals, UVs, colors, topology and exposed mutation operations.
`RenderMesh` is a derived display view and may triangulate. Preserve quads,
ngon membership and indexing before generating a display view. Retain separate
render/analysis/preview cache types and per-Brep-face ownership.

BReps use indexed vertices/edges/trims/loops/faces/surfaces, curve/surface proxy
domains and orientations. Validate bounds and index ranges without replacing
invalid source geometry with an invented solid. A trimmed face requires its
2D trim curves and 3D edges, including seam/singular cases. NURBS preserve order,
degree, knot convention, rational control-point weights, domains and periodic
state. Match exposed constructors and operations via behavioral tests.

There are three separate implementation tasks: reading exact geometry, reading
cached display meshes, and generating new display meshes. Only the first two
are unconditional parity requirements. Any new tessellator has its own
tolerance/quality contract and may not replace exact geometry or claim Rhino
tessellation identity.

## General writing and edit invalidation

Build a versioned writer alongside each supported reader, starting with points
and document tables. Preserve UUIDs, source order where observable, indices,
units, settings, nesting, userdata and correct chunk/CRC rules. Keep archive
version, writer version, class payload version and API distribution version
distinct. Probe what the pinned Python writer actually accepts, including
default `version=0`; do not assume all historic versions are writable.

An unmodified document may reuse an opaque record only when retained bytes,
CRC scope and references remain valid. If edits change indices, versions or
parents, raw bytes may no longer be safe to replay. Track dirty dependencies;
re-encode known fields and reject a requested lossless write whose opaque
references cannot be repaired. Byte preservation and semantic preservation
are separate guarantees. Metadata such as timestamps may legitimately differ.

All failure paths must leave an existing destination untouched. Write to a
unique sibling, finalize length/checksums, verify where appropriate, then
replace atomically under the documented platform policy. The current exact
STEP path has this pattern, but its narrow writer is not File3dm.Write parity.
Cache invalidation applies to bounds, render meshes, topology, material/UV
projections and encoded payloads after every relevant edit.

Treat CAD archives and embedded filenames as untrusted input. Reading a texture
reference must not automatically fetch a URL or open arbitrary external paths.
Expose references as data; asset resolution is an explicit caller policy with
an allowed root. Bound embedded extraction and reject path traversal/symlink
escapes before writing assets. Rust memory safety does not prevent resource
exhaustion, filesystem traversal or accidental network access.

## How the bridge is reused and retired

Create a private adapter that accepts the same source/limits and returns typed
fields plus structured coverage. Maintain a per-operation ledger with
`native`, `bridge`, `opaque` and `missing` backend states. Public behavior can
be verified for a bridge operation; dependency presence alone gives no credit.
Do not project all IR through JSON or execute two full file reads per API call.
Do not promote a count-only probe into a Mesh/Brep result.

For each family: extract oracle cases, test the current bridge, expose only
the fields that preserve identity/units/topology, then replace unsupported
codec/algorithm pieces natively. A feature build with the bridge disabled
proves which capabilities are actually native. Full pure-Rust dependency reuse
is permitted; OpenNURBS FFI or invoking Python in production is not the selected
architecture. If a dependency is translated/forked, pin provenance and retain
licenses; do not hand-copy unexplained UUIDs or wire layouts.

## Rust and Python interface mapping

Use idiomatic Rust names in the core and record `PythonClass.Member(overload)`
to Rust symbol mappings. Model Python overloads with separate constructors,
typed input enums or well-scoped conversion traits. Match defaults, indexing,
ordering, mutation aliasing, precision, return tuples/booleans/None and failures.
Language-level TypeError/IndexError can map to typed Rust errors with preserved
meaning. Rust does not need to mimic Python's garbage collector or dynamic
types, but its observable object/collection behavior must be specified.

Do not create hundreds of empty public types to improve counts. Introduce a
type with one tested read/create/query/write slice, then fill its operation
ledger. A Python binding of the new Rust core is optional; it is a useful later
compatibility harness, not a prerequisite for an embeddable Rust library.
