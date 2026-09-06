//! Renderer-facing Rhino document model.
//!
//! The archive reader remains the source-of-truth for byte ranges and opaque
//! data. This module exposes the semantic data recovered by the pure-Rust
//! bridge in a stable, deliberately conservative Rust shape. It is suitable
//! for renderer planning and diagnostics; it does not claim a scene can be
//! faithfully rendered until its capability gate passes.

use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::{
    tessellation::{Tessellation, TessellationChannelDomain},
    Codec, DecodeOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::path::Path;

/// A semantic, renderer-facing projection of one 3DM document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneDocument {
    /// Archive version read from the original 3DM header.
    pub archive_version: u32,
    /// Canonical decoded units. The bridge normalizes length values to mm.
    pub units: Value,
    /// Named Rhino layers with source UUIDs, inheritance, and visibility.
    pub layers: Vec<Layer>,
    /// Legacy and physically-based source materials. Unknown material data is
    /// retained in `source` rather than discarded.
    pub materials: Vec<Material>,
    /// Document textures and their mapping data as referenced by materials.
    pub textures: Vec<Texture>,
    /// Embedded-image and legacy bitmap descriptors. A descriptor proves that
    /// the source has an asset but does *not* expose image bytes yet.
    pub texture_assets: Vec<TextureAsset>,
    /// Object-level names, UUIDs, attributes, and user strings.
    pub objects: Vec<SceneObject>,
    /// Reusable Rhino block definitions.
    pub definitions: Vec<InstanceDefinition>,
    /// Placed uses of block definitions; a definition and occurrence retain
    /// distinct identities.
    pub occurrences: Vec<InstanceOccurrence>,
    /// Exact B-rep/NURBS carrier census. This is intentionally separate from
    /// render meshes.
    pub exact_geometry: ExactGeometrySummary,
    /// Every source loss emitted by the semantic decoder.
    pub diagnostics: Vec<SceneDiagnostic>,
}

/// Source UUIDs are retained as their canonical hyphenated string form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RhinoId(pub String);

/// A 4-component color as it appeared in a Rhino record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba8(pub [u8; 4]);

/// One Rhino layer with parent and material inheritance inputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layer {
    pub id: RhinoId,
    pub archive_index: i32,
    pub name: String,
    pub parent_id: Option<RhinoId>,
    pub color: Rgba8,
    pub visible: bool,
    pub locked: bool,
    pub material_index: i32,
    pub linetype_index: i32,
    pub source: Value,
}

/// One legacy Rhino material and optional native PBR payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Material {
    pub id: RhinoId,
    pub source_id: Option<RhinoId>,
    pub archive_index: Option<i32>,
    pub name: String,
    pub diffuse: Rgba8,
    pub emission: Rgba8,
    pub transparency: f64,
    pub index_of_refraction: f64,
    pub physically_based: Option<PhysicallyBasedMaterial>,
    pub textures: Vec<Texture>,
    pub source: Value,
}

/// Scalars from Rhino's native physically based material payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicallyBasedMaterial {
    pub base_color: [f32; 4],
    pub metallic: f64,
    pub roughness: f64,
    pub specular: f64,
    pub specular_tint: f64,
    pub anisotropic: f64,
    pub anisotropic_rotation: f64,
    pub sheen: f64,
    pub sheen_tint: f64,
    pub clearcoat: f64,
    pub clearcoat_roughness: f64,
    pub opacity: f64,
    pub opacity_ior: f64,
    pub opacity_roughness: f64,
    pub emission: [f32; 4],
    pub alpha: f64,
    pub source: Value,
}

/// A material texture with full source UVW matrix and asset reference data.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Texture {
    pub id: Option<RhinoId>,
    pub mapping_channel: u32,
    pub path: String,
    pub enabled: bool,
    pub texture_type: u32,
    pub wrap: [u32; 3],
    pub uvw_transform: [[f64; 4]; 4],
    pub treat_as_linear: Option<bool>,
    pub embedded_file_id: Option<RhinoId>,
    pub source: Value,
}

/// A source image record referenced by a 3DM material or texture mapping.
///
/// `byte_len` and `sha256` describe the source payload without pretending the
/// payload itself is available to the public API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextureAsset {
    pub id: String,
    pub source_id: Option<RhinoId>,
    pub name: String,
    pub path: String,
    pub byte_len: u64,
    pub sha256: String,
    pub source: Value,
}

/// An indexed display mesh retained independently from exact B-rep/NURBS.
///
/// This is renderer input decoded from the 3DM's own display/facet stream. It
/// is never used as a STEP import fallback or as the engineering source model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderMesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub feature_edges: Vec<[u32; 2]>,
    pub normals: Vec<[f64; 3]>,
    pub corner_normals: Vec<[f64; 3]>,
    pub texture_assignments: Vec<MeshTextureAssignment>,
    pub channels: Vec<MeshChannel>,
}

/// A texture resource assigned to a subset of indexed mesh triangles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeshTextureAssignment {
    pub source_id: Option<String>,
    pub texture_id: String,
    pub triangles: Vec<u32>,
}

/// A source-defined mesh channel such as UV or vertex color data.
///
/// Channel payload bytes are exact decoder output. `kind` remains a native
/// numeric tag until Rhino's tag vocabulary is fully stabilized in this API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeshChannel {
    pub domain: MeshChannelDomain,
    pub kind: u32,
    pub flags: u32,
    pub item_size: u32,
    pub count: u32,
    pub data: Vec<u8>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshChannelDomain {
    Vertex,
    Corner,
    Triangle,
    Unknown,
}

/// Object attributes needed for visibility, grouping, styling, and lookup.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneObject {
    pub id: RhinoId,
    pub name: String,
    pub visible: bool,
    pub layer_index: i32,
    pub material_index: i32,
    pub material_source: i32,
    pub color: Rgba8,
    pub color_source: i32,
    pub groups: Vec<i32>,
    pub user_strings: BTreeMap<String, String>,
    pub geometry: SceneGeometry,
    pub source: Value,
}

/// Renderer-relevant geometry classification without converting exact B-reps
/// to triangle meshes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneGeometry {
    Point {
        position: [f64; 3],
    },
    /// One object can carry multiple source display meshes (for example, a
    /// per-face B-rep cache). Keep all of them; collapsing to one would lose
    /// geometry and per-face material association.
    RenderMeshes {
        meshes: Vec<RenderMesh>,
    },
    InstanceReference {
        definition_id: RhinoId,
    },
    ExactBrepCarrier,
    Unsupported,
}

/// Rhino block/instance-definition identity and membership.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstanceDefinition {
    pub id: RhinoId,
    pub name: String,
    pub description: String,
    pub member_object_ids: Vec<RhinoId>,
    pub source: Value,
}

/// A placed occurrence, distinct from its definition and source member.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstanceOccurrence {
    pub id: RhinoId,
    pub name: String,
    pub definition_id: RhinoId,
    pub parent_definition_ids: Vec<RhinoId>,
    pub transform: [[f64; 4]; 4],
    pub visible: bool,
    pub source: Value,
}

/// Counts for analytic/NURBS/B-rep entities retained independently from mesh
/// display data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ExactGeometrySummary {
    pub bodies: usize,
    pub regions: usize,
    pub shells: usize,
    pub faces: usize,
    pub loops: usize,
    pub coedges: usize,
    pub edges: usize,
    pub vertices: usize,
    pub curves: usize,
    pub surfaces: usize,
    pub points: usize,
}

/// A semantic-decoder warning with stable source category and loss code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
}

/// Conservative renderer admission facts for a document projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SceneCapabilities {
    pub object_attributes: bool,
    pub layers: bool,
    pub definitions: bool,
    pub pbr_scalars: bool,
    pub texture_mapping_transforms: bool,
    pub embedded_texture_bytes: bool,
    pub mesh_uv_channels: bool,
    pub per_face_materials: bool,
}

impl SceneCapabilities {
    /// True only when all data a backend needs for faithful textured PBR is
    /// exposed through this public API.
    pub const fn complete_pbr_reconstruction(self) -> bool {
        self.pbr_scalars
            && self.texture_mapping_transforms
            && self.embedded_texture_bytes
            && self.mesh_uv_channels
            && self.per_face_materials
    }
}

#[derive(Debug)]
pub enum SceneError {
    Io(std::io::Error),
    Archive(crate::Error),
    Decode(String),
    SemanticShape(&'static str),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Archive(error) => write!(f, "3DM archive error: {error}"),
            Self::Decode(error) => write!(f, "semantic 3DM decode error: {error}"),
            Self::SemanticShape(field) => write!(f, "semantic 3DM record has invalid {field}"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<std::io::Error> for SceneError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::Error> for SceneError {
    fn from(value: crate::Error) -> Self {
        Self::Archive(value)
    }
}

impl SceneDocument {
    /// Decode source-level attributes, layers, materials, blocks, and the
    /// analytic geometry census through the pure-Rust semantic bridge.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, SceneError> {
        let archive = crate::File3dm::read(&path)?;
        let mut input = File::open(path)?;
        let decoded = RhinoCodec
            .decode(&mut input, &DecodeOptions::default())
            .map_err(|error| SceneError::Decode(error.to_string()))?;
        let value = serde_json::to_value(decoded.ir())
            .map_err(|error| SceneError::Decode(error.to_string()))?;
        let model = object_at(&value, &["model"])?;
        let arenas = object_at(&value, &["native", "rhino", "arenas"])?;

        let layers = records(arenas, "layers")
            .iter()
            .map(parse_layer)
            .collect::<Result<Vec<_>, _>>()?;
        let materials = records(arenas, "materials")
            .iter()
            .map(parse_material)
            .collect::<Result<Vec<_>, _>>()?;
        let textures = materials
            .iter()
            .flat_map(|material| material.textures.clone())
            .collect();
        let texture_assets = records(arenas, "embedded_images")
            .iter()
            .chain(records(arenas, "windows_bitmaps"))
            .map(parse_texture_asset)
            .collect::<Result<Vec<_>, _>>()?;
        let definitions = records(arenas, "product_definitions")
            .iter()
            .map(parse_definition)
            .collect::<Result<Vec<_>, _>>()?;
        let occurrences = records(arenas, "product_occurrences")
            .iter()
            .map(parse_occurrence)
            .collect::<Result<Vec<_>, _>>()?;
        let presentations = records(arenas, "object_presentation");
        let points = records(model, "points");
        let point_positions = positions_by_source_object(points);
        let mesh_data = meshes_by_source_object(&decoded.ir().model.tessellations);
        let occurrence_definitions = occurrences
            .iter()
            .map(|occurrence| (occurrence.id.0.clone(), occurrence.definition_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let archive_geometry = archive
            .archive()
            .objects
            .iter()
            .enumerate()
            .map(|(index, object)| {
                (
                    format!("rhino:object:record#{index:06}"),
                    object.geometry_kind,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut objects = presentations
            .iter()
            .map(|record| {
                parse_object(
                    record,
                    &point_positions,
                    &mesh_data,
                    &occurrence_definitions,
                    &archive_geometry,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        objects.sort_by(|left, right| left.id.cmp(&right.id));

        let diagnostics = decoded
            .report()
            .losses
            .iter()
            .map(|loss| SceneDiagnostic {
                severity: loss.severity.to_string(),
                code: loss.code.to_string(),
                message: loss.message.clone(),
            })
            .collect();

        Ok(Self {
            archive_version: archive.archive_version(),
            units: value.get("units").cloned().unwrap_or(Value::Null),
            layers,
            materials,
            textures,
            texture_assets,
            objects,
            definitions,
            occurrences,
            exact_geometry: ExactGeometrySummary {
                bodies: records(model, "bodies").len(),
                regions: records(model, "regions").len(),
                shells: records(model, "shells").len(),
                faces: records(model, "faces").len(),
                loops: records(model, "loops").len(),
                coedges: records(model, "coedges").len(),
                edges: records(model, "edges").len(),
                vertices: records(model, "vertices").len(),
                curves: records(model, "curves").len(),
                surfaces: records(model, "surfaces").len(),
                points: points.len(),
            },
            diagnostics,
        })
    }

    /// Public projection coverage. These flags stay false for channels whose
    /// exact source bytes or associations are not yet exposed.
    pub const fn capabilities() -> SceneCapabilities {
        SceneCapabilities {
            object_attributes: true,
            layers: true,
            definitions: true,
            pbr_scalars: true,
            texture_mapping_transforms: true,
            embedded_texture_bytes: false,
            mesh_uv_channels: true,
            per_face_materials: true,
        }
    }

    /// Produces a deterministic, Three.js-oriented scene manifest without
    /// changing coordinates, flattening blocks, or baking source transforms.
    /// Callers can convert this manifest into `Object3D` / `BufferGeometry`
    /// while retaining original Rhino IDs in `userData.rhino`.
    pub fn threejs_manifest(&self) -> ThreejsManifest {
        let nodes = self
            .objects
            .iter()
            .map(|object| ThreejsNode {
                name: object.name.clone(),
                rhino_id: object.id.clone(),
                visible: object.visible,
                layer_index: object.layer_index,
                geometry: object.geometry.clone(),
            })
            .collect();
        ThreejsManifest {
            coordinate_system: "rhino-z-up-document-units".into(),
            nodes,
            required_capabilities: Self::capabilities(),
            warnings: self
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect(),
        }
    }
}

/// Stable adapter payload for a Three.js implementation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThreejsManifest {
    /// No implicit Y-up flip is performed. The consuming adapter must apply one
    /// documented root transform if a Three.js Y-up scene is desired.
    pub coordinate_system: String,
    pub nodes: Vec<ThreejsNode>,
    pub required_capabilities: SceneCapabilities,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThreejsNode {
    pub name: String,
    pub rhino_id: RhinoId,
    pub visible: bool,
    pub layer_index: i32,
    pub geometry: SceneGeometry,
}

fn records<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> &'a [Value] {
    object
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn object_at<'a>(
    value: &'a Value,
    path: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, SceneError> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_object)
        .ok_or(SceneError::SemanticShape("semantic document object"))
}

fn required_string(record: &Value, key: &'static str) -> Result<String, SceneError> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(SceneError::SemanticShape(key))
}

fn optional_string(record: &Value, key: &str) -> Option<String> {
    record.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn int(record: &Value, key: &str, default: i32) -> i32 {
    record
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(default)
}

fn uint(record: &Value, key: &str, default: u32) -> u32 {
    record
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
}

fn float(record: &Value, key: &str, default: f64) -> f64 {
    record.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn bool_value(record: &Value, key: &str, default: bool) -> bool {
    record.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn rgba(record: &Value, key: &str) -> Rgba8 {
    let mut result = [0; 4];
    if let Some(values) = record.get(key).and_then(Value::as_array) {
        for (index, value) in values.iter().take(4).enumerate() {
            result[index] = value
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .unwrap_or(0);
        }
    }
    Rgba8(result)
}

fn matrix(record: &Value, key: &str) -> [[f64; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for (index, value) in result.iter_mut().enumerate() {
        value[index] = 1.0;
    }
    if let Some(rows) = record.get(key).and_then(Value::as_array) {
        for (row_index, row) in rows.iter().take(4).enumerate() {
            if let Some(values) = row.as_array() {
                for (column_index, value) in values.iter().take(4).enumerate() {
                    if let Some(value) = value.as_f64() {
                        result[row_index][column_index] = value;
                    }
                }
            }
        }
    }
    result
}

fn parse_layer(record: &Value) -> Result<Layer, SceneError> {
    Ok(Layer {
        id: RhinoId(required_string(record, "source_uuid")?),
        archive_index: int(record, "archive_index", -1),
        name: required_string(record, "name")?,
        parent_id: optional_string(record, "parent_uuid").map(RhinoId),
        color: rgba(record, "color"),
        visible: bool_value(record, "visible", true),
        locked: bool_value(record, "locked", false),
        material_index: int(record, "material_index", -1),
        linetype_index: int(record, "linetype_index", -1),
        source: record.clone(),
    })
}

fn parse_material(record: &Value) -> Result<Material, SceneError> {
    let pbr = record.get("physically_based").and_then(|value| {
        (!value.is_null()).then(|| PhysicallyBasedMaterial {
            base_color: color_f32(value, "base_color"),
            metallic: float(value, "metallic", 0.0),
            roughness: float(value, "roughness", 0.0),
            specular: float(value, "specular", 0.0),
            specular_tint: float(value, "specular_tint", 0.0),
            anisotropic: float(value, "anisotropic", 0.0),
            anisotropic_rotation: float(value, "anisotropic_rotation", 0.0),
            sheen: float(value, "sheen", 0.0),
            sheen_tint: float(value, "sheen_tint", 0.0),
            clearcoat: float(value, "clearcoat", 0.0),
            clearcoat_roughness: float(value, "clearcoat_roughness", 0.0),
            opacity: float(value, "opacity", 1.0),
            opacity_ior: float(value, "opacity_ior", 1.0),
            opacity_roughness: float(value, "opacity_roughness", 0.0),
            emission: color_f32(value, "emission"),
            alpha: float(value, "alpha", 1.0),
            source: value.clone(),
        })
    });
    Ok(Material {
        id: RhinoId(required_string(record, "id")?),
        source_id: optional_string(record, "source_uuid").map(RhinoId),
        archive_index: record
            .get("archive_index")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok()),
        name: required_string(record, "name")?,
        diffuse: rgba(record, "diffuse"),
        emission: rgba(record, "emission"),
        transparency: float(record, "transparency", 0.0),
        index_of_refraction: float(record, "index_of_refraction", 1.0),
        physically_based: pbr,
        textures: records(
            record
                .as_object()
                .ok_or(SceneError::SemanticShape("material"))?,
            "textures",
        )
        .iter()
        .map(parse_texture)
        .collect::<Result<Vec<_>, _>>()?,
        source: record.clone(),
    })
}

fn color_f32(record: &Value, key: &str) -> [f32; 4] {
    let mut result = [0.0; 4];
    if let Some(values) = record.get(key).and_then(Value::as_array) {
        for (index, value) in values.iter().take(4).enumerate() {
            result[index] = value.as_f64().unwrap_or(0.0) as f32;
        }
    }
    result
}

fn parse_texture(record: &Value) -> Result<Texture, SceneError> {
    let embedded_file_id = record
        .get("file_reference")
        .and_then(|reference| reference.get("embedded_file_uuid"))
        .and_then(Value::as_str)
        .map(|value| RhinoId(value.to_owned()));
    let wrap = record
        .get("wrap")
        .and_then(Value::as_array)
        .map_or([0; 3], |values| {
            let mut output = [0; 3];
            for (index, value) in values.iter().take(3).enumerate() {
                output[index] = value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(0);
            }
            output
        });
    Ok(Texture {
        id: optional_string(record, "source_uuid").map(RhinoId),
        mapping_channel: uint(record, "mapping_channel_id", 0),
        path: required_string(record, "legacy_file_path")?,
        enabled: bool_value(record, "enabled", false),
        texture_type: uint(record, "texture_type", 0),
        wrap,
        uvw_transform: matrix(record, "uvw_transform"),
        treat_as_linear: record.get("treat_as_linear").and_then(Value::as_bool),
        embedded_file_id,
        source: record.clone(),
    })
}

fn parse_texture_asset(record: &Value) -> Result<TextureAsset, SceneError> {
    let id = required_string(record, "id")?;
    let is_embedded_image = record.get("buffer_byte_len").is_some();
    Ok(TextureAsset {
        id,
        source_id: optional_string(record, "source_uuid").map(RhinoId),
        name: optional_string(record, "name").unwrap_or_default(),
        path: optional_string(record, "file_path").unwrap_or_default(),
        byte_len: record
            .get(if is_embedded_image {
                "buffer_byte_len"
            } else {
                "pixel_buffer_byte_len"
            })
            .and_then(Value::as_u64)
            .unwrap_or(0),
        sha256: optional_string(
            record,
            if is_embedded_image {
                "buffer_sha256"
            } else {
                "pixel_buffer_sha256"
            },
        )
        .unwrap_or_default(),
        source: record.clone(),
    })
}

fn parse_definition(record: &Value) -> Result<InstanceDefinition, SceneError> {
    Ok(InstanceDefinition {
        id: RhinoId(required_string(record, "source_uuid")?),
        name: required_string(record, "name")?,
        description: optional_string(record, "description").unwrap_or_default(),
        member_object_ids: records(
            record
                .as_object()
                .ok_or(SceneError::SemanticShape("definition"))?,
            "member_object_ids",
        )
        .iter()
        .filter_map(Value::as_str)
        .map(|value| RhinoId(value.to_owned()))
        .collect(),
        source: record.clone(),
    })
}

fn parse_occurrence(record: &Value) -> Result<InstanceOccurrence, SceneError> {
    Ok(InstanceOccurrence {
        id: RhinoId(required_string(record, "source_uuid")?),
        name: required_string(record, "name")?,
        definition_id: RhinoId(required_string(record, "definition_uuid")?),
        parent_definition_ids: records(
            record
                .as_object()
                .ok_or(SceneError::SemanticShape("occurrence"))?,
            "parent_definition_uuids",
        )
        .iter()
        .filter_map(Value::as_str)
        .map(|value| RhinoId(value.to_owned()))
        .collect(),
        transform: matrix(record, "transform"),
        visible: bool_value(record, "visible", true),
        source: record.clone(),
    })
}

fn positions_by_source_object(records: &[Value]) -> BTreeMap<String, [f64; 3]> {
    records
        .iter()
        .filter_map(|record| {
            let id = record.get("source_object")?.get("object_id")?.as_str()?;
            let position = record.get("position")?.as_object()?;
            Some((
                id.to_owned(),
                [
                    position.get("x")?.as_f64()?,
                    position.get("y")?.as_f64()?,
                    position.get("z")?.as_f64()?,
                ],
            ))
        })
        .collect()
}

fn meshes_by_source_object(records: &[Tessellation]) -> BTreeMap<String, Vec<RenderMesh>> {
    records
        .iter()
        .filter_map(|record| {
            let id = record.source_object.as_ref()?.object_id.clone();
            Some((id, render_mesh(record)))
        })
        .fold(BTreeMap::new(), |mut by_source_object, (id, mesh)| {
            by_source_object.entry(id).or_default().push(mesh);
            by_source_object
        })
}

fn render_mesh(record: &Tessellation) -> RenderMesh {
    RenderMesh {
        vertices: record.vertices.iter().copied().map(Into::into).collect(),
        triangles: record.triangles.clone(),
        feature_edges: record.feature_edges.clone(),
        normals: record.normals.iter().copied().map(Into::into).collect(),
        corner_normals: record
            .corner_normals
            .iter()
            .copied()
            .map(Into::into)
            .collect(),
        texture_assignments: record
            .texture_assignments
            .iter()
            .map(|assignment| MeshTextureAssignment {
                source_id: assignment.source_id.clone(),
                texture_id: assignment.texture.0.clone(),
                triangles: assignment.triangles.clone(),
            })
            .collect(),
        channels: record
            .channels
            .iter()
            .map(|channel| MeshChannel {
                domain: match channel.domain {
                    TessellationChannelDomain::Vertex => MeshChannelDomain::Vertex,
                    TessellationChannelDomain::Corner => MeshChannelDomain::Corner,
                    TessellationChannelDomain::Triangle => MeshChannelDomain::Triangle,
                    _ => MeshChannelDomain::Unknown,
                },
                kind: channel.kind,
                flags: channel.flags,
                item_size: channel.item_size,
                count: channel.count,
                data: channel.data.clone(),
                indices: channel.indices.clone(),
            })
            .collect(),
    }
}

fn parse_object(
    record: &Value,
    points: &BTreeMap<String, [f64; 3]>,
    meshes: &BTreeMap<String, Vec<RenderMesh>>,
    occurrences: &BTreeMap<String, RhinoId>,
    archive_geometry: &BTreeMap<String, crate::GeometryKind>,
) -> Result<SceneObject, SceneError> {
    let id = required_string(record, "source_uuid")?;
    let geometry = if let Some(position) = points.get(&id) {
        SceneGeometry::Point {
            position: *position,
        }
    } else if let Some(meshes) = meshes.get(&id) {
        SceneGeometry::RenderMeshes {
            meshes: meshes.clone(),
        }
    } else if let Some(definition_id) = occurrences.get(&id) {
        SceneGeometry::InstanceReference {
            definition_id: definition_id.clone(),
        }
    } else if records(
        record
            .as_object()
            .ok_or(SceneError::SemanticShape("object"))?,
        "links",
    )
    .iter()
    .filter_map(Value::as_str)
    .filter_map(|link| archive_geometry.get(link))
    .any(|kind| {
        matches!(
            kind,
            crate::GeometryKind::Brep | crate::GeometryKind::Extrusion
        )
    }) {
        SceneGeometry::ExactBrepCarrier
    } else {
        SceneGeometry::Unsupported
    };
    let user_strings = records(
        record
            .as_object()
            .ok_or(SceneError::SemanticShape("object"))?,
        "attribute_user_strings",
    )
    .iter()
    .filter_map(|entry| {
        Some((
            entry.get("key")?.as_str()?.to_owned(),
            entry.get("value")?.as_str()?.to_owned(),
        ))
    })
    .collect::<BTreeMap<_, _>>();
    Ok(SceneObject {
        id: RhinoId(id),
        name: required_string(record, "name")?,
        visible: bool_value(record, "visible", true),
        layer_index: int(record, "layer_index", -1),
        material_index: int(record, "material_index", -1),
        material_source: int(record, "material_source", 0),
        color: rgba(record, "color"),
        color_source: int(record, "color_source", 0),
        groups: records(
            record
                .as_object()
                .ok_or(SceneError::SemanticShape("object"))?,
            "group_indexes",
        )
        .iter()
        .filter_map(Value::as_i64)
        .filter_map(|value| i32::try_from(value).ok())
        .collect(),
        user_strings,
        geometry,
        source: record.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_projection_keeps_ids_layers_user_strings_and_instances() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/structural-benchmark-v1.3dm"
        );
        let scene = SceneDocument::read(fixture).unwrap();
        assert_eq!(scene.layers.len(), 2);
        assert_eq!(scene.objects.len(), 2_305);
        assert_eq!(scene.definitions.len(), 1);
        assert_eq!(scene.occurrences.len(), 256);
        // The semantic bridge retains the 2,048 source points plus the block
        // member and placed instance expansion as exact point carriers.
        assert_eq!(scene.exact_geometry.points, 2_304);
        assert!(scene.objects.iter().all(|object| !object.id.0.is_empty()));
        assert!(scene
            .objects
            .iter()
            .any(|object| object.user_strings.get("fixture")
                == Some(&"structural-benchmark-v1".into())));
        assert!(!SceneDocument::capabilities().complete_pbr_reconstruction());
    }

    #[test]
    fn render_mesh_projection_keeps_indexed_geometry_and_raw_channels() {
        let mesh = Tessellation {
            id: "fixture:mesh".into(),
            body: None,
            faces: vec![],
            chordal_deflection: None,
            source_object: Some(cadmpeg_ir::SourceObjectAssociation {
                format: "rhino".into(),
                object_id: "mesh-object".into(),
                name: None,
                color: None,
                visible: Some(true),
                layer: None,
                instance_path: vec![],
            }),
            vertices: vec![cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)],
            triangles: vec![[0, 0, 0]],
            feature_edges: vec![[0, 0]],
            strip_lengths: vec![],
            normals: vec![cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)],
            corner_normals: vec![],
            triangle_groups: vec![],
            texture_assignments: vec![],
            channels: vec![cadmpeg_ir::tessellation::TessellationChannel {
                domain: TessellationChannelDomain::Vertex,
                item_size: 8,
                kind: 7,
                flags: 3,
                count: 1,
                data: vec![1, 2, 3, 4, 5, 6, 7, 8],
                indices: vec![],
            }],
        };
        let projected = meshes_by_source_object(&[mesh]);
        let meshes = projected.get("mesh-object").unwrap();
        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0];
        assert_eq!(mesh.vertices, vec![[1.0, 2.0, 3.0]]);
        assert_eq!(mesh.triangles, vec![[0, 0, 0]]);
        assert_eq!(mesh.normals, vec![[0.0, 0.0, 1.0]]);
        assert_eq!(mesh.channels[0].data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(mesh.channels[0].domain, MeshChannelDomain::Vertex);
    }
}
