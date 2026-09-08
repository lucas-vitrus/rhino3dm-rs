//! A pure-Rust, source-read-only foundation for Rhino 3DM archives.
//!
//! The public names deliberately follow the observable `rhino3dm.py` model
//! where that improves migration (`File3dm`, `Point3d`, `ObjectAttributes`).
//! Unsupported data is reported explicitly; it is never decoded as empty data.

use cadmpeg_codec_rhino::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};
use cadmpeg_ir::codec::{EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::{BodyId, PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::math::Point3 as IrPoint3;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{Body as IrBody, BodyKind, Point as IrPoint, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

pub mod scene;
pub mod step;

pub const FILE_SIGNATURE: &[u8; 24] = b"3D Geometry File Format ";
const HEADER_LENGTH: usize = 32;
/// Default upper bound for one retained 3DM source archive (1 GiB).
///
/// `File3dm` deliberately retains bytes for checked opaque ranges, so this is
/// a real resident-memory admission limit rather than merely a read buffer.
pub const DEFAULT_MAX_SOURCE_BYTES: usize = 1024 * 1024 * 1024;
const TCODE_SHORT: u32 = 0x8000_0000;
const TCODE_CRC: u32 = 0x0000_8000;
const TCODE_END_OF_FILE: u32 = 0x0000_7fff;
const TCODE_END_OF_TABLE: u32 = 0xffff_ffff;
const TCODE_OBJECTS: u32 = 0x1000_0013;
const TCODE_INSTANCE_DEFINITIONS: u32 = 0x1000_0021;
const TCODE_INSTANCE_DEFINITION_RECORD: u32 = 0x2000_8076;
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const OBJECT_RECORD_TYPE: u32 = 0x8200_0071;
const OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const OBJECT_RECORD_END: u32 = 0x8200_007f;
const OPENNURBS_CLASS: u32 = 0x0002_7ffa;
const CLASS_UUID: u32 = 0x0002_fffb;
const CLASS_DATA: u32 = 0x0002_fffc;
const CLASS_END: u32 = 0x8002_7fff;
const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const ANONYMOUS: u32 = 0x4000_8000;
const USER_STRING_LIST_WIRE: [u8; 16] = [
    0x29, 0xde, 0x28, 0xce, 0xc5, 0xf4, 0xaa, 0x4f, 0xa5, 0x0a, 0xc3, 0xa6, 0x84, 0x9b, 0x63, 0x29,
];
const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const INSTANCE_REFERENCE_CLASS: [u8; 16] = [
    0x38, 0xb6, 0xcf, 0xf9, 0xd4, 0xb9, 0x40, 0x43, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
];
const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const BREP_CLASS: [u8; 16] = [
    0x43, 0xc2, 0x6f, 0xf0, 0x2a, 0xa3, 0x08, 0x46, 0x9d, 0xd8, 0xa7, 0xd2, 0xc4, 0xce, 0x2a, 0x36,
];
const EXTRUSION_CLASS: [u8; 16] = [
    0x75, 0x31, 0xf5, 0x36, 0xb8, 0x72, 0x47, 0x4d, 0xbf, 0x1f, 0xb4, 0xe6, 0xfc, 0x24, 0xf4, 0xb9,
];
const INSTANCE_DEFINITION_CLASS: [u8; 16] = [
    0xf6, 0xbf, 0xf8, 0x26, 0x18, 0x26, 0x7f, 0x41, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File3dmHeader {
    pub archive_version: u32,
}

/// Resource limit applied before a source archive is retained in memory.
///
/// Set a larger value explicitly for a known large model. Limits for decoded
/// strings, records and compressed payloads are separate P01 work and are not
/// implied by this source-byte admission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    pub max_source_bytes: usize,
}

impl ReadLimits {
    pub const fn new(max_source_bytes: usize) -> Self {
        Self { max_source_bytes }
    }
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SOURCE_BYTES)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceRange {
    pub offset: u64,
    pub length: u64,
}

/// Immutable source bytes retained by a [`File3dm`].
///
/// Ranges in the archive index are meaningful only against this store. The
/// reader keeps exactly one shared allocation, so opaque records can be
/// inspected without rereading the input or inventing a decoded replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStore {
    bytes: Arc<[u8]>,
}

impl SourceStore {
    fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }

    /// Entire immutable source archive.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Source archive size in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether this store is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Return a checked slice for an indexed source range.
    pub fn slice(&self, range: SourceRange) -> Result<&[u8], Error> {
        let start = usize::try_from(range.offset).map_err(|_| Error::OutOfBounds {
            offset: range.offset,
            end: u64::MAX,
            bound: self.bytes.len() as u64,
        })?;
        let end = usize::try_from(end(range)?).map_err(|_| Error::OutOfBounds {
            offset: range.offset,
            end: u64::MAX,
            bound: self.bytes.len() as u64,
        })?;
        self.bytes.get(start..end).ok_or(Error::OutOfBounds {
            offset: range.offset,
            end: end as u64,
            bound: self.bytes.len() as u64,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveRecord {
    pub typecode: u32,
    pub source: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveTable {
    pub typecode: u32,
    pub source: SourceRange,
    pub records: Vec<ArchiveRecord>,
}

/// Framed identity of an object record. Geometry payload remains opaque until
/// its concrete type reader is implemented.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRecord {
    pub source: SourceRange,
    pub object_type: Option<u32>,
    pub class_id: Option<[u8; 16]>,
    pub class_data: Option<SourceRange>,
    pub attributes: Option<ObjectAttributes>,
    pub attribute_error: Option<String>,
    pub attribute_userdata_error: Option<String>,
    pub geometry_kind: GeometryKind,
    pub point: Option<Point3d>,
    pub instance_reference: Option<InstanceReference>,
    pub geometry_error: Option<String>,
    pub framing_error: Option<String>,
}

/// A lossless structural index of an archive. Payload decoding follows in the
/// next compatibility stages; ranges are retained exactly in source order.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveIndex {
    pub tables: Vec<ArchiveTable>,
    pub objects: Vec<ObjectRecord>,
    /// One result for every instance-definition table record. A failed parse
    /// remains inspectable instead of disappearing from the archive index.
    pub instance_definition_records: Vec<InstanceDefinitionRecord>,
    /// Successfully decoded definitions, retained for backwards-compatible
    /// callers. Inspect `instance_definition_records` for failures.
    pub instance_definitions: Vec<InstanceDefinition>,
    pub end_of_file: SourceRange,
}

impl ArchiveIndex {
    pub fn object_records(&self) -> impl Iterator<Item = &ArchiveRecord> {
        self.tables
            .iter()
            .filter(|table| without_crc(table.typecode) == TCODE_OBJECTS)
            .flat_map(|table| table.records.iter())
            // The object-record code itself carries the CRC bit by definition.
            .filter(|record| record.typecode == TCODE_OBJECT_RECORD)
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}

/// A source-readable 3DM model with an archive-level structural index.
#[derive(Debug, Clone, PartialEq)]
pub struct File3dm {
    header: File3dmHeader,
    archive: ArchiveIndex,
    source: SourceStore,
    /// Mutable document tables used by newly-created documents. Read-only
    /// archive loading will populate these tables as their codecs become
    /// available; keeping them separate from the structural index prevents
    /// edits from masquerading as decoded source data.
    layers: Vec<Layer>,
    objects: Vec<PointObject>,
    meshes: Vec<Tessellation>,
    mesh_views: Vec<Mesh>,
    curves: Vec<Curve>,
    metadata_error: Option<String>,
}

/// A mutable point object for the first document-object slice.
#[derive(Debug, Clone, PartialEq)]
pub struct PointObject {
    pub geometry: Point3d,
    pub attributes: ObjectAttributes,
}

/// A document layer in the Python `rhino3dm.Layer` surface.
///
/// This is the first mutable document-table slice. The index is stable within
/// a document and is assigned by [`File3dm::add_layer`]. More presentation
/// fields will be added only with oracle-backed cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub index: i32,
    pub parent_layer_id: Option<[u8; 16]>,
    pub id: [u8; 16],
    pub visible: bool,
    pub locked: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, id: [u8; 16]) -> Self {
        Self {
            name: name.into(),
            index: -1,
            parent_layer_id: None,
            id,
            visible: true,
            locked: false,
        }
    }
}

/// Machine-readable coverage gates for consumers that require more than the
/// currently supported structural archive API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderCapabilities {
    pub pbr_materials: bool,
    pub embedded_texture_bytes: bool,
    pub mesh_uv_channels: bool,
    pub texture_mapping_transforms: bool,
    pub per_face_materials: bool,
}

impl DecoderCapabilities {
    /// True only when a consumer can reconstruct textured PBR appearance
    /// without silent defaults or omitted source data.
    pub fn complete_pbr_reconstruction(self) -> bool {
        self.pbr_materials
            && self.embedded_texture_bytes
            && self.mesh_uv_channels
            && self.texture_mapping_transforms
            && self.per_face_materials
    }
}

/// Geometry recovered by the current pure-Rust geometry backend.
///
/// This is intentionally a report, not a fabricated native mesh API: callers
/// can admit output only when `complete_object_decode` is true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryProbe {
    pub backend: &'static str,
    pub units: Value,
    pub entity_counts: BTreeMap<String, usize>,
    pub native_namespaces: Vec<String>,
    pub decoded_object_records: Option<usize>,
    pub source_object_records: Option<usize>,
    pub warnings: Vec<String>,
}

impl GeometryProbe {
    pub fn complete_object_decode(&self) -> bool {
        self.decoded_object_records
            .zip(self.source_object_records)
            .is_some_and(|(decoded, source)| decoded == source)
            && !self
                .warnings
                .iter()
                .any(|warning| warning.starts_with("Error:"))
    }
}

impl File3dm {
    /// Create an empty mutable document.
    ///
    /// The returned document is an in-memory authoring model. It is not yet a
    /// general `.3dm` writer; callers must not treat it as serializable until
    /// the P03 writer slice lands.
    pub fn new() -> Self {
        Self {
            header: File3dmHeader { archive_version: 8 },
            archive: ArchiveIndex {
                tables: Vec::new(),
                objects: Vec::new(),
                instance_definition_records: Vec::new(),
                instance_definitions: Vec::new(),
                end_of_file: SourceRange::default(),
            },
            source: SourceStore::new(Arc::from([])),
            layers: Vec::new(),
            objects: Vec::new(),
            meshes: Vec::new(),
            mesh_views: Vec::new(),
            curves: Vec::new(),
            metadata_error: None,
        }
    }

    /// Coverage of the current public decoder API.
    ///
    /// These flags are deliberately conservative. Opaque preservation inside
    /// a backend does not count as decoded, reconstructable data.
    pub const fn capabilities() -> DecoderCapabilities {
        DecoderCapabilities {
            pbr_materials: false,
            embedded_texture_bytes: false,
            mesh_uv_channels: false,
            texture_mapping_transforms: false,
            per_face_materials: false,
        }
    }

    /// Read and structurally index a native 3DM file without modifying it.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::read_with_limits(path, ReadLimits::default())
    }

    /// Read and structurally index a native 3DM file under an explicit source
    /// byte admission limit, without modifying the source file.
    pub fn read_with_limits(path: impl AsRef<Path>, limits: ReadLimits) -> Result<Self, Error> {
        let path = path.as_ref();
        let actual = std::fs::metadata(path)?.len();
        if actual > limits.max_source_bytes as u64 {
            return Err(Error::InputTooLarge {
                limit: limits.max_source_bytes,
                actual,
            });
        }
        Self::from_bytes_with_limits(std::fs::read(path)?, limits)
    }

    /// Structurally index retained 3DM bytes without copying them again.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, Error> {
        Self::from_bytes_with_limits(bytes, ReadLimits::default())
    }

    /// Structurally index retained 3DM bytes under an explicit source-byte
    /// admission limit without copying the caller's bytes again.
    pub fn from_bytes_with_limits(
        bytes: impl Into<Arc<[u8]>>,
        limits: ReadLimits,
    ) -> Result<Self, Error> {
        let source = SourceStore::new(bytes.into());
        if source.len() > limits.max_source_bytes {
            return Err(Error::InputTooLarge {
                limit: limits.max_source_bytes,
                actual: source.len() as u64,
            });
        }
        let header = parse_header(source.as_bytes())?;
        let archive = scan_archive(source.as_bytes(), header.archive_version)?;
        let objects = archive
            .objects
            .iter()
            .filter_map(|record| {
                record.point.map(|geometry| PointObject {
                    geometry,
                    attributes: record.attributes.clone().unwrap_or_default(),
                })
            })
            .collect();
        let (layers, mut metadata_error) = match decode_layers(source.as_bytes()) {
            Ok(layers) => (layers, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let meshes = match RhinoCodec.decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        ) {
            Ok(decoded) => decoded.ir().model.tessellations.clone(),
            Err(error) => {
                let message = format!("mesh projection: {error}");
                metadata_error = Some(match metadata_error {
                    Some(existing) => format!("{existing}; {message}"),
                    None => message,
                });
                Vec::new()
            }
        };
        let mesh_views = meshes
            .iter()
            .map(|mesh| Mesh {
                vertices: mesh
                    .vertices
                    .iter()
                    .map(|point| Point3d {
                        x: point.x,
                        y: point.y,
                        z: point.z,
                    })
                    .collect(),
                faces: mesh
                    .triangles
                    .iter()
                    .map(|indices| MeshFace::Triangle(indices.map(|index| index as i32)))
                    .collect(),
                normals: mesh
                    .normals
                    .iter()
                    .map(|normal| Vector3f::new(normal.x as f32, normal.y as f32, normal.z as f32))
                    .collect(),
                vertex_colors: mesh
                    .channels
                    .iter()
                    .find(|channel| channel.kind == 0x5248_0002 && channel.item_size == 4)
                    .and_then(|channel| {
                        let expected = usize::try_from(channel.count).ok()?.checked_mul(4)?;
                        (channel.data.len() == expected).then(|| {
                            channel
                                .data
                                .chunks_exact(4)
                                .map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3]])
                                .collect()
                        })
                    })
                    .unwrap_or_default(),
            })
            .collect();
        let curves = match RhinoCodec.decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        ) {
            Ok(decoded) => decoded.ir().model.curves.clone(),
            Err(error) => {
                let message = format!("curve projection: {error}");
                metadata_error = Some(match metadata_error {
                    Some(existing) => format!("{existing}; {message}"),
                    None => message,
                });
                Vec::new()
            }
        };
        Ok(Self {
            header,
            archive,
            source,
            layers,
            objects,
            meshes,
            mesh_views,
            curves,
            metadata_error,
        })
    }

    /// Read only the fixed header required to discover the archive version.
    pub fn read_archive_version(path: impl AsRef<Path>) -> Result<u32, Error> {
        let mut input = File::open(path)?;
        Ok(read_header(&mut input)?.archive_version)
    }

    pub fn archive_version(&self) -> u32 {
        self.header.archive_version
    }

    pub fn header(&self) -> &File3dmHeader {
        &self.header
    }

    pub fn archive(&self) -> &ArchiveIndex {
        &self.archive
    }

    /// Immutable source storage that owns all archive ranges.
    pub fn source(&self) -> &SourceStore {
        &self.source
    }

    /// Return the exact bytes for a checked source range.
    pub fn source_slice(&self, range: SourceRange) -> Result<&[u8], Error> {
        self.source.slice(range)
    }

    /// Layers in document order.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Diagnostic from the optional native metadata projection.
    ///
    /// Structural archive parsing can succeed while the richer native
    /// metadata bridge fails. Callers requiring complete layer metadata must
    /// check this value instead of interpreting an empty layer slice as proof
    /// that the source had no layers.
    pub fn metadata_error(&self) -> Option<&str> {
        self.metadata_error.as_deref()
    }

    /// Native mesh tessellations decoded by the pure-Rust Rhino bridge.
    ///
    /// This is a read projection of source tessellations, not the complete
    /// mutable `rhino3dm.Mesh` collection API. Native quad/ngon/cache fields
    /// remain outside this bounded slice.
    pub fn meshes(&self) -> &[Tessellation] {
        &self.meshes
    }

    /// Read-only mesh values with the Python-facing vertex/face shape.
    pub fn mesh_views(&self) -> &[Mesh] {
        &self.mesh_views
    }

    /// Typed curve carriers decoded by the pure-Rust Rhino bridge.
    ///
    /// This is a read projection; mutable `rhino3dm.Curve` wrappers and
    /// object-table writing are intentionally not implied by this method.
    pub fn curves(&self) -> &[Curve] {
        &self.curves
    }

    /// Add a layer and return its stable document index.
    pub fn add_layer(&mut self, mut layer: Layer) -> i32 {
        let index = self.layers.len() as i32;
        layer.index = index;
        self.layers.push(layer);
        index
    }

    /// Find a layer by its native UUID bytes.
    pub fn find_layer(&self, id: [u8; 16]) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    /// Find a mutable layer by its native UUID bytes.
    pub fn find_layer_mut(&mut self, id: [u8; 16]) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == id)
    }

    /// Add a point to a newly-created mutable document.
    ///
    /// This currently stages the object in the authoring model. Serialization
    /// is intentionally separate and remains unsupported until the point
    /// writer has been matched against the pinned Python oracle.
    pub fn add_point(&mut self, geometry: Point3d, attributes: ObjectAttributes) -> usize {
        let index = self.objects.len();
        self.objects.push(PointObject {
            geometry,
            attributes,
        });
        index
    }

    pub fn objects(&self) -> &[PointObject] {
        &self.objects
    }

    /// Number of mutable point objects in the authoring/document projection.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Return one mutable point object by document index.
    pub fn object(&self, index: usize) -> Option<&PointObject> {
        self.objects.get(index)
    }

    /// Return one mutable point object for in-place geometry/attribute edits.
    pub fn object_mut(&mut self, index: usize) -> Option<&mut PointObject> {
        self.objects.get_mut(index)
    }

    /// Delete one mutable point object and return it when the index existed.
    pub fn delete_object(&mut self, index: usize) -> Option<PointObject> {
        (index < self.objects.len()).then(|| self.objects.remove(index))
    }

    /// Write a minimal native Rhino archive for the supported mutable point
    /// slice. Metadata, layers, and non-point objects are rejected until their
    /// wire contracts are implemented and oracle-tested.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        if !self.source.is_empty() {
            return Err(Error::Unsupported {
                capability: "editing and rewriting a decoded archive",
            });
        }
        if !self.layers.is_empty()
            || self.objects.iter().any(|object| {
                object.attributes.id.is_some()
                    || object.attributes.layer_index.is_some()
                    || !object.attributes.user_strings.is_empty()
            })
        {
            return Err(Error::Unsupported {
                capability: "serializing layers or point attributes",
            });
        }
        let mut ir = CadIr::empty(Units::default());
        for (index, object) in self.objects.iter().enumerate() {
            if !object.geometry.x.is_finite()
                || !object.geometry.y.is_finite()
                || !object.geometry.z.is_finite()
            {
                return Err(Error::Unsupported {
                    capability: "a point with non-finite coordinates",
                });
            }
            let point_id = PointId(format!("rhino3dm:object:point#{index}"));
            ir.model.points.push(IrPoint {
                id: point_id.clone(),
                position: IrPoint3::new(object.geometry.x, object.geometry.y, object.geometry.z),
                source_object: None,
            });
            if let Some(name) = object.attributes.name.clone() {
                let body_id = BodyId(format!("rhino3dm:object:body#{index}"));
                let region_id = RegionId(format!("rhino3dm:object:region#{index}"));
                let shell_id = ShellId(format!("rhino3dm:object:shell#{index}"));
                let vertex_id = VertexId(format!("rhino3dm:object:vertex#{index}"));
                ir.model.bodies.push(IrBody {
                    id: body_id.clone(),
                    kind: BodyKind::General,
                    regions: vec![region_id.clone()],
                    transform: None,
                    name: Some(name),
                    color: None,
                    visible: Some(true),
                });
                ir.model.regions.push(Region {
                    id: region_id,
                    body: body_id,
                    shells: vec![shell_id.clone()],
                });
                ir.model.shells.push(Shell {
                    id: shell_id,
                    region: RegionId(format!("rhino3dm:object:region#{index}")),
                    faces: Vec::new(),
                    wire_edges: Vec::new(),
                    free_vertices: vec![vertex_id.clone()],
                });
                ir.model.vertices.push(Vertex {
                    id: vertex_id,
                    point: point_id,
                    tolerance: None,
                });
            }
        }
        let plan = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .map_err(|error| Error::Decode(error.to_string()))?;
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes)
            .map_err(|error| Error::Decode(error.to_string()))?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Decode supported curves, Breps, extrusions and meshes through the
    /// packaged Rust geometry bridge. The report always carries any loss;
    /// callers must check `complete_object_decode` before export.
    pub fn probe_geometry(path: impl AsRef<Path>) -> Result<GeometryProbe, Error> {
        let mut input = File::open(path)?;
        let decoded = RhinoCodec
            .decode(&mut input, &DecodeOptions::default())
            .map_err(|error| Error::Decode(error.to_string()))?;
        let value =
            serde_json::to_value(decoded.ir()).map_err(|error| Error::Decode(error.to_string()))?;
        let mut entity_counts = BTreeMap::new();
        if let Some(model) = value.get("model").and_then(Value::as_object) {
            for (name, entries) in model {
                if let Some(entries) = entries.as_array() {
                    entity_counts.insert(name.clone(), entries.len());
                }
            }
        }
        let native_namespaces = value
            .get("native")
            .and_then(Value::as_object)
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default();
        let warnings = decoded
            .report()
            .losses
            .iter()
            .map(|loss| format!("{:?}: {}", loss.severity, loss.message))
            .collect::<Vec<_>>();
        let (decoded_object_records, source_object_records) = decode_progress(&warnings);
        Ok(GeometryProbe {
            backend: "cadmpeg-codec-rhino@0.5.5 (pure Rust bridge)",
            units: value.get("units").cloned().unwrap_or(Value::Null),
            entity_counts,
            native_namespaces,
            decoded_object_records,
            source_object_records,
            warnings,
        })
    }
}

impl Default for File3dm {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// One native mesh face. Unlike a render triangulation, this preserves whether
/// a caller supplied a triangle or quad, including invalid source indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshFace {
    Triangle([i32; 3]),
    Quad([i32; 4]),
}

/// The bounded mesh model currently supported by `File3dm`.
///
/// Bridge-read meshes contain triangulated display faces. New Rust meshes can
/// preserve triangle/quad arity and intentionally retain invalid indices to
/// match Python's observable `Mesh.Faces.AddFace` behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Point3d>,
    pub faces: Vec<MeshFace>,
    pub normals: Vec<Vector3f>,
    pub vertex_colors: Vec<[u8; 4]>,
}

impl Mesh {
    /// Create an empty mesh, matching Python's `Mesh()` constructor.
    pub const fn new() -> Self {
        Self {
            vertices: Vec::new(),
            faces: Vec::new(),
            normals: Vec::new(),
            vertex_colors: Vec::new(),
        }
    }

    /// Python `Mesh.Vertices.Count` equivalent for the read projection.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Python `Mesh.Faces.Count` equivalent. Invalid retained faces count too.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Python `Mesh.Faces.TriangleCount` equivalent: only valid triangles are
    /// counted, while invalid retained faces remain inspectable.
    pub fn triangle_count(&self) -> usize {
        self.faces
            .iter()
            .filter(
                |face| matches!(face, MeshFace::Triangle(indices) if self.face_is_valid(indices)),
            )
            .count()
    }

    /// Python `Mesh.Faces.QuadCount` equivalent for valid quad faces.
    pub fn quad_count(&self) -> usize {
        self.faces
            .iter()
            .filter(|face| matches!(face, MeshFace::Quad(indices) if self.face_is_valid(indices)))
            .count()
    }

    /// Add one vertex and return its zero-based index.
    pub fn add_vertex(&mut self, point: Point3d) -> usize {
        let index = self.vertices.len();
        self.vertices.push(point);
        index
    }

    /// Replace one vertex. Returns false when the index is out of bounds.
    pub fn set_vertex(&mut self, index: usize, point: Point3d) -> bool {
        let Some(vertex) = self.vertices.get_mut(index) else {
            return false;
        };
        *vertex = point;
        true
    }

    /// Number of stored vertex normals.
    pub fn normal_count(&self) -> usize {
        self.normals.len()
    }

    /// Add one single-precision vertex normal and return its index.
    pub fn add_normal(&mut self, normal: Vector3f) -> usize {
        let index = self.normals.len();
        self.normals.push(normal);
        index
    }

    /// Remove all stored vertex normals.
    pub fn clear_normals(&mut self) {
        self.normals.clear();
    }

    /// Number of stored vertex colors.
    pub fn color_count(&self) -> usize {
        self.vertex_colors.len()
    }

    /// Add one RGBA vertex color. Python's three-channel `Add` maps to an
    /// opaque color by convention; callers may supply any explicit alpha.
    pub fn add_color(&mut self, color: [u8; 4]) -> usize {
        let index = self.vertex_colors.len();
        self.vertex_colors.push(color);
        index
    }

    /// Remove all stored vertex colors.
    pub fn clear_colors(&mut self) {
        self.vertex_colors.clear();
    }

    /// Negate all stored normals, matching `Mesh.Normals.Flip()`.
    pub fn flip_normals(&mut self) {
        for normal in &mut self.normals {
            normal.x = -normal.x;
            normal.y = -normal.y;
            normal.z = -normal.z;
        }
    }

    /// Normalize every stored normal. Returns false when there is no normal
    /// data or when any normal is zero/non-finite.
    pub fn unitize_normals(&mut self) -> bool {
        if self.normals.is_empty() {
            return false;
        }
        let mut valid = true;
        for normal in &mut self.normals {
            let length = f32::sqrt(normal.x * normal.x + normal.y * normal.y + normal.z * normal.z);
            if !length.is_finite() || length == 0.0 {
                valid = false;
                continue;
            }
            normal.x /= length;
            normal.y /= length;
            normal.z /= length;
        }
        valid
    }

    /// Compute averaged per-vertex normals from valid triangle and quad faces.
    /// Degenerate or invalid faces are skipped. Returns false if no face
    /// contributes a normal.
    pub fn compute_normals(&mut self) -> bool {
        let mut sums = vec![[0.0_f64; 3]; self.vertices.len()];
        let mut contributed = false;
        for face in &self.faces {
            let indices: Vec<usize> = match face {
                MeshFace::Triangle(indices) => indices
                    .iter()
                    .map(|&index| usize::try_from(index))
                    .collect::<Result<_, _>>()
                    .ok()
                    .filter(|indices: &Vec<usize>| {
                        indices.iter().all(|&index| index < self.vertices.len())
                    })
                    .unwrap_or_default(),
                MeshFace::Quad(indices) => indices
                    .iter()
                    .map(|&index| usize::try_from(index))
                    .collect::<Result<_, _>>()
                    .ok()
                    .filter(|indices: &Vec<usize>| {
                        indices.iter().all(|&index| index < self.vertices.len())
                    })
                    .unwrap_or_default(),
            };
            let triangles: Vec<[usize; 3]> = match indices.as_slice() {
                [a, b, c] => vec![[*a, *b, *c]],
                [a, b, c, d] => vec![[*a, *b, *c], [*a, *c, *d]],
                _ => Vec::new(),
            };
            for [a, b, c] in triangles {
                let ab = (
                    self.vertices[b].x - self.vertices[a].x,
                    self.vertices[b].y - self.vertices[a].y,
                    self.vertices[b].z - self.vertices[a].z,
                );
                let ac = (
                    self.vertices[c].x - self.vertices[a].x,
                    self.vertices[c].y - self.vertices[a].y,
                    self.vertices[c].z - self.vertices[a].z,
                );
                let cross = (
                    ab.1 * ac.2 - ab.2 * ac.1,
                    ab.2 * ac.0 - ab.0 * ac.2,
                    ab.0 * ac.1 - ab.1 * ac.0,
                );
                let length = (cross.0 * cross.0 + cross.1 * cross.1 + cross.2 * cross.2).sqrt();
                if !length.is_finite() || length == 0.0 {
                    continue;
                }
                contributed = true;
                for index in [a, b, c] {
                    sums[index][0] += cross.0;
                    sums[index][1] += cross.1;
                    sums[index][2] += cross.2;
                }
            }
        }
        if !contributed {
            return false;
        }
        self.normals = sums
            .into_iter()
            .map(|[x, y, z]| {
                let length = (x * x + y * y + z * z).sqrt();
                if length == 0.0 || !length.is_finite() {
                    Vector3f::default()
                } else {
                    Vector3f::new(
                        (x / length) as f32,
                        (y / length) as f32,
                        (z / length) as f32,
                    )
                }
            })
            .collect();
        true
    }

    /// Clear every vertex while retaining faces, matching
    /// `Mesh.Vertices.Clear()`. Retained faces become invalid until matching
    /// vertices are added again and therefore no longer contribute to valid
    /// triangle/quad counts.
    pub fn clear_vertices(&mut self) {
        self.vertices.clear();
    }

    /// Add a triangle face. The face is always retained. Returns its index
    /// when all indices are valid, or `-1` just like Python `AddFace`.
    pub fn add_triangle(&mut self, indices: [i32; 3]) -> i32 {
        let index = self.faces.len() as i32;
        self.faces.push(MeshFace::Triangle(indices));
        if self.face_is_valid(&indices) {
            index
        } else {
            -1
        }
    }

    /// Add a quad face. The face is always retained. Returns its index when
    /// valid, or `-1` for an invalid retained face.
    pub fn add_quad(&mut self, indices: [i32; 4]) -> i32 {
        let index = self.faces.len() as i32;
        self.faces.push(MeshFace::Quad(indices));
        if self.face_is_valid(&indices) {
            index
        } else {
            -1
        }
    }

    /// Replace one face and report whether the resulting face is valid.
    pub fn set_face(&mut self, index: usize, face: MeshFace) -> bool {
        let valid = match &face {
            MeshFace::Triangle(indices) => self.face_is_valid(indices),
            MeshFace::Quad(indices) => self.face_is_valid(indices),
        };
        let Some(destination) = self.faces.get_mut(index) else {
            return false;
        };
        *destination = face;
        valid
    }

    /// Clear every face while leaving vertices untouched.
    pub fn clear_faces(&mut self) {
        self.faces.clear();
    }

    /// Bounds-checked equivalent of `Mesh.Vertices.Point3dAt`.
    pub fn vertex(&self, index: usize) -> Option<Point3d> {
        self.vertices.get(index).copied()
    }

    /// Bounds-checked triangle query for the current display projection.
    pub fn face(&self, index: usize) -> Option<MeshFace> {
        self.faces.get(index).copied()
    }

    fn face_is_valid<const N: usize>(&self, indices: &[i32; N]) -> bool {
        indices
            .iter()
            .all(|&index| usize::try_from(index).is_ok_and(|index| index < self.vertices.len()))
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

/// A mutable single-precision point corresponding to Python's
/// `rhino3dm.Point3f`.
///
/// Construction and arithmetic deliberately use `f32`, preserving the
/// binding's observable narrowing rather than pretending this is a second
/// double-precision point type.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A mutable homogeneous four-dimensional point corresponding to Python's
/// `rhino3dm.Point4d`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point4d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

/// A mutable two-dimensional point with the same coordinate precision as
/// Python's `rhino3dm.Point2d` binding.
///
/// Rust exposes fields in its normal `snake_case` style; they correspond to
/// the Python binding's writable `X` and `Y` properties.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point2d {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryKind {
    Point,
    InstanceReference,
    Mesh,
    Brep,
    Extrusion,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vector3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A mutable single-precision vector corresponding to Python's
/// `rhino3dm.Vector3f`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A mutable two-dimensional vector with the same coordinate precision as
/// Python's `rhino3dm.Vector2d` binding.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vector2d {
    pub x: f64,
    pub y: f64,
}

/// A mutable numeric interval corresponding to Python's `rhino3dm.Interval`.
///
/// Endpoints are intentionally retained verbatim: descending, degenerate,
/// non-finite, and NaN intervals are representable by the Python binding and
/// this value type does not normalize them on construction or mutation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Interval {
    pub t0: f64,
    pub t1: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub matrix: [[f64; 4]; 4],
}

/// A finite non-degenerate line segment corresponding to Python's
/// `rhino3dm.Line` value. Endpoints remain mutable; derived values are
/// calculated from their current coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Line {
    pub from: Point3d,
    pub to: Point3d,
}

/// An axis-aligned bounding box corresponding to Python's
/// `rhino3dm.BoundingBox`. Construction retains supplied endpoint order; a
/// reversed range is invalid instead of being silently normalized.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoundingBox {
    pub min: Point3d,
    pub max: Point3d,
}

/// OpenNURBS' public unset sentinel, used by several numeric Python APIs
/// instead of `NaN` when an operation has no geometrically meaningful value.
pub const UNSET_VALUE: f64 = -1.234_321_012_343_21e308;

/// The typed payload of `rhino3dm.InstanceReference`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstanceReference {
    pub definition_id: [u8; 16],
    pub transform: Transform,
}

/// A recoverable v6+ block definition prefix. Its member UUIDs identify the
/// object records owned by this reusable definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceDefinition {
    pub source: SourceRange,
    pub id: [u8; 16],
    pub index: Option<i32>,
    pub name: String,
    pub members: Vec<[u8; 16]>,
}

/// Parse outcome for one source instance-definition record.
///
/// Failed definitions are source facts: consumers can retain, report and later
/// retry them instead of misreading an incomplete definition table as empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceDefinitionRecord {
    pub source: SourceRange,
    pub definition: Option<InstanceDefinition>,
    pub error: Option<String>,
}

/// Coverage of fields in one tagged `ON_3dmObjectAttributes` payload.
///
/// `complete` in older releases meant only that the attribute stream had been
/// consumed. This contract instead describes whether every encountered field
/// was retained in the public typed representation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AttributeCoverage {
    #[default]
    Complete,
    Partial {
        /// Known field tags that were skipped rather than preserved.
        skipped_tags: Vec<u8>,
        /// First unknown tag after which the unread suffix remains opaque.
        opaque_suffix_from_tag: Option<u8>,
    },
}

impl AttributeCoverage {
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        matrix: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Equivalent to Python's `Transform.Identity()`.
    pub const fn identity() -> Self {
        Self::IDENTITY
    }

    /// Equivalent to Python's `Transform.ZeroTransformation()`.
    pub const fn zero_transformation() -> Self {
        Self::diagonal(0.0)
    }

    /// Equivalent to Python's `Transform.Unset()` sentinel matrix.
    pub const fn unset() -> Self {
        Self {
            matrix: [[f64::NEG_INFINITY; 4]; 4],
        }
    }

    /// Equivalent to Python's single-diagonal `Transform(value)` constructor.
    /// The homogeneous bottom-right entry remains one.
    pub const fn diagonal(value: f64) -> Self {
        Self {
            matrix: [
                [value, 0.0, 0.0, 0.0],
                [0.0, value, 0.0, 0.0],
                [0.0, 0.0, value, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Equivalent to `Transform.Translation(x, y, z)`.
    pub const fn translation(x: f64, y: f64, z: f64) -> Self {
        Self {
            matrix: [
                [1.0, 0.0, 0.0, x],
                [0.0, 1.0, 0.0, y],
                [0.0, 0.0, 1.0, z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Equivalent to `Transform.Translation(Vector3d)`.
    pub const fn translation_vector(vector: Vector3d) -> Self {
        Self::translation(vector.x, vector.y, vector.z)
    }

    /// Reproduce the observed Python binding overload
    /// `Transform.Translation(Vector3d)`.
    ///
    /// The pinned CPython oracle narrows the vector components through a
    /// 32-bit float before storing the double-precision transform entries.
    /// That is a binding compatibility behavior, not a useful default for
    /// new Rust code, so [`Self::translation_vector`] keeps the native f64
    /// values and this explicit adapter is used by the Python conformance
    /// surface.
    pub fn python_translation_vector(vector: Vector3d) -> Self {
        Self::translation(
            f64::from(vector.x as f32),
            f64::from(vector.y as f32),
            f64::from(vector.z as f32),
        )
    }

    /// Standard row-major matrix product, matching `Transform.Multiply(a, b)`.
    pub fn multiply(self, right: Self) -> Self {
        let mut matrix = [[0.0; 4]; 4];
        for (row, output) in matrix.iter_mut().enumerate() {
            for (column, value) in output.iter_mut().enumerate() {
                *value = (0..4)
                    .map(|index| self.matrix[row][index] * right.matrix[index][column])
                    .sum();
            }
        }
        Self { matrix }
    }

    /// Determinant of the full 4x4 transform.
    pub fn determinant(self) -> f64 {
        // The OpenNURBS binding returns zero rather than propagating NaN when
        // called on an unset/otherwise invalid matrix.
        if !self.is_valid() {
            return 0.0;
        }
        let mut matrix = self.matrix;
        let mut sign = 1.0;
        let mut determinant = 1.0;
        for pivot_column in 0..4 {
            let pivot_row = (pivot_column..4)
                .max_by(|left, right| {
                    matrix[*left][pivot_column]
                        .abs()
                        .total_cmp(&matrix[*right][pivot_column].abs())
                })
                .expect("non-empty 4x4 pivot range");
            let pivot = matrix[pivot_row][pivot_column];
            if pivot == 0.0 {
                return 0.0;
            }
            if pivot_row != pivot_column {
                matrix.swap(pivot_row, pivot_column);
                sign = -sign;
            }
            determinant *= matrix[pivot_column][pivot_column];
            let pivot_values = matrix[pivot_column];
            for row in matrix.iter_mut().skip(pivot_column + 1) {
                let factor = row[pivot_column] / pivot_values[pivot_column];
                for (value, pivot_value) in row
                    .iter_mut()
                    .skip(pivot_column + 1)
                    .zip(pivot_values.iter().skip(pivot_column + 1))
                {
                    *value -= factor * pivot_value;
                }
            }
        }
        determinant * sign
    }

    /// Invert this matrix, returning `None` when it is singular or non-finite.
    pub fn try_inverse(self) -> Option<Self> {
        if !self.is_valid() {
            return None;
        }
        let mut augmented = [[0.0; 8]; 4];
        for row in 0..4 {
            augmented[row][..4].copy_from_slice(&self.matrix[row]);
            augmented[row][row + 4] = 1.0;
        }
        for pivot_column in 0..4 {
            let pivot_row = (pivot_column..4).max_by(|left, right| {
                augmented[*left][pivot_column]
                    .abs()
                    .total_cmp(&augmented[*right][pivot_column].abs())
            })?;
            if augmented[pivot_row][pivot_column] == 0.0 {
                return None;
            }
            augmented.swap(pivot_row, pivot_column);
            let pivot = augmented[pivot_column][pivot_column];
            for value in &mut augmented[pivot_column] {
                *value /= pivot;
            }
            let pivot_values = augmented[pivot_column];
            for (row_index, row) in augmented.iter_mut().enumerate() {
                if row_index == pivot_column {
                    continue;
                }
                let factor = row[pivot_column];
                for (value, pivot_value) in row.iter_mut().zip(pivot_values) {
                    *value -= factor * pivot_value;
                }
            }
        }
        let mut matrix = [[0.0; 4]; 4];
        for row in 0..4 {
            matrix[row].copy_from_slice(&augmented[row][4..]);
        }
        Some(Self { matrix })
    }

    /// Python's observed one-return `TryGetInverse()` behavior: it supplies
    /// identity when no inverse is available. Prefer [`Self::try_inverse`] in
    /// new Rust code when singularity needs to remain explicit.
    pub fn python_try_get_inverse(self) -> Self {
        self.try_inverse().unwrap_or(Self::IDENTITY)
    }

    pub fn is_valid(self) -> bool {
        self.matrix.iter().flatten().all(|value| value.is_finite())
    }

    pub fn is_identity(self) -> bool {
        self.matrix == Self::IDENTITY.matrix
    }

    /// Exact affine predicate for preserved source coefficients.
    pub fn is_affine(self) -> bool {
        self.is_valid() && self.matrix[3] == [0.0, 0.0, 0.0, 1.0]
    }

    /// Python-compatible affine zero-transform predicate.
    pub fn is_zero(self) -> bool {
        self.matrix[..3]
            .iter()
            .all(|row| row.iter().all(|value| *value == 0.0))
    }

    /// Whether all sixteen coefficients are exactly zero.
    pub fn is_zero_4x4(self) -> bool {
        self.matrix
            .iter()
            .all(|row| row.iter().all(|value| *value == 0.0))
    }

    /// Equivalent to Python's `Transform.IsZeroTransformation` predicate.
    pub fn is_zero_transformation(self) -> bool {
        self.is_affine() && self.is_zero()
    }

    /// Equivalent to Python's `Transform.IsLinear` predicate.
    pub fn is_linear(self) -> bool {
        self.is_affine()
            && self.matrix[0][3] == 0.0
            && self.matrix[1][3] == 0.0
            && self.matrix[2][3] == 0.0
    }

    /// Whether the linear component is an orientation-preserving orthonormal
    /// transform. The small tolerance applies to computed rotations; source
    /// coefficients themselves are never snapped or rewritten.
    pub fn is_rotation(self) -> bool {
        const TOLERANCE: f64 = 1e-12;
        if !self.is_linear() {
            return false;
        }
        let rows = [
            [self.matrix[0][0], self.matrix[0][1], self.matrix[0][2]],
            [self.matrix[1][0], self.matrix[1][1], self.matrix[1][2]],
            [self.matrix[2][0], self.matrix[2][1], self.matrix[2][2]],
        ];
        let dot = |left: [f64; 3], right: [f64; 3]| {
            left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
        };
        rows.iter()
            .all(|row| (dot(*row, *row) - 1.0).abs() <= TOLERANCE)
            && (dot(rows[0], rows[1])).abs() <= TOLERANCE
            && (dot(rows[0], rows[2])).abs() <= TOLERANCE
            && (dot(rows[1], rows[2])).abs() <= TOLERANCE
            && (self.determinant() - 1.0).abs() <= TOLERANCE
    }

    /// Return the transposed matrix without modifying the source transform.
    pub fn transpose(self) -> Self {
        Self {
            matrix: std::array::from_fn(|row| {
                std::array::from_fn(|column| self.matrix[column][row])
            }),
        }
    }

    /// Build an axis-angle rotation around a point.
    ///
    /// This maps the first runtime overload of Python's
    /// `Transform.Rotation(angleRadians, rotationAxis, rotationCenter)`. A
    /// zero or non-finite axis has no defined rotation and returns `None`
    /// instead of silently manufacturing a transform.
    pub fn try_rotation_axis_angle(
        angle_radians: f64,
        rotation_axis: Vector3d,
        rotation_center: Point3d,
    ) -> Option<Self> {
        if !angle_radians.is_finite()
            || !rotation_center.x.is_finite()
            || !rotation_center.y.is_finite()
            || !rotation_center.z.is_finite()
        {
            return None;
        }
        let axis_length = rotation_axis.length();
        if !axis_length.is_finite() || axis_length == 0.0 {
            return None;
        }
        let x = rotation_axis.x / axis_length;
        let y = rotation_axis.y / axis_length;
        let z = rotation_axis.z / axis_length;
        let cosine = angle_radians.cos();
        let sine = angle_radians.sin();
        let one_minus_cosine = 1.0 - cosine;
        let linear = [
            [
                cosine + x * x * one_minus_cosine,
                x * y * one_minus_cosine - z * sine,
                x * z * one_minus_cosine + y * sine,
            ],
            [
                y * x * one_minus_cosine + z * sine,
                cosine + y * y * one_minus_cosine,
                y * z * one_minus_cosine - x * sine,
            ],
            [
                z * x * one_minus_cosine - y * sine,
                z * y * one_minus_cosine + x * sine,
                cosine + z * z * one_minus_cosine,
            ],
        ];
        let center = [rotation_center.x, rotation_center.y, rotation_center.z];
        let offset: [f64; 3] = std::array::from_fn(|row| {
            center[row]
                - linear[row][0] * center[0]
                - linear[row][1] * center[1]
                - linear[row][2] * center[2]
        });
        Some(Self {
            matrix: [
                [linear[0][0], linear[0][1], linear[0][2], offset[0]],
                [linear[1][0], linear[1][1], linear[1][2], offset[1]],
                [linear[2][0], linear[2][1], linear[2][2], offset[2]],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    pub fn to_row_major_array(self) -> [f64; 16] {
        std::array::from_fn(|index| self.matrix[index / 4][index % 4])
    }

    pub fn to_column_major_array(self) -> [f64; 16] {
        std::array::from_fn(|index| self.matrix[index % 4][index / 4])
    }

    /// Equivalent to Python's `Transform.ToFloatArray(rowDominant)` layout
    /// selection. Values stay f64 in Rust because Python exposes its result
    /// as Python floats; callers that require an f32 GPU buffer can convert
    /// explicitly at their API boundary.
    pub fn to_float_array(self, row_dominant: bool) -> [f64; 16] {
        if row_dominant {
            self.to_row_major_array()
        } else {
            self.to_column_major_array()
        }
    }

    fn apply_homogeneous(self, [x, y, z, w]: [f64; 4]) -> [f64; 4] {
        std::array::from_fn(|row| {
            self.matrix[row][0] * x
                + self.matrix[row][1] * y
                + self.matrix[row][2] * z
                + self.matrix[row][3] * w
        })
    }
}

impl Point3d {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Equivalent to Python's `Point3d.Unset` value.
    pub const fn unset() -> Self {
        Self::new(UNSET_VALUE, UNSET_VALUE, UNSET_VALUE)
    }

    /// Equivalent to Python's `Point3d.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("X".to_owned(), self.x),
            ("Y".to_owned(), self.y),
            ("Z".to_owned(), self.z),
        ])
    }

    pub fn distance_to(self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }

    /// Equivalent to Python's `Point3d + Point3d` overload.
    pub fn add_point(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    /// Equivalent to Python's `Point3d + Vector3d` overload.
    pub fn add_vector(self, vector: Vector3d) -> Self {
        Self::new(self.x + vector.x, self.y + vector.y, self.z + vector.z)
    }

    /// Equivalent to Python's scalar `Point3d * value` overload.
    pub fn scaled(self, value: f64) -> Self {
        Self::new(self.x * value, self.y * value, self.z * value)
    }

    /// Return a transformed point without mutating the source, matching the
    /// observed Python `Point3d.Transform` result behavior.
    pub fn transformed(self, transform: Transform) -> Self {
        let [x, y, z, w] = transform.apply_homogeneous([self.x, self.y, self.z, 1.0]);
        if w != 0.0 {
            Self::new(x / w, y / w, z / w)
        } else {
            Self::new(x, y, z)
        }
    }
}

impl Point3f {
    /// Equivalent to `Point3f(x, y, z)`.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Equivalent to Python's `Point3f.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f32> {
        BTreeMap::from([
            ("X".to_owned(), self.x),
            ("Y".to_owned(), self.y),
            ("Z".to_owned(), self.z),
        ])
    }

    /// Equivalent to Python's `Point3f + Point3f` overload.
    pub fn add_point(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Point4d {
    /// Equivalent to `Point4d(x, y, z, w)`.
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    /// Equivalent to Python's `Point4d.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("X".to_owned(), self.x),
            ("Y".to_owned(), self.y),
            ("Z".to_owned(), self.z),
            ("W".to_owned(), self.w),
        ])
    }
}

impl Point2d {
    /// Equivalent to `Point2d(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Equivalent to Python's `Point2d.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f64> {
        BTreeMap::from([("X".to_owned(), self.x), ("Y".to_owned(), self.y)])
    }

    /// Equivalent to Python's `Point2d.DistanceTo(other)`.
    pub fn distance_to(self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    /// Equivalent to Python's `Point2d + Point2d` overload.
    pub fn add_point(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Vector3d {
    /// Python's default `IsParallelTo` angle tolerance (one degree).
    pub const DEFAULT_ANGLE_TOLERANCE: f64 = std::f64::consts::PI / 180.0;

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Equivalent to Python's `Vector3d.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("X".to_owned(), self.x),
            ("Y".to_owned(), self.y),
            ("Z".to_owned(), self.z),
        ])
    }

    pub fn length(self) -> f64 {
        (self.x.powi(2) + self.y.powi(2) + self.z.powi(2)).sqrt()
    }

    /// Normalize in place. Zero vectors remain zero, matching the Python API's
    /// `None` return / mutation behavior for the tested inputs.
    pub fn unitize(&mut self) {
        let length = self.length();
        if length != 0.0 {
            self.x /= length;
            self.y /= length;
            self.z /= length;
        }
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Equivalent to Python's static `Vector3d.DotProduct(a, b)`.
    pub fn dot_product(left: Self, right: Self) -> f64 {
        left.dot(right)
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Equivalent to Python's static `Vector3d.CrossProduct(a, b)`.
    pub fn cross_product(left: Self, right: Self) -> Self {
        left.cross(right)
    }

    /// `1`, `-1` or `0` for parallel, anti-parallel or nonparallel vectors.
    pub fn is_parallel_to(self, other: Self) -> i32 {
        self.is_parallel_to_with_tolerance(other, Self::DEFAULT_ANGLE_TOLERANCE)
    }

    /// Equivalent to Python's `Vector3d.IsParallelTo(other, angleTolerance)`.
    pub fn is_parallel_to_with_tolerance(self, other: Self, angle_tolerance: f64) -> i32 {
        let lengths = self.length() * other.length();
        if lengths == 0.0 {
            return 0;
        }
        let angle = self.cross(other).length().atan2(self.dot(other));
        if angle <= angle_tolerance {
            1
        } else if (std::f64::consts::PI - angle).abs() <= angle_tolerance {
            -1
        } else {
            0
        }
    }

    pub fn vector_angle(self, other: Self) -> f64 {
        let lengths = self.length() * other.length();
        if lengths == 0.0 {
            return UNSET_VALUE;
        }
        self.cross(other).length().atan2(self.dot(other))
    }
}

impl Vector3f {
    /// Equivalent to `Vector3f(x, y, z)`.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Equivalent to Python's `Vector3f.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f32> {
        BTreeMap::from([
            ("X".to_owned(), self.x),
            ("Y".to_owned(), self.y),
            ("Z".to_owned(), self.z),
        ])
    }
}

impl Vector2d {
    /// Equivalent to `Vector2d(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Equivalent to Python's `Vector2d.Encode()` coordinate mapping.
    pub fn encode(self) -> BTreeMap<String, f64> {
        BTreeMap::from([("X".to_owned(), self.x), ("Y".to_owned(), self.y)])
    }
}

impl Line {
    /// Equivalent to `Line(start, end)`.
    pub const fn new(from: Point3d, to: Point3d) -> Self {
        Self { from, to }
    }

    /// Equivalent to Python's read-only `Line.Direction` property.
    pub fn direction(self) -> Vector3d {
        Vector3d::new(
            self.to.x - self.from.x,
            self.to.y - self.from.y,
            self.to.z - self.from.z,
        )
    }

    /// Equivalent to Python's read-only `Line.Length` property.
    pub fn length(self) -> f64 {
        let direction = self.direction();
        let length = direction.length();
        if length.is_finite() {
            length
        } else {
            0.0
        }
    }

    /// Equivalent to Python's read-only `Line.UnitTangent` property.
    pub fn unit_tangent(self) -> Vector3d {
        let mut direction = self.direction();
        if self.is_valid() {
            direction.unitize();
        } else {
            direction = Vector3d::default();
        }
        direction
    }

    /// Equivalent to Python's read-only `Line.IsValid` property.
    pub fn is_valid(self) -> bool {
        self.from.x.is_finite()
            && self.from.y.is_finite()
            && self.from.z.is_finite()
            && self.to.x.is_finite()
            && self.to.y.is_finite()
            && self.to.z.is_finite()
            && self.direction().length() > 0.0
    }

    /// Equivalent to Python's `Line.PointAt(t)`, including extrapolation for
    /// values outside the segment's `[0, 1]` parameter range.
    pub fn point_at(self, parameter: f64) -> Point3d {
        let direction = self.direction();
        Point3d::new(
            self.from.x + parameter * direction.x,
            self.from.y + parameter * direction.y,
            self.from.z + parameter * direction.z,
        )
    }

    /// Transform both endpoints in place. This is equivalent to Python's
    /// `Line.Transform(xform)`: invalid transforms are rejected without
    /// changing the line and yield `false`.
    pub fn transform(&mut self, transform: Transform) -> bool {
        if !transform.is_valid() {
            return false;
        }
        self.from = self.from.transformed(transform);
        self.to = self.to.transformed(transform);
        true
    }
}

impl BoundingBox {
    pub const fn new(min: Point3d, max: Point3d) -> Self {
        Self { min, max }
    }

    pub const fn from_coordinates(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self::new(
            Point3d::new(min_x, min_y, min_z),
            Point3d::new(max_x, max_y, max_z),
        )
    }

    pub fn is_valid(self) -> bool {
        [
            self.min.x, self.min.y, self.min.z, self.max.x, self.max.y, self.max.z,
        ]
        .iter()
        .all(|value| value.is_finite())
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
            && self.min.z <= self.max.z
    }

    pub fn diagonal(self) -> Vector3d {
        Vector3d::new(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.max.z - self.min.z,
        )
    }

    pub fn center(self) -> Point3d {
        Point3d::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
            (self.min.z + self.max.z) / 2.0,
        )
    }

    pub fn area(self) -> f64 {
        if !self.is_valid() {
            return 0.0;
        }
        let diagonal = self.diagonal();
        2.0 * (diagonal.x * diagonal.y + diagonal.x * diagonal.z + diagonal.y * diagonal.z)
    }

    pub fn volume(self) -> f64 {
        if !self.is_valid() {
            return 0.0;
        }
        let diagonal = self.diagonal();
        diagonal.x * diagonal.y * diagonal.z
    }

    pub fn contains(self, point: Point3d) -> bool {
        self.is_valid()
            && point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    pub fn closest_point(self, point: Point3d) -> Point3d {
        Point3d::new(
            point.x.clamp(self.min.x, self.max.x),
            point.y.clamp(self.min.y, self.max.y),
            point.z.clamp(self.min.z, self.max.z),
        )
    }

    pub fn inflate(&mut self, amounts: Vector3d) {
        self.min.x -= amounts.x;
        self.min.y -= amounts.y;
        self.min.z -= amounts.z;
        self.max.x += amounts.x;
        self.max.y += amounts.y;
        self.max.z += amounts.z;
    }

    pub fn is_degenerate(self, tolerance: f64) -> i32 {
        if !self.is_valid() {
            return 4;
        }
        let diagonal = self.diagonal();
        let threshold = tolerance.max(0.0);
        [diagonal.x, diagonal.y, diagonal.z]
            .iter()
            .filter(|value| **value <= threshold)
            .count() as i32
    }

    pub fn union(left: Self, right: Self) -> Self {
        Self::from_coordinates(
            left.min.x.min(right.min.x),
            left.min.y.min(right.min.y),
            left.min.z.min(right.min.z),
            left.max.x.max(right.max.x),
            left.max.y.max(right.max.y),
            left.max.z.max(right.max.z),
        )
    }

    pub fn transform(&mut self, transform: Transform) -> bool {
        if !transform.is_valid() {
            return false;
        }
        let corners = [
            Point3d::new(self.min.x, self.min.y, self.min.z),
            Point3d::new(self.min.x, self.min.y, self.max.z),
            Point3d::new(self.min.x, self.max.y, self.min.z),
            Point3d::new(self.min.x, self.max.y, self.max.z),
            Point3d::new(self.max.x, self.min.y, self.min.z),
            Point3d::new(self.max.x, self.min.y, self.max.z),
            Point3d::new(self.max.x, self.max.y, self.min.z),
            Point3d::new(self.max.x, self.max.y, self.max.z),
        ]
        .map(|point| point.transformed(transform));
        *self = Self::from_coordinates(
            corners
                .iter()
                .map(|point| point.x)
                .fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|point| point.y)
                .fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|point| point.z)
                .fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|point| point.x)
                .fold(f64::NEG_INFINITY, f64::max),
            corners
                .iter()
                .map(|point| point.y)
                .fold(f64::NEG_INFINITY, f64::max),
            corners
                .iter()
                .map(|point| point.z)
                .fold(f64::NEG_INFINITY, f64::max),
        );
        true
    }
}

impl Interval {
    /// Equivalent to `Interval(t0, t1)`.
    pub const fn new(t0: f64, t1: f64) -> Self {
        Self { t0, t1 }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectAttributes {
    pub source: SourceRange,
    pub id: Option<[u8; 16]>,
    pub name: Option<String>,
    pub layer_index: Option<i32>,
    pub user_strings: Vec<(String, String)>,
    pub coverage: AttributeCoverage,
    /// Backwards-compatible shorthand for `coverage.is_complete()`.
    ///
    /// This is false whenever even a known field tag was skipped.
    pub complete: bool,
}

impl ObjectAttributes {
    /// Create authoring attributes with a caller-supplied native object ID.
    pub fn with_id(id: [u8; 16]) -> Self {
        Self {
            id: Some(id),
            complete: true,
            ..Self::default()
        }
    }

    /// Return the value of one object UserString key.
    pub fn get_user_string(&self, key: &str) -> Option<&str> {
        self.user_strings
            .iter()
            .find(|(stored_key, _)| stored_key == key)
            .map(|(_, value)| value.as_str())
    }

    /// Insert or replace one object UserString.
    pub fn set_user_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        if let Some((_, stored_value)) = self
            .user_strings
            .iter_mut()
            .find(|(stored_key, _)| *stored_key == key)
        {
            *stored_value = value;
        } else {
            self.user_strings.push((key, value));
        }
    }

    /// Remove one object UserString and return whether it existed.
    pub fn delete_user_string(&mut self, key: &str) -> bool {
        let before = self.user_strings.len();
        self.user_strings
            .retain(|(stored_key, _)| stored_key != key);
        self.user_strings.len() != before
    }

    pub const fn is_complete(&self) -> bool {
        self.coverage.is_complete()
    }

    fn mark_skipped(&mut self, tag: u8) {
        self.complete = false;
        match &mut self.coverage {
            AttributeCoverage::Complete => {
                self.coverage = AttributeCoverage::Partial {
                    skipped_tags: vec![tag],
                    opaque_suffix_from_tag: None,
                };
            }
            AttributeCoverage::Partial { skipped_tags, .. } => skipped_tags.push(tag),
        }
    }

    fn mark_opaque_suffix(&mut self, tag: u8) {
        self.complete = false;
        match &mut self.coverage {
            AttributeCoverage::Complete => {
                self.coverage = AttributeCoverage::Partial {
                    skipped_tags: Vec::new(),
                    opaque_suffix_from_tag: Some(tag),
                };
            }
            AttributeCoverage::Partial {
                opaque_suffix_from_tag,
                ..
            } => {
                if opaque_suffix_from_tag.is_none() {
                    *opaque_suffix_from_tag = Some(tag);
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Truncated {
        offset: u64,
        needed: usize,
    },
    InvalidSignature,
    InvalidArchiveVersion,
    InputTooLarge {
        limit: usize,
        actual: u64,
    },
    InvalidChunkLength {
        offset: u64,
        length: i64,
    },
    CrcMismatch {
        offset: u64,
        expected: u32,
        actual: u32,
    },
    OutOfBounds {
        offset: u64,
        end: u64,
        bound: u64,
    },
    InvalidTableTerminator {
        offset: u64,
    },
    MissingEndOfFile,
    Unsupported {
        capability: &'static str,
    },
    Decode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Truncated { offset, needed } => {
                write!(f, "truncated at {offset}, need {needed} bytes")
            }
            Self::InvalidSignature => write!(f, "not a native Rhino 3DM file"),
            Self::InvalidArchiveVersion => write!(f, "3DM header has no valid archive version"),
            Self::InputTooLarge { limit, actual } => write!(
                f,
                "3DM input is {actual} bytes, exceeding the configured {limit}-byte source limit"
            ),
            Self::InvalidChunkLength { offset, length } => {
                write!(f, "invalid chunk length {length} at {offset}")
            }
            Self::CrcMismatch {
                offset,
                expected,
                actual,
            } => write!(
                f,
                "CRC32 mismatch at {offset}: stored {expected:#010x}, calculated {actual:#010x}"
            ),
            Self::OutOfBounds { offset, end, bound } => {
                write!(f, "range {offset}..{end} exceeds bound {bound}")
            }
            Self::InvalidTableTerminator { offset } => {
                write!(f, "invalid end-of-table marker at {offset}")
            }
            Self::MissingEndOfFile => write!(f, "missing end-of-file chunk"),
            Self::Unsupported { capability } => {
                write!(f, "rhino3dm-rs does not yet support {capability}")
            }
            Self::Decode(message) => write!(f, "geometry decode failed: {message}"),
        }
    }
}

impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Copy)]
struct Chunk {
    typecode: u32,
    source: SourceRange,
    body: SourceRange,
    short: bool,
    value: i64,
}

fn parse_header(bytes: &[u8]) -> Result<File3dmHeader, Error> {
    if bytes.len() < HEADER_LENGTH {
        return Err(Error::Truncated {
            offset: bytes.len() as u64,
            needed: HEADER_LENGTH - bytes.len(),
        });
    }
    if &bytes[..FILE_SIGNATURE.len()] != FILE_SIGNATURE {
        return Err(Error::InvalidSignature);
    }
    let archive_version = std::str::from_utf8(&bytes[FILE_SIGNATURE.len()..HEADER_LENGTH])
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .filter(|version| *version > 0)
        .ok_or(Error::InvalidArchiveVersion)?;
    Ok(File3dmHeader { archive_version })
}

fn read_header(input: &mut impl Read) -> Result<File3dmHeader, Error> {
    let mut bytes = [0_u8; HEADER_LENGTH];
    let mut read = 0;
    while read < bytes.len() {
        let count = input.read(&mut bytes[read..])?;
        if count == 0 {
            return Err(Error::Truncated {
                offset: read as u64,
                needed: bytes.len() - read,
            });
        }
        read += count;
    }
    parse_header(&bytes)
}

fn scan_archive(bytes: &[u8], archive_version: u32) -> Result<ArchiveIndex, Error> {
    let mut offset = HEADER_LENGTH;
    let comment = chunk_at(bytes, offset, bytes.len(), archive_version)?;
    if comment.typecode != 1 || comment.short {
        return Err(Error::Unsupported {
            capability: "a non-standard 3DM start section",
        });
    }
    offset = end(comment.source)? as usize;
    let mut tables = Vec::new();
    let mut objects = Vec::new();
    let mut instance_definition_records = Vec::new();
    let mut instance_definitions = Vec::new();
    while offset < bytes.len() {
        let chunk = chunk_at(bytes, offset, bytes.len(), archive_version)?;
        if without_crc(chunk.typecode) == TCODE_END_OF_FILE {
            return Ok(ArchiveIndex {
                tables,
                objects,
                instance_definition_records,
                instance_definitions,
                end_of_file: chunk.source,
            });
        }
        if chunk.short || chunk.typecode & 0x1000_0000 == 0 {
            return Err(Error::Unsupported {
                capability: "a non-table top-level 3DM chunk",
            });
        }
        let (table, mut table_objects, mut table_definition_records) =
            scan_table(bytes, chunk, archive_version)?;
        offset = end(chunk.source)? as usize;
        tables.push(table);
        objects.append(&mut table_objects);
        instance_definitions.extend(
            table_definition_records
                .iter()
                .filter_map(|record| record.definition.clone()),
        );
        instance_definition_records.append(&mut table_definition_records);
    }
    Err(Error::MissingEndOfFile)
}

fn scan_table(
    bytes: &[u8],
    table: Chunk,
    archive_version: u32,
) -> Result<
    (
        ArchiveTable,
        Vec<ObjectRecord>,
        Vec<InstanceDefinitionRecord>,
    ),
    Error,
> {
    let mut records = Vec::new();
    let mut objects = Vec::new();
    let mut instance_definition_records = Vec::new();
    let mut offset = table.body.offset as usize;
    let table_end = end(table.body)? as usize;
    while offset < table_end {
        let child = chunk_at(bytes, offset, table_end, archive_version)?;
        if child.typecode == TCODE_END_OF_TABLE {
            if !child.short || child.value != 0 || end(child.source)? as usize != table_end {
                return Err(Error::InvalidTableTerminator {
                    offset: child.source.offset,
                });
            }
            return Ok((
                ArchiveTable {
                    typecode: table.typecode,
                    source: table.source,
                    records,
                },
                objects,
                instance_definition_records,
            ));
        }
        if without_crc(table.typecode) == TCODE_OBJECTS && child.typecode == TCODE_OBJECT_RECORD {
            objects.push(parse_object_record(bytes, child, archive_version));
        }
        if without_crc(table.typecode) == TCODE_INSTANCE_DEFINITIONS
            && child.typecode == TCODE_INSTANCE_DEFINITION_RECORD
        {
            let definition = match parse_instance_definition(bytes, child, archive_version) {
                Ok(definition) => InstanceDefinitionRecord {
                    source: child.source,
                    definition: Some(definition),
                    error: None,
                },
                Err(error) => InstanceDefinitionRecord {
                    source: child.source,
                    definition: None,
                    error: Some(error.to_string()),
                },
            };
            instance_definition_records.push(definition);
        }
        records.push(ArchiveRecord {
            typecode: child.typecode,
            source: child.source,
        });
        offset = end(child.source)? as usize;
    }
    Err(Error::InvalidTableTerminator {
        offset: table.body.offset,
    })
}

fn parse_object_record(bytes: &[u8], record: Chunk, archive_version: u32) -> ObjectRecord {
    let fallback = || ObjectRecord {
        source: record.source,
        object_type: None,
        class_id: None,
        class_data: None,
        attributes: None,
        attribute_error: None,
        attribute_userdata_error: None,
        geometry_kind: GeometryKind::Other,
        point: None,
        instance_reference: None,
        geometry_error: None,
        framing_error: None,
    };
    let result = (|| -> Result<ObjectRecord, Error> {
        if record.short || record.typecode != TCODE_OBJECT_RECORD {
            return Err(Error::Unsupported {
                capability: "a non-standard object record",
            });
        }
        let record_end = end(record.body)? as usize;
        let type_chunk = chunk_at(
            bytes,
            record.body.offset as usize,
            record_end,
            archive_version,
        )?;
        if !type_chunk.short || type_chunk.typecode != OBJECT_RECORD_TYPE || type_chunk.value < 0 {
            return Err(Error::Unsupported {
                capability: "an object record without a standard type child",
            });
        }
        let object_type = u32::try_from(type_chunk.value).map_err(|_| Error::Unsupported {
            capability: "an object type outside the u32 range",
        })?;
        let class = chunk_at(
            bytes,
            end(type_chunk.source)? as usize,
            record_end,
            archive_version,
        )?;
        if class.short || class.typecode != OPENNURBS_CLASS {
            return Err(Error::Unsupported {
                capability: "an object record without an OpenNURBS class wrapper",
            });
        }
        let class_end = end(class.body)? as usize;
        let uuid_chunk = chunk_at_with_class_crc(
            bytes,
            class.body.offset as usize,
            class_end,
            archive_version,
            true,
        )?;
        if uuid_chunk.short || uuid_chunk.typecode != CLASS_UUID || uuid_chunk.body.length != 16 {
            return Err(Error::Unsupported {
                capability: "an OpenNURBS class without a standard UUID",
            });
        }
        let mut class_id = [0_u8; 16];
        let uuid_start = uuid_chunk.body.offset as usize;
        class_id.copy_from_slice(&bytes[uuid_start..uuid_start + 16]);
        let data_chunk = chunk_at(
            bytes,
            end(uuid_chunk.source)? as usize,
            class_end,
            archive_version,
        )?;
        if data_chunk.short || data_chunk.typecode != CLASS_DATA {
            return Err(Error::Unsupported {
                capability: "an OpenNURBS class without a standard data child",
            });
        }
        let mut class_offset = end(data_chunk.source)? as usize;
        loop {
            let class_child = chunk_at(bytes, class_offset, class_end, archive_version)?;
            if class_child.typecode == CLASS_END {
                if !class_child.short
                    || class_child.value != 0
                    || end(class_child.source)? as usize != class_end
                {
                    return Err(Error::Unsupported {
                        capability: "an OpenNURBS class without a standard terminator",
                    });
                }
                break;
            }
            if class_child.typecode != CLASS_USERDATA || class_child.short {
                return Err(Error::Unsupported {
                    capability: "an unknown OpenNURBS class trailer",
                });
            }
            class_offset = end(class_child.source)? as usize;
        }
        let mut attributes_range = None;
        let mut attribute_userdata_range = None;
        let mut offset = end(class.source)? as usize;
        while offset < record_end {
            let child = chunk_at(bytes, offset, record_end, archive_version)?;
            if child.typecode == OBJECT_RECORD_END {
                if !child.short || child.value != 0 || end(child.source)? as usize != record_end {
                    return Err(Error::Unsupported {
                        capability: "an object record without a final standard terminator",
                    });
                }
                let (mut attributes, attribute_error) = match attributes_range {
                    Some(range) => match parse_attributes(bytes, range) {
                        Ok(attributes) => (Some(attributes), None),
                        Err(error) => (None, Some(error.to_string())),
                    },
                    None => (None, None),
                };
                let attribute_userdata_error = match (attributes.as_mut(), attribute_userdata_range)
                {
                    (Some(attributes), Some(range)) => {
                        match parse_user_strings(bytes, range, archive_version) {
                            Ok(user_strings) => {
                                attributes.user_strings = user_strings;
                                None
                            }
                            Err(error) => Some(error.to_string()),
                        }
                    }
                    _ => None,
                };
                let geometry_kind = classify_geometry(class_id);
                let (point, instance_reference, geometry_error) = match geometry_kind {
                    GeometryKind::Point => match parse_point(bytes, data_chunk.body) {
                        Ok(point) => (Some(point), None, None),
                        Err(error) => (None, None, Some(error.to_string())),
                    },
                    GeometryKind::InstanceReference => {
                        match parse_instance_reference(bytes, data_chunk.body) {
                            Ok(reference) => (None, Some(reference), None),
                            Err(error) => (None, None, Some(error.to_string())),
                        }
                    }
                    _ => (None, None, None),
                };
                return Ok(ObjectRecord {
                    source: record.source,
                    object_type: Some(object_type),
                    class_id: Some(class_id),
                    class_data: Some(data_chunk.body),
                    attributes,
                    attribute_error,
                    attribute_userdata_error,
                    geometry_kind,
                    point,
                    instance_reference,
                    geometry_error,
                    framing_error: None,
                });
            }
            if child.typecode == OBJECT_RECORD_ATTRIBUTES
                && attributes_range.is_none()
                && !child.short
            {
                attributes_range = Some(child.body);
            }
            if child.typecode == OBJECT_RECORD_ATTRIBUTES_USERDATA
                && attribute_userdata_range.is_none()
                && !child.short
            {
                attribute_userdata_range = Some(child.body);
            }
            offset = end(child.source)? as usize;
        }
        Err(Error::Unsupported {
            capability: "an object record missing its terminator",
        })
    })();
    match result {
        Ok(object) => object,
        Err(error) => {
            let mut object = fallback();
            object.framing_error = Some(error.to_string());
            object
        }
    }
}

fn classify_geometry(class_id: [u8; 16]) -> GeometryKind {
    match class_id {
        POINT_CLASS => GeometryKind::Point,
        INSTANCE_REFERENCE_CLASS => GeometryKind::InstanceReference,
        MESH_CLASS => GeometryKind::Mesh,
        BREP_CLASS => GeometryKind::Brep,
        EXTRUSION_CLASS => GeometryKind::Extrusion,
        _ => GeometryKind::Other,
    }
}

fn parse_point(bytes: &[u8], source: SourceRange) -> Result<Point3d, Error> {
    let mut offset = source.offset as usize;
    let source_end = end(source)? as usize;
    let version = take_u8(bytes, &mut offset, source_end)?;
    if version >> 4 != 1 {
        return Err(Error::Unsupported {
            capability: "a non-v1 ON_Point payload",
        });
    }
    let x = read_f64(bytes, &mut offset, source_end)?;
    let y = read_f64(bytes, &mut offset, source_end)?;
    let z = read_f64(bytes, &mut offset, source_end)?;
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return Err(Error::Unsupported {
            capability: "a point with non-finite coordinates",
        });
    }
    Ok(Point3d { x, y, z })
}

fn parse_instance_reference(bytes: &[u8], source: SourceRange) -> Result<InstanceReference, Error> {
    let mut offset = source.offset as usize;
    let source_end = end(source)? as usize;
    let version = take_u8(bytes, &mut offset, source_end)?;
    if version >> 4 != 1 {
        return Err(Error::Unsupported {
            capability: "a non-v1 instance-reference payload",
        });
    }
    let definition_id = read_uuid(bytes, &mut offset, source_end)?;
    let mut matrix = [[0.0; 4]; 4];
    for row in &mut matrix {
        for value in row {
            *value = read_f64(bytes, &mut offset, source_end)?;
        }
    }
    if !matrix.iter().flatten().all(|value| value.is_finite()) {
        return Err(Error::Unsupported {
            capability: "an instance-reference transform with non-finite values",
        });
    }
    Ok(InstanceReference {
        definition_id,
        transform: Transform { matrix },
    })
}

fn parse_instance_definition(
    bytes: &[u8],
    record: Chunk,
    archive_version: u32,
) -> Result<InstanceDefinition, Error> {
    let (class_id, data) = class_data(bytes, record.body, archive_version)?;
    if class_id != INSTANCE_DEFINITION_CLASS {
        return Err(Error::Unsupported {
            capability: "an instance-definition record with an unknown class",
        });
    }
    let outer = chunk_at(
        bytes,
        data.offset as usize,
        end(data)? as usize,
        archive_version,
    )?;
    if outer.typecode != ANONYMOUS || outer.short {
        return Err(Error::Unsupported {
            capability: "a non-v6 instance-definition payload",
        });
    }
    let mut offset = outer.body.offset as usize;
    let outer_end = end(outer.body)? as usize;
    if read_i32(bytes, &mut offset, outer_end)? != 1 || read_i32(bytes, &mut offset, outer_end)? < 0
    {
        return Err(Error::Unsupported {
            capability: "an unsupported instance-definition version",
        });
    }
    let (index, id, name) = parse_model_component(bytes, &mut offset, outer_end, archive_version)?;
    if id == [0; 16] {
        return Err(Error::Unsupported {
            capability: "an instance definition with a nil UUID",
        });
    }
    let _kind = read_u32(bytes, &mut offset, outer_end)?;
    let units = chunk_at(bytes, offset, outer_end, archive_version)?;
    if units.typecode != ANONYMOUS || units.short {
        return Err(Error::Unsupported {
            capability: "an instance definition without unit detail",
        });
    }
    offset = end(units.source)? as usize;
    let _description = read_utf16(bytes, &mut offset, outer_end)?;
    let _url = read_utf16(bytes, &mut offset, outer_end)?;
    let _url_tag = read_utf16(bytes, &mut offset, outer_end)?;
    skip(bytes, &mut offset, outer_end, 6 * 8)?;
    let has_members = take_u8(bytes, &mut offset, outer_end)?;
    if has_members > 1 {
        return Err(Error::Unsupported {
            capability: "an instance definition with an invalid member flag",
        });
    }
    let mut members = Vec::new();
    if has_members == 1 {
        let raw_count = read_i32(bytes, &mut offset, outer_end)?;
        let count = usize::try_from(raw_count).map_err(|_| Error::InvalidChunkLength {
            offset: offset as u64,
            length: i64::from(raw_count),
        })?;
        let bytes_len = count.checked_mul(16).ok_or(Error::OutOfBounds {
            offset: offset as u64,
            end: u64::MAX,
            bound: outer_end as u64,
        })?;
        if offset
            .checked_add(bytes_len)
            .is_none_or(|end| end > outer_end)
        {
            return Err(Error::Truncated {
                offset: offset as u64,
                needed: bytes_len,
            });
        }
        members.reserve(count);
        for _ in 0..count {
            members.push(read_uuid(bytes, &mut offset, outer_end)?);
        }
    }
    Ok(InstanceDefinition {
        source: record.source,
        id,
        index,
        name,
        members,
    })
}

fn class_data(
    bytes: &[u8],
    source: SourceRange,
    archive_version: u32,
) -> Result<([u8; 16], SourceRange), Error> {
    let wrapper = chunk_at(
        bytes,
        source.offset as usize,
        end(source)? as usize,
        archive_version,
    )?;
    if wrapper.typecode != OPENNURBS_CLASS || wrapper.short {
        return Err(Error::Unsupported {
            capability: "a record without an OpenNURBS class wrapper",
        });
    }
    let mut offset = wrapper.body.offset as usize;
    let wrapper_end = end(wrapper.body)? as usize;
    let uuid = chunk_at_with_class_crc(bytes, offset, wrapper_end, archive_version, true)?;
    if uuid.typecode != CLASS_UUID || uuid.short || uuid.body.length != 16 {
        return Err(Error::Unsupported {
            capability: "a class wrapper without a UUID",
        });
    }
    let mut uuid_offset = uuid.body.offset as usize;
    let class_id = read_uuid(bytes, &mut uuid_offset, end(uuid.body)? as usize)?;
    offset = end(uuid.source)? as usize;
    let data = chunk_at(bytes, offset, wrapper_end, archive_version)?;
    if data.typecode != CLASS_DATA || data.short {
        return Err(Error::Unsupported {
            capability: "a class wrapper without class data",
        });
    }
    Ok((class_id, data.body))
}

fn parse_model_component(
    bytes: &[u8],
    offset: &mut usize,
    bound: usize,
    archive_version: u32,
) -> Result<(Option<i32>, [u8; 16], String), Error> {
    const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
    let chunk = chunk_at(bytes, *offset, bound, archive_version)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short {
        return Err(Error::Unsupported {
            capability: "an instance definition without model-component attributes",
        });
    }
    let mut payload_offset = chunk.body.offset as usize;
    let payload_end = end(chunk.body)? as usize;
    if read_i32(bytes, &mut payload_offset, payload_end)? != 1
        || read_i32(bytes, &mut payload_offset, payload_end)? < 0
    {
        return Err(Error::Unsupported {
            capability: "an unsupported model-component version",
        });
    }
    match take_u8(bytes, &mut payload_offset, payload_end)? {
        0 | 2 => {}
        1 => skip(bytes, &mut payload_offset, payload_end, 12)?,
        _ => {
            return Err(Error::Unsupported {
                capability: "an invalid model serial status",
            })
        }
    }
    let id = match take_u8(bytes, &mut payload_offset, payload_end)? {
        0 | 2 => [0; 16],
        1 => read_uuid(bytes, &mut payload_offset, payload_end)?,
        _ => {
            return Err(Error::Unsupported {
                capability: "an invalid model UUID status",
            })
        }
    };
    match take_u8(bytes, &mut payload_offset, payload_end)? {
        0 | 2 => {}
        1 => skip(bytes, &mut payload_offset, payload_end, 4)?,
        _ => {
            return Err(Error::Unsupported {
                capability: "an invalid component type status",
            })
        }
    }
    let index = match take_u8(bytes, &mut payload_offset, payload_end)? {
        0 | 2 => None,
        1 => Some(read_i32(bytes, &mut payload_offset, payload_end)?),
        _ => {
            return Err(Error::Unsupported {
                capability: "an invalid component index status",
            })
        }
    };
    let name = match take_u8(bytes, &mut payload_offset, payload_end)? {
        0 | 2 => String::new(),
        1 => read_utf16(bytes, &mut payload_offset, payload_end)?,
        _ => {
            return Err(Error::Unsupported {
                capability: "an invalid component name status",
            })
        }
    };
    *offset = end(chunk.source)? as usize;
    Ok((index, id, name))
}

/// Decode the stable prefix of modern tagged `ON_3dmObjectAttributes`.
///
/// Tags that require their own nested class decoder intentionally stop parsing
/// at the containing boundary. The already decoded identity fields remain
/// valid and [`AttributeCoverage`] describes skipped or opaque data.
fn parse_attributes(bytes: &[u8], source: SourceRange) -> Result<ObjectAttributes, Error> {
    let mut offset = source.offset as usize;
    let end = end(source)? as usize;
    let packed_version = take_u8(bytes, &mut offset, end)?;
    if packed_version >> 4 != 2 {
        return Err(Error::Unsupported {
            capability: "legacy fixed object attributes",
        });
    }
    let mut id = [0_u8; 16];
    id.copy_from_slice(take(bytes, &mut offset, end, 16)?);
    let layer_index = read_i32(bytes, &mut offset, end)?;
    let mut attributes = ObjectAttributes {
        source,
        id: Some(id),
        name: None,
        layer_index: Some(layer_index),
        user_strings: Vec::new(),
        coverage: AttributeCoverage::Complete,
        complete: true,
    };
    while offset < end {
        let tag = take_u8(bytes, &mut offset, end)?;
        match tag {
            0 => return Ok(attributes),
            1 => attributes.name = Some(read_utf16(bytes, &mut offset, end)?),
            2 => {
                let _url = read_utf16(bytes, &mut offset, end)?;
                attributes.mark_skipped(tag);
            }
            3 | 4 | 10 | 22 => {
                skip(bytes, &mut offset, end, 4)?;
                attributes.mark_skipped(tag);
            }
            6 | 7 => {
                skip(bytes, &mut offset, end, 4)?;
                attributes.mark_skipped(tag);
            }
            20 => {
                skip(bytes, &mut offset, end, 16)?;
                attributes.mark_skipped(tag);
            }
            8 => {
                skip(bytes, &mut offset, end, 8)?;
                attributes.mark_skipped(tag);
            }
            9 | 11..=17 | 19 | 23..=27 => {
                skip(bytes, &mut offset, end, 1)?;
                attributes.mark_skipped(tag);
            }
            18 => {
                let count = read_i32(bytes, &mut offset, end)?;
                skip_count(bytes, &mut offset, end, count, 4)?;
                attributes.mark_skipped(tag);
            }
            21 => {
                let count = read_i32(bytes, &mut offset, end)?;
                skip_count(bytes, &mut offset, end, count, 32)?;
                attributes.mark_skipped(tag);
            }
            _ => {
                attributes.mark_opaque_suffix(tag);
                return Ok(attributes);
            }
        }
    }
    Err(Error::Truncated {
        offset: end as u64,
        needed: 1,
    })
}

/// Extract the built-in `ON_UserStringList` class userdata carrier.
fn parse_user_strings(
    bytes: &[u8],
    source: SourceRange,
    archive_version: u32,
) -> Result<Vec<(String, String)>, Error> {
    let mut offset = source.offset as usize;
    let source_end = end(source)? as usize;
    let mut values = Vec::new();
    while offset < source_end {
        let wrapper = chunk_at(bytes, offset, source_end, archive_version)?;
        if wrapper.typecode == CLASS_END {
            break;
        }
        if wrapper.typecode != CLASS_USERDATA || wrapper.short {
            return Err(Error::Unsupported {
                capability: "an unknown object-attribute userdata carrier",
            });
        }
        if let Some(payload) = userdata_payload(bytes, wrapper, archive_version)? {
            values.extend(parse_user_string_list(bytes, payload, archive_version)?);
        }
        offset = end(wrapper.source)? as usize;
    }
    Ok(values)
}

fn userdata_payload(
    bytes: &[u8],
    wrapper: Chunk,
    archive_version: u32,
) -> Result<Option<SourceRange>, Error> {
    let mut offset = wrapper.body.offset as usize;
    let wrapper_end = end(wrapper.body)? as usize;
    let version = take_u8(bytes, &mut offset, wrapper_end)?;
    let (class_id, item_id, payload_offset) = if version >> 4 == 1 {
        let class_id = read_uuid(bytes, &mut offset, wrapper_end)?;
        let item_id = read_uuid(bytes, &mut offset, wrapper_end)?;
        skip(bytes, &mut offset, wrapper_end, 4 + 16 * 8)?;
        (class_id, item_id, offset)
    } else if version >> 4 == 2 {
        let header = chunk_at(bytes, offset, wrapper_end, archive_version)?;
        if header.typecode != CLASS_USERDATA_HEADER || header.short {
            return Err(Error::Unsupported {
                capability: "a class-userdata record without its header",
            });
        }
        let mut header_offset = header.body.offset as usize;
        let header_end = end(header.body)? as usize;
        let class_id = read_uuid(bytes, &mut header_offset, header_end)?;
        let item_id = read_uuid(bytes, &mut header_offset, header_end)?;
        (class_id, item_id, end(header.source)? as usize)
    } else {
        return Ok(None);
    };
    if class_id != USER_STRING_LIST_WIRE || item_id != USER_STRING_LIST_WIRE {
        return Ok(None);
    }
    let payload = chunk_at(bytes, payload_offset, wrapper_end, archive_version)?;
    if payload.typecode != ANONYMOUS || payload.short {
        return Err(Error::Unsupported {
            capability: "a user-string carrier without anonymous payload",
        });
    }
    Ok(Some(payload.body))
}

fn parse_user_string_list(
    bytes: &[u8],
    payload: SourceRange,
    archive_version: u32,
) -> Result<Vec<(String, String)>, Error> {
    let outer = chunk_at(
        bytes,
        payload.offset as usize,
        end(payload)? as usize,
        archive_version,
    )?;
    if outer.typecode != ANONYMOUS || outer.short {
        return Err(Error::Unsupported {
            capability: "a user-string list without anonymous framing",
        });
    }
    let mut offset = outer.body.offset as usize;
    let outer_end = end(outer.body)? as usize;
    if read_i32(bytes, &mut offset, outer_end)? != 1 {
        return Err(Error::Unsupported {
            capability: "a non-v1 ON_UserStringList",
        });
    }
    let _minor = read_i32(bytes, &mut offset, outer_end)?;
    let raw_count = read_i32(bytes, &mut offset, outer_end)?;
    let count = usize::try_from(raw_count).map_err(|_| Error::InvalidChunkLength {
        offset: offset as u64,
        length: i64::from(raw_count),
    })?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = chunk_at(bytes, offset, outer_end, archive_version)?;
        if entry.typecode != ANONYMOUS || entry.short {
            return Err(Error::Unsupported {
                capability: "a non-anonymous user-string entry",
            });
        }
        let mut entry_offset = entry.body.offset as usize;
        let entry_end = end(entry.body)? as usize;
        if read_i32(bytes, &mut entry_offset, entry_end)? != 1 {
            return Err(Error::Unsupported {
                capability: "a non-v1 user-string entry",
            });
        }
        let _minor = read_i32(bytes, &mut entry_offset, entry_end)?;
        let key = read_utf16(bytes, &mut entry_offset, entry_end)?;
        let value = read_utf16(bytes, &mut entry_offset, entry_end)?;
        values.push((key, value));
        offset = end(entry.source)? as usize;
    }
    Ok(values)
}

fn read_uuid(bytes: &[u8], offset: &mut usize, end: usize) -> Result<[u8; 16], Error> {
    let mut value = [0_u8; 16];
    value.copy_from_slice(take(bytes, offset, end, 16)?);
    Ok(value)
}

fn take<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    end: usize,
    len: usize,
) -> Result<&'a [u8], Error> {
    let next = offset.checked_add(len).ok_or(Error::OutOfBounds {
        offset: *offset as u64,
        end: u64::MAX,
        bound: end as u64,
    })?;
    if next > end {
        return Err(Error::Truncated {
            offset: *offset as u64,
            needed: len,
        });
    }
    let result = &bytes[*offset..next];
    *offset = next;
    Ok(result)
}

fn skip(bytes: &[u8], offset: &mut usize, end: usize, len: usize) -> Result<(), Error> {
    let _ = take(bytes, offset, end, len)?;
    Ok(())
}

fn take_u8(bytes: &[u8], offset: &mut usize, end: usize) -> Result<u8, Error> {
    Ok(take(bytes, offset, end, 1)?[0])
}

fn read_i32(bytes: &[u8], offset: &mut usize, end: usize) -> Result<i32, Error> {
    Ok(i32::from_le_bytes(
        take(bytes, offset, end, 4)?.try_into().unwrap(),
    ))
}

fn read_u32(bytes: &[u8], offset: &mut usize, end: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        take(bytes, offset, end, 4)?.try_into().unwrap(),
    ))
}

fn read_f64(bytes: &[u8], offset: &mut usize, end: usize) -> Result<f64, Error> {
    Ok(f64::from_le_bytes(
        take(bytes, offset, end, 8)?.try_into().unwrap(),
    ))
}

fn skip_count(
    bytes: &[u8],
    offset: &mut usize,
    end: usize,
    raw_count: i32,
    width: usize,
) -> Result<(), Error> {
    let count = usize::try_from(raw_count).map_err(|_| Error::InvalidChunkLength {
        offset: *offset as u64,
        length: i64::from(raw_count),
    })?;
    let bytes_len = count.checked_mul(width).ok_or(Error::OutOfBounds {
        offset: *offset as u64,
        end: u64::MAX,
        bound: end as u64,
    })?;
    skip(bytes, offset, end, bytes_len)
}

fn read_utf16(bytes: &[u8], offset: &mut usize, end: usize) -> Result<String, Error> {
    let count = u32::from_le_bytes(take(bytes, offset, end, 4)?.try_into().unwrap()) as usize;
    let raw = take(
        bytes,
        offset,
        end,
        count.checked_mul(2).ok_or(Error::OutOfBounds {
            offset: *offset as u64,
            end: u64::MAX,
            bound: end as u64,
        })?,
    )?;
    if count == 0 {
        return Ok(String::new());
    }
    let words: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| u16::from_le_bytes(*word))
        .collect();
    if words.last() != Some(&0) {
        return Err(Error::Unsupported {
            capability: "a UTF-16 string without a NUL terminator",
        });
    }
    String::from_utf16(&words[..words.len() - 1]).map_err(|_| Error::Unsupported {
        capability: "an invalid UTF-16 string",
    })
}

fn chunk_at(
    bytes: &[u8],
    offset: usize,
    bound: usize,
    archive_version: u32,
) -> Result<Chunk, Error> {
    chunk_at_with_class_crc(bytes, offset, bound, archive_version, false)
}

fn chunk_at_with_class_crc(
    bytes: &[u8],
    offset: usize,
    bound: usize,
    archive_version: u32,
    class_uuid: bool,
) -> Result<Chunk, Error> {
    let width = if archive_version >= 50 { 8 } else { 4 };
    let header_end = offset.checked_add(4 + width).ok_or(Error::OutOfBounds {
        offset: offset as u64,
        end: u64::MAX,
        bound: bound as u64,
    })?;
    if header_end > bound {
        return Err(Error::Truncated {
            offset: offset as u64,
            needed: 4 + width,
        });
    }
    let typecode = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let short = typecode & TCODE_SHORT != 0;
    let value = if width == 8 {
        i64::from_le_bytes(bytes[offset + 4..header_end].try_into().unwrap())
    } else if short {
        i64::from(i32::from_le_bytes(
            bytes[offset + 4..header_end].try_into().unwrap(),
        ))
    } else {
        i64::from(u32::from_le_bytes(
            bytes[offset + 4..header_end].try_into().unwrap(),
        ))
    };
    let body_offset = header_end as u64;
    if short {
        return Ok(Chunk {
            typecode,
            source: SourceRange {
                offset: offset as u64,
                length: (4 + width) as u64,
            },
            body: SourceRange {
                offset: body_offset,
                length: 0,
            },
            short,
            value,
        });
    }
    if value < 0 {
        return Err(Error::InvalidChunkLength {
            offset: offset as u64,
            length: value,
        });
    }
    let length = usize::try_from(value).map_err(|_| Error::InvalidChunkLength {
        offset: offset as u64,
        length: value,
    })?;
    let declared_end = header_end.checked_add(length).ok_or(Error::OutOfBounds {
        offset: offset as u64,
        end: u64::MAX,
        bound: bound as u64,
    })?;
    if declared_end > bound {
        return Err(Error::OutOfBounds {
            offset: offset as u64,
            end: declared_end as u64,
            bound: bound as u64,
        });
    }
    let checksum = if archive_version >= 2 && (typecode & TCODE_CRC != 0 || class_uuid) {
        4
    } else {
        0
    };
    if length < checksum {
        return Err(Error::InvalidChunkLength {
            offset: offset as u64,
            length: value,
        });
    }
    // Class UUID chunks contain a direct 16-byte payload, so their checksum
    // scope is unambiguous. Container checksums deliberately exclude complete
    // nested chunks; validate those only in their owning parsers once the
    // direct child ranges are known. Treating every CRC chunk as a flat body
    // corrupts valid object/table checksums.
    if checksum != 0 && class_uuid {
        let payload_end = declared_end - checksum;
        let expected = u32::from_le_bytes(bytes[payload_end..declared_end].try_into().unwrap());
        let actual = crc32fast::hash(&bytes[header_end..payload_end]);
        if expected != actual {
            return Err(Error::CrcMismatch {
                offset: payload_end as u64,
                expected,
                actual,
            });
        }
    }
    Ok(Chunk {
        typecode,
        source: SourceRange {
            offset: offset as u64,
            length: (4 + width + length) as u64,
        },
        body: SourceRange {
            offset: body_offset,
            length: (length - checksum) as u64,
        },
        short,
        value,
    })
}

fn without_crc(typecode: u32) -> u32 {
    typecode & !TCODE_CRC
}

fn decode_progress(warnings: &[String]) -> (Option<usize>, Option<usize>) {
    let Some(message) = warnings
        .iter()
        .find(|message| message.starts_with("Info: decoded "))
    else {
        return (None, None);
    };
    let Some(counts) = message
        .strip_prefix("Info: decoded ")
        .and_then(|value| value.strip_suffix(" Rhino object records"))
    else {
        return (None, None);
    };
    let Some((decoded, source)) = counts.split_once('/') else {
        return (None, None);
    };
    (decoded.trim().parse().ok(), source.trim().parse().ok())
}

fn decode_layers(bytes: &[u8]) -> Result<Vec<Layer>, Error> {
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .map_err(|error| Error::Decode(error.to_string()))?;
    let Some(namespace) = decoded.ir().native.namespace("rhino") else {
        return Ok(Vec::new());
    };
    let Some(records) = namespace.arenas.get("layers") else {
        return Ok(Vec::new());
    };
    records
        .iter()
        .map(|record| {
            let fields = record.fields();
            let id = fields
                .get("source_uuid")
                .and_then(Value::as_str)
                .and_then(parse_uuid_bytes)
                .ok_or_else(|| Error::Decode("Rhino layer record has no valid UUID".into()))?;
            let name = fields
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let index = fields
                .get("archive_index")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or(-1);
            let parent_layer_id = fields
                .get("parent_uuid")
                .and_then(Value::as_str)
                .and_then(parse_uuid_bytes);
            let visible = fields
                .get("visible")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let locked = fields
                .get("locked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(Layer {
                name,
                index,
                parent_layer_id,
                id,
                visible,
                locked,
            })
        })
        .collect()
}

fn parse_uuid_bytes(value: &str) -> Option<[u8; 16]> {
    let hex = value.replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut result = [0_u8; 16];
    for (index, slot) in result.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(result)
}

fn end(range: SourceRange) -> Result<u64, Error> {
    range
        .offset
        .checked_add(range.length)
        .ok_or(Error::OutOfBounds {
            offset: range.offset,
            end: u64::MAX,
            bound: u64::MAX,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_assigns_stable_layer_indices_without_touching_archive_state() {
        let mut document = File3dm::new();
        let first = document.add_layer(Layer::new("Base", [1; 16]));
        let second = document.add_layer(Layer::new("Tools", [2; 16]));

        assert_eq!((first, second), (0, 1));
        assert_eq!(document.layers()[0].name, "Base");
        assert_eq!(document.layers()[1].index, 1);
        assert!(document.archive().tables.is_empty());
        assert!(document.source().is_empty());

        let object_index =
            document.add_point(Point3d::new(1.25, 2.5, 3.75), ObjectAttributes::default());
        assert_eq!(object_index, 0);
        assert_eq!(
            document.objects()[0].geometry,
            Point3d::new(1.25, 2.5, 3.75)
        );
        assert_eq!(
            document.find_layer([1; 16]).map(|layer| layer.index),
            Some(0)
        );
        document.find_layer_mut([2; 16]).expect("second layer").name = "Fixtures".into();
        assert_eq!(document.layers()[1].name, "Fixtures");
    }

    #[test]
    fn object_attributes_preserve_python_style_user_string_mutation_order() {
        let mut attributes = ObjectAttributes::with_id([7; 16]);
        attributes.set_user_string("purpose", "calibration");
        attributes.set_user_string("owner", "robotics");
        attributes.set_user_string("purpose", "validation");
        assert_eq!(attributes.get_user_string("purpose"), Some("validation"));
        assert!(attributes.delete_user_string("owner"));
        assert!(!attributes.delete_user_string("missing"));
        assert_eq!(
            attributes.user_strings,
            vec![("purpose".into(), "validation".into())]
        );
    }

    #[test]
    fn point_object_collection_supports_indexed_edit_and_delete() {
        let mut document = File3dm::new();
        document.add_point(Point3d::new(1.0, 2.0, 3.0), ObjectAttributes::default());
        document.add_point(Point3d::new(4.0, 5.0, 6.0), ObjectAttributes::default());
        assert_eq!(document.object_count(), 2);
        document.object_mut(1).expect("second point").geometry.x = 9.0;
        assert_eq!(document.object(1).expect("second point").geometry.x, 9.0);
        assert!(document.delete_object(0).is_some());
        assert_eq!(document.object_count(), 1);
        assert_eq!(document.object(0).expect("remaining point").geometry.x, 9.0);
        assert!(document.delete_object(9).is_none());
    }

    #[test]
    fn source_less_point_document_writes_and_reads_back_through_native_reader() {
        let path = std::env::temp_dir().join(format!(
            "rhino3dm-rs-point-roundtrip-{}.3dm",
            std::process::id()
        ));
        let mut document = File3dm::new();
        document.add_point(Point3d::new(1.25, 2.5, 3.75), ObjectAttributes::default());
        document.write(&path).expect("native point writer");

        let decoded = File3dm::read(&path).expect("native point reader");
        assert_eq!(decoded.archive().object_count(), 1);
        assert_eq!(
            decoded.archive().objects[0].point,
            Some(Point3d::new(1.25, 2.5, 3.75))
        );
        if std::env::var_os("RHINO3DM_RS_KEEP_ROUNDTRIP").is_none() {
            std::fs::remove_file(path).expect("remove temporary round-trip archive");
        }
    }

    #[test]
    fn named_point_uses_native_free_vertex_object_presentation() {
        let path = std::env::temp_dir().join(format!(
            "rhino3dm-rs-named-point-roundtrip-{}.3dm",
            std::process::id()
        ));
        let attributes = ObjectAttributes {
            name: Some("NamedPoint".into()),
            ..ObjectAttributes::default()
        };
        let mut document = File3dm::new();
        document.add_point(Point3d::new(4.0, 5.0, 6.0), attributes);
        document.write(&path).expect("native named point writer");
        let decoded = File3dm::read(&path).expect("native named point reader");
        assert_eq!(
            decoded.objects()[0].attributes.name.as_deref(),
            Some("NamedPoint")
        );
        std::fs::remove_file(path).expect("remove temporary named point archive");
    }

    fn header() -> Vec<u8> {
        let mut bytes = vec![b' '; HEADER_LENGTH];
        bytes[..FILE_SIGNATURE.len()].copy_from_slice(FILE_SIGNATURE);
        bytes[24..26].copy_from_slice(b"80");
        bytes
    }
    fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(body.len() as i64).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }
    fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
        let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }
    fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut payload = body.to_vec();
        payload.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        long_chunk(typecode, &payload)
    }
    #[test]
    fn structurally_indexes_an_object_table() {
        let mut bytes = header();
        bytes.extend(long_chunk(1, &[]));
        // Object records carry a trailing CRC32 in V2+ archives.
        let mut object_table = long_chunk(TCODE_OBJECT_RECORD, &[1, 2, 3, 0, 0, 0, 0]);
        object_table.extend(short_chunk(TCODE_END_OF_TABLE, 0));
        bytes.extend(long_chunk(TCODE_OBJECTS, &object_table));
        bytes.extend(long_chunk(TCODE_END_OF_FILE, &[0; 8]));
        let index = scan_archive(&bytes, 80).unwrap();
        assert_eq!(index.tables.len(), 1);
        assert_eq!(index.object_count(), 1);
    }
    #[test]
    fn rejects_non_3dm_input() {
        assert!(matches!(
            parse_header(&[0_u8; 32]),
            Err(Error::InvalidSignature)
        ));
    }
    #[test]
    fn retains_source_bytes_and_checks_indexed_ranges() {
        let mut bytes = header();
        bytes.extend(long_chunk(1, &[]));
        let mut object_table = long_chunk(TCODE_OBJECT_RECORD, &[1, 2, 3, 0, 0, 0, 0]);
        object_table.extend(short_chunk(TCODE_END_OF_TABLE, 0));
        bytes.extend(long_chunk(TCODE_OBJECTS, &object_table));
        bytes.extend(long_chunk(TCODE_END_OF_FILE, &[0; 8]));

        let model = File3dm::from_bytes(bytes.clone()).expect("synthetic archive is framed");
        assert_eq!(model.source().as_bytes(), bytes);
        assert_eq!(model.source().len(), bytes.len());
        let record = model.archive().objects.first().expect("one object record");
        assert_eq!(
            model
                .source_slice(record.source)
                .expect("object source range"),
            &bytes[record.source.offset as usize..end(record.source).unwrap() as usize]
        );
        assert!(matches!(
            model.source_slice(SourceRange {
                offset: bytes.len() as u64,
                length: 1,
            }),
            Err(Error::OutOfBounds { .. })
        ));
    }
    #[test]
    fn refuses_source_bytes_over_the_explicit_limit_before_indexing() {
        let bytes = header();
        assert!(matches!(
            File3dm::from_bytes_with_limits(bytes.clone(), ReadLimits::new(bytes.len() - 1)),
            Err(Error::InputTooLarge { limit, actual }) if limit == bytes.len() - 1 && actual == bytes.len() as u64
        ));
        assert!(matches!(
            File3dm::from_bytes_with_limits(bytes, ReadLimits::new(HEADER_LENGTH)),
            Err(Error::Truncated {
                offset,
                needed,
            }) if offset == HEADER_LENGTH as u64 && needed == 12
        ));
    }
    #[test]
    fn reads_header_from_a_bounded_reader() {
        let bytes = header();
        let mut input = &bytes[..];
        assert_eq!(read_header(&mut input).unwrap().archive_version, 80);
        assert_eq!(input.len(), 0);
    }
    #[test]
    fn validates_direct_class_uuid_crc() {
        let uuid = [0xa5; 16];
        let valid = crc_chunk(CLASS_UUID, &uuid);
        assert!(chunk_at_with_class_crc(&valid, 0, valid.len(), 80, true).is_ok());

        let mut corrupt = valid;
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(matches!(
            chunk_at_with_class_crc(&corrupt, 0, corrupt.len(), 80, true),
            Err(Error::CrcMismatch { .. })
        ));
    }
    #[test]
    fn keeps_native_and_python_vector_translation_contracts_separate() {
        let vector = Vector3d::new(0.6, 0.8, 0.0);
        assert_eq!(Transform::translation_vector(vector).matrix[0][3], 0.6);
        assert_eq!(
            Transform::python_translation_vector(vector).matrix[0][3],
            f64::from(0.6_f32)
        );
    }
    #[test]
    fn math_basics_are_stable_for_native_consumers() {
        let point = Point3d::new(2.0, 3.0, 5.0);
        let translation = Transform::translation(7.0, 11.0, 13.0);
        let scale = Transform::diagonal(2.0);
        let composed = translation.multiply(scale);
        assert_eq!(
            point.transformed(translation),
            Point3d::new(9.0, 14.0, 18.0)
        );
        assert_eq!(point.transformed(composed), Point3d::new(11.0, 17.0, 23.0));
        let inverse = composed.try_inverse().expect("non-singular transform");
        let round_trip = point.transformed(composed).transformed(inverse);
        assert!((round_trip.x - point.x).abs() <= 1e-12);
        assert!((round_trip.y - point.y).abs() <= 1e-12);
        assert!((round_trip.z - point.z).abs() <= 1e-12);
        assert!(Transform::diagonal(0.0).try_inverse().is_none());
    }
    #[test]
    fn matches_foundational_open_nurbs_math_edge_cases() {
        let point = Point3d::new(2.0, 3.0, 5.0);
        let vector = Vector3d::new(3.0, 4.0, 0.0);
        assert_eq!(
            point.add_point(Point3d::new(7.0, 11.0, 13.0)),
            Point3d::new(9.0, 14.0, 18.0)
        );
        assert_eq!(point.add_vector(vector), Point3d::new(5.0, 7.0, 5.0));
        assert_eq!(point.scaled(2.0), Point3d::new(4.0, 6.0, 10.0));
        assert_eq!(point.encode().get("X"), Some(&2.0));
        assert_eq!(
            Point3d::unset(),
            Point3d::new(UNSET_VALUE, UNSET_VALUE, UNSET_VALUE)
        );
        assert_eq!(vector.encode().get("Y"), Some(&4.0));
        assert_eq!(vector.is_parallel_to(Vector3d::new(-3.0, -4.0, 0.0)), -1);
        assert_eq!(vector.is_parallel_to(Vector3d::default()), 0);
        assert_eq!(
            vector.is_parallel_to_with_tolerance(Vector3d::new(3.0, 4.0, 0.001), 1e-4),
            0
        );
        assert_eq!(vector.vector_angle(Vector3d::default()), UNSET_VALUE);
        let zero = Transform::zero_transformation();
        assert!(zero.is_zero());
        assert!(!zero.is_zero_4x4());
        assert!(zero.is_zero_transformation());
        assert!(Transform::identity().is_rotation());
        assert!(Transform::try_rotation_axis_angle(
            std::f64::consts::FRAC_PI_2,
            Vector3d::new(0.0, 0.0, 1.0),
            Point3d::default(),
        )
        .unwrap()
        .is_rotation());
        assert!(
            Transform::try_rotation_axis_angle(0.0, Vector3d::default(), Point3d::default())
                .is_none()
        );
        assert!(!Transform::translation(1.0, 0.0, 0.0).is_linear());
        assert!(!Transform::unset().is_valid());
        assert_eq!(Transform::unset().determinant(), 0.0);
        assert_eq!(
            Transform::translation(7.0, 11.0, 13.0).to_float_array(false),
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7.0, 11.0, 13.0, 1.0,]
        );
    }
    #[test]
    fn two_dimensional_values_and_intervals_preserve_python_state_contracts() {
        let mut point = Point2d::new(1.25, -2.5);
        let other = Point2d::new(-3.0, 4.0);
        assert_eq!(point.add_point(other), Point2d::new(-1.75, 1.5));
        assert!((point.distance_to(other) - 7.766_112_283_504_533).abs() < 1e-12);
        point.x = 7.25;
        point.y = -1.0;
        assert_eq!(point.encode().get("X"), Some(&7.25));
        assert_eq!(point.encode().get("Y"), Some(&-1.0));

        let mut vector = Vector2d::new(1.0, 2.0);
        vector.x = 7.25;
        vector.y = -1.0;
        assert_eq!(vector.encode(), point.encode());

        let mut interval = Interval::new(5.0, 2.0);
        interval.t0 = 7.25;
        interval.t1 = -1.0;
        assert_eq!(interval, Interval::new(7.25, -1.0));
    }
    #[test]
    fn point3f_retains_single_precision_storage_and_arithmetic() {
        let mut point = Point3f::new(0.1, 0.2, 0.3);
        assert_eq!(point.x, 0.1_f32);
        assert_eq!(
            point.add_point(Point3f::new(1.0, 2.0, 3.0)),
            Point3f::new(1.1, 2.2, 3.3)
        );
        point.x = 7.25;
        point.y = -1.0;
        point.z = 0.0;
        assert_eq!(point.encode().get("Z"), Some(&0.0));

        let mut vector = Vector3f::new(0.1, 0.2, 0.3);
        vector.x = 7.25;
        vector.y = -1.0;
        vector.z = 0.0;
        assert_eq!(vector.encode().get("X"), Some(&7.25));

        let mut homogeneous = Point4d::new(0.1, 0.2, 0.3, 0.4);
        homogeneous.x = 7.25;
        homogeneous.y = -1.0;
        homogeneous.z = 0.0;
        homogeneous.w = 2.0;
        assert_eq!(homogeneous.encode().get("W"), Some(&2.0));

        let mut line = Line::new(Point3d::new(1.0, 2.0, 3.0), Point3d::new(4.0, 6.0, 3.0));
        assert!(line.is_valid());
        assert_eq!(line.direction(), Vector3d::new(3.0, 4.0, 0.0));
        assert_eq!(line.point_at(2.0), Point3d::new(7.0, 10.0, 3.0));
        assert!(line.transform(Transform::translation(7.0, 11.0, 13.0)));
        assert_eq!(line.from, Point3d::new(8.0, 13.0, 16.0));
        assert!(!Line::new(Point3d::default(), Point3d::default()).is_valid());

        let mut bounds = BoundingBox::from_coordinates(1.0, 2.0, 3.0, 4.0, 6.0, 8.0);
        assert!(bounds.is_valid());
        assert_eq!(bounds.center(), Point3d::new(2.5, 4.0, 5.5));
        assert_eq!(bounds.area(), 94.0);
        assert_eq!(bounds.volume(), 60.0);
        assert_eq!(
            bounds.closest_point(Point3d::new(5.0, 7.0, 9.0)),
            bounds.max
        );
        bounds.inflate(Vector3d::new(1.0, 2.0, 3.0));
        assert_eq!(bounds.min, Point3d::new(0.0, 0.0, 0.0));
        assert_eq!(bounds.max, Point3d::new(5.0, 8.0, 11.0));
    }
    #[test]
    fn distinguishes_skipped_and_opaque_attribute_fields() {
        let mut skipped = vec![0x20];
        skipped.extend([0; 16]);
        skipped.extend(3_i32.to_le_bytes());
        skipped.push(3); // display mode: known width but not retained yet
        skipped.extend(7_i32.to_le_bytes());
        skipped.push(0);
        let attributes = parse_attributes(
            &skipped,
            SourceRange {
                offset: 0,
                length: skipped.len() as u64,
            },
        )
        .unwrap();
        assert!(!attributes.complete);
        assert_eq!(
            attributes.coverage,
            AttributeCoverage::Partial {
                skipped_tags: vec![3],
                opaque_suffix_from_tag: None,
            }
        );

        let mut opaque = vec![0x20];
        opaque.extend([0; 16]);
        opaque.extend(3_i32.to_le_bytes());
        opaque.push(42);
        let attributes = parse_attributes(
            &opaque,
            SourceRange {
                offset: 0,
                length: opaque.len() as u64,
            },
        )
        .unwrap();
        assert_eq!(
            attributes.coverage,
            AttributeCoverage::Partial {
                skipped_tags: vec![],
                opaque_suffix_from_tag: Some(42),
            }
        );
    }
    #[test]
    fn retains_failed_instance_definition_records() {
        let mut bytes = header();
        bytes.extend(long_chunk(1, &[]));
        let mut records = long_chunk(TCODE_INSTANCE_DEFINITION_RECORD, &[0; 4]);
        records.extend(short_chunk(TCODE_END_OF_TABLE, 0));
        bytes.extend(long_chunk(TCODE_INSTANCE_DEFINITIONS, &records));
        bytes.extend(long_chunk(TCODE_END_OF_FILE, &[0; 8]));

        let index = scan_archive(&bytes, 80).unwrap();
        assert!(index.instance_definitions.is_empty());
        assert_eq!(index.instance_definition_records.len(), 1);
        let record = &index.instance_definition_records[0];
        assert!(record.definition.is_none());
        assert!(record
            .error
            .as_deref()
            .is_some_and(|message| !message.is_empty()));
    }
    #[test]
    fn decodes_v1_instance_reference_prefix() {
        let mut payload = vec![0x10];
        payload.extend_from_slice(&[0x7a; 16]);
        for row in Transform::IDENTITY.matrix {
            for value in row {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        let reference = parse_instance_reference(
            &payload,
            SourceRange {
                offset: 0,
                length: payload.len() as u64,
            },
        )
        .unwrap();
        assert_eq!(reference.definition_id, [0x7a; 16]);
        assert_eq!(reference.transform, Transform::IDENTITY);
    }
    #[test]
    fn geometry_probe_requires_a_complete_census_and_no_errors() {
        let mut probe = GeometryProbe {
            backend: "test",
            units: Value::Null,
            entity_counts: BTreeMap::new(),
            native_namespaces: Vec::new(),
            decoded_object_records: Some(3),
            source_object_records: Some(3),
            warnings: Vec::new(),
        };
        assert!(probe.complete_object_decode());
        probe.warnings.push("Error: unreadable mesh".to_owned());
        assert!(!probe.complete_object_decode());
    }
}
#[test]
fn pbr_reconstruction_stays_fail_closed_until_all_channels_are_public() {
    let capabilities = File3dm::capabilities();
    assert!(!capabilities.complete_pbr_reconstruction());
    assert!(!capabilities.pbr_materials);
    assert!(!capabilities.embedded_texture_bytes);
    assert!(!capabilities.mesh_uv_channels);
    assert!(!capabilities.texture_mapping_transforms);
    assert!(!capabilities.per_face_materials);
}

#[test]
fn mesh_projection_has_bounds_checked_python_shape_queries() {
    let mut mesh = Mesh::new();
    mesh.add_vertex(Point3d::new(1.0, 2.0, 3.0));
    mesh.add_triangle([0, 0, 0]);
    assert_eq!(mesh.vertex_count(), 1);
    assert_eq!(mesh.face_count(), 1);
    assert_eq!(
        mesh.vertex(0),
        Some(Point3d {
            x: 1.0,
            y: 2.0,
            z: 3.0
        })
    );
    assert_eq!(mesh.face(0), Some(MeshFace::Triangle([0, 0, 0])));
    assert_eq!(mesh.vertex(1), None);
    assert_eq!(mesh.face(1), None);
}

#[test]
fn mesh_face_mutation_retains_invalid_faces_like_python() {
    let mut mesh = Mesh::new();
    for point in [
        Point3d::new(0.0, 0.0, 0.0),
        Point3d::new(1.0, 0.0, 0.0),
        Point3d::new(1.0, 1.0, 0.0),
        Point3d::new(0.0, 1.0, 0.0),
    ] {
        mesh.add_vertex(point);
    }
    assert_eq!(mesh.add_triangle([0, 1, 2]), 0);
    assert_eq!(mesh.add_quad([0, 1, 2, 3]), 1);
    assert_eq!(mesh.add_triangle([0, 1, 8]), -1);
    assert_eq!(mesh.face_count(), 3);
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(mesh.quad_count(), 1);
    assert!(!mesh.set_face(0, MeshFace::Triangle([0, 1, 9])));
    assert_eq!(mesh.face(0), Some(MeshFace::Triangle([0, 1, 9])));
    assert_eq!(mesh.triangle_count(), 0);
    mesh.clear_faces();
    assert_eq!(mesh.face_count(), 0);
}

#[test]
fn clearing_mesh_vertices_retains_faces_but_invalidates_their_counts() {
    let mut mesh = Mesh::new();
    for point in [
        Point3d::new(0.0, 0.0, 0.0),
        Point3d::new(1.0, 0.0, 0.0),
        Point3d::new(0.0, 1.0, 0.0),
    ] {
        mesh.add_vertex(point);
    }
    assert_eq!(mesh.add_triangle([0, 1, 2]), 0);
    mesh.clear_vertices();
    assert_eq!(mesh.vertex_count(), 0);
    assert_eq!(mesh.face_count(), 1);
    assert_eq!(mesh.triangle_count(), 0);
    assert_eq!(mesh.face(0), Some(MeshFace::Triangle([0, 1, 2])));
}

#[test]
fn mesh_normals_compute_flip_unitize_and_clear() {
    let mut mesh = Mesh::new();
    for point in [
        Point3d::new(0.0, 0.0, 0.0),
        Point3d::new(1.0, 0.0, 0.0),
        Point3d::new(0.0, 1.0, 0.0),
    ] {
        mesh.add_vertex(point);
    }
    mesh.add_triangle([0, 1, 2]);
    assert!(mesh.compute_normals());
    assert_eq!(mesh.normal_count(), 3);
    assert_eq!(mesh.normals[0], Vector3f::new(0.0, 0.0, 1.0));
    mesh.flip_normals();
    assert_eq!(mesh.normals[0], Vector3f::new(0.0, 0.0, -1.0));
    assert!(mesh.unitize_normals());
    mesh.clear_normals();
    assert_eq!(mesh.normal_count(), 0);
    assert!(!mesh.unitize_normals());
}

#[test]
fn mesh_vertex_colors_have_indexed_add_and_clear_semantics() {
    let mut mesh = Mesh::new();
    assert_eq!(mesh.color_count(), 0);
    assert_eq!(mesh.add_color([1, 2, 3, 255]), 0);
    assert_eq!(mesh.add_color([4, 5, 6, 7]), 1);
    assert_eq!(mesh.vertex_colors, vec![[1, 2, 3, 255], [4, 5, 6, 7]]);
    mesh.clear_colors();
    assert_eq!(mesh.color_count(), 0);
}
