//! A pure-Rust, source-read-only foundation for Rhino 3DM archives.
//!
//! The public names deliberately follow the observable `rhino3dm.py` model
//! where that improves migration (`File3dm`, `Point3d`, `ObjectAttributes`).
//! Unsupported data is reported explicitly; it is never decoded as empty data.

use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;

pub const FILE_SIGNATURE: &[u8; 24] = b"3D Geometry File Format ";
const HEADER_LENGTH: usize = 32;
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceRange {
    pub offset: u64,
    pub length: u64,
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
        let bytes = fs::read(path)?;
        let header = parse_header(&bytes)?;
        let archive = scan_archive(&bytes, header.archive_version)?;
        Ok(Self { header, archive })
    }

    pub fn read_archive_version(path: impl AsRef<Path>) -> Result<u32, Error> {
        let bytes = fs::read(path)?;
        Ok(parse_header(&bytes)?.archive_version)
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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub matrix: [[f64; 4]; 4],
}

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

impl Transform {
    pub const IDENTITY: Self = Self {
        matrix: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectAttributes {
    pub source: SourceRange,
    pub id: Option<[u8; 16]>,
    pub name: Option<String>,
    pub layer_index: Option<i32>,
    pub user_strings: Vec<(String, String)>,
    pub complete: bool,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Truncated { offset: u64, needed: usize },
    InvalidSignature,
    InvalidArchiveVersion,
    InvalidChunkLength { offset: u64, length: i64 },
    OutOfBounds { offset: u64, end: u64, bound: u64 },
    InvalidTableTerminator { offset: u64 },
    MissingEndOfFile,
    Unsupported { capability: &'static str },
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
            Self::InvalidChunkLength { offset, length } => {
                write!(f, "invalid chunk length {length} at {offset}")
            }
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
    let mut instance_definitions = Vec::new();
    while offset < bytes.len() {
        let chunk = chunk_at(bytes, offset, bytes.len(), archive_version)?;
        if without_crc(chunk.typecode) == TCODE_END_OF_FILE {
            return Ok(ArchiveIndex {
                tables,
                objects,
                instance_definitions,
                end_of_file: chunk.source,
            });
        }
        if chunk.short || chunk.typecode & 0x1000_0000 == 0 {
            return Err(Error::Unsupported {
                capability: "a non-table top-level 3DM chunk",
            });
        }
        let (table, mut table_objects, mut table_definitions) =
            scan_table(bytes, chunk, archive_version)?;
        offset = end(chunk.source)? as usize;
        tables.push(table);
        objects.append(&mut table_objects);
        instance_definitions.append(&mut table_definitions);
    }
    Err(Error::MissingEndOfFile)
}

fn scan_table(
    bytes: &[u8],
    table: Chunk,
    archive_version: u32,
) -> Result<(ArchiveTable, Vec<ObjectRecord>, Vec<InstanceDefinition>), Error> {
    let mut records = Vec::new();
    let mut objects = Vec::new();
    let mut instance_definitions = Vec::new();
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
                instance_definitions,
            ));
        }
        if without_crc(table.typecode) == TCODE_OBJECTS && child.typecode == TCODE_OBJECT_RECORD {
            objects.push(parse_object_record(bytes, child, archive_version));
        }
        if without_crc(table.typecode) == TCODE_INSTANCE_DEFINITIONS
            && child.typecode == TCODE_INSTANCE_DEFINITION_RECORD
        {
            if let Ok(definition) = parse_instance_definition(bytes, child, archive_version) {
                instance_definitions.push(definition);
            }
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
/// valid and `complete` communicates that the suffix is opaque.
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
        complete: true,
    };
    while offset < end {
        let tag = take_u8(bytes, &mut offset, end)?;
        match tag {
            0 => return Ok(attributes),
            1 => attributes.name = Some(read_utf16(bytes, &mut offset, end)?),
            2 => {
                let _url = read_utf16(bytes, &mut offset, end)?;
            }
            3 | 4 | 10 | 22 => skip(bytes, &mut offset, end, 4)?,
            6 | 7 => skip(bytes, &mut offset, end, 4)?,
            20 => skip(bytes, &mut offset, end, 16)?,
            8 => skip(bytes, &mut offset, end, 8)?,
            9 | 11..=17 | 19 | 23..=27 => skip(bytes, &mut offset, end, 1)?,
            18 => {
                let count = read_i32(bytes, &mut offset, end)?;
                skip_count(bytes, &mut offset, end, count, 4)?;
            }
            21 => {
                let count = read_i32(bytes, &mut offset, end)?;
                skip_count(bytes, &mut offset, end, count, 32)?;
            }
            _ => {
                attributes.complete = false;
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
