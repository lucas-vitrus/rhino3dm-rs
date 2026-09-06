//! Exact B-rep STEP to Rhino transfer.
//!
//! This module intentionally has no mesh fallback. It admits a STEP source
//! only when the decoder supplies exact B-rep geometry/topology and the Rhino
//! writer can emit it without a geometry, topology, units, or product loss.

use cadmpeg_codec_rhino::{RhinoArchiveVersion, RhinoEncoder};
use cadmpeg_codec_step::StepCodec;
use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::{
    validate_neutral, Codec, DecodeOptions, Encoder, LossCategory, LossNote, Severity,
};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Output archive version for [`import_exact_step`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RhinoVersion {
    V5,
    V6,
    V7,
    #[default]
    V8,
}

impl From<RhinoVersion> for RhinoArchiveVersion {
    fn from(value: RhinoVersion) -> Self {
        match value {
            RhinoVersion::V5 => Self::V5,
            RhinoVersion::V6 => Self::V6,
            RhinoVersion::V7 => Self::V7,
            RhinoVersion::V8 => Self::V8,
        }
    }
}

/// Policy for engineering-grade STEP import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactStepOptions {
    /// Rhino archive version to write. Rhino 8 is the default.
    pub target_version: RhinoVersion,
    /// Require every decoded body to be a closed solid. Set `false` to admit
    /// exact sheet/wire B-reps while retaining all other strict checks.
    pub require_closed_manifold_solids: bool,
}

impl Default for ExactStepOptions {
    fn default() -> Self {
        Self {
            target_version: RhinoVersion::V8,
            require_closed_manifold_solids: true,
        }
    }
}

/// Auditable result of one admitted exact STEP → Rhino transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactStepReport {
    pub input: PathBuf,
    pub output: PathBuf,
    pub source_bodies: usize,
    pub source_faces: usize,
    pub source_edges: usize,
    pub source_vertices: usize,
    pub target_archive_version: RhinoVersion,
}

/// A refusal raised before a misleading `.3dm` is written.
#[derive(Debug)]
pub enum ExactStepError {
    Io(std::io::Error),
    Decode(String),
    Validation(Vec<String>),
    Losses(Vec<String>),
    NoBrepBodies,
    NonSolidBodies { count: usize },
}

impl fmt::Display for ExactStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Decode(error) => write!(f, "STEP/Rhino codec error: {error}"),
            Self::Validation(findings) => {
                write!(f, "exact B-rep validation failed: {}", findings.join("; "))
            }
            Self::Losses(losses) => write!(
                f,
                "exact B-rep transfer refused due to loss: {}",
                losses.join("; ")
            ),
            Self::NoBrepBodies => write!(f, "STEP source contains no native B-rep bodies"),
            Self::NonSolidBodies { count } => write!(
                f,
                "STEP source contains {count} non-solid bodies; closed manifold solids are required"
            ),
        }
    }
}

impl std::error::Error for ExactStepError {}

impl From<std::io::Error> for ExactStepError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Import one ISO 10303-21 STEP file as native Rhino B-rep/NURBS geometry.
///
/// The output is written atomically only after decoder-loss, topology, solid,
/// dimensional, and encoder-loss gates all pass. The current codec supports
/// exact geometry at the document level; it does not invent render meshes.
pub fn import_exact_step(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: ExactStepOptions,
) -> Result<ExactStepReport, ExactStepError> {
    let input = input.as_ref();
    let output = output.as_ref();
    let mut source = File::open(input)?;
    let decoded = StepCodec::default()
        .decode(&mut source, &DecodeOptions::default())
        .map_err(|error| ExactStepError::Decode(error.to_string()))?;

    if !decoded.report().geometry_transferred {
        return Err(ExactStepError::Losses(vec![
            "STEP decoder did not transfer B-rep geometry".into(),
        ]));
    }
    reject_engineering_losses(&decoded.report().losses)?;

    let ir = decoded.ir();
    if ir.model.bodies.is_empty() {
        return Err(ExactStepError::NoBrepBodies);
    }
    if options.require_closed_manifold_solids {
        let non_solids = ir
            .model
            .bodies
            .iter()
            .filter(|body| body.kind != cadmpeg_ir::topology::BodyKind::Solid)
            .count();
        if non_solids != 0 {
            return Err(ExactStepError::NonSolidBodies { count: non_solids });
        }
    }

    let validation = validate_neutral(ir, decoded.report().losses.clone());
    if !validation.is_ok() {
        return Err(ExactStepError::Validation(
            validation
                .findings
                .iter()
                .map(|finding| format!("{}: {}", finding.check, finding.message))
                .collect(),
        ));
    }

    let plan = RhinoEncoder::new(options.target_version.into())
        .plan(EncodeInput {
            ir,
            fidelity: Some(decoded.source_fidelity()),
        })
        .map_err(|error| ExactStepError::Decode(error.to_string()))?;
    reject_engineering_losses(&plan.report().losses)?;

    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .map_err(|error| ExactStepError::Decode(error.to_string()))?;
    write_atomic(output, &bytes)?;

    Ok(ExactStepReport {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        source_bodies: ir.model.bodies.len(),
        source_faces: ir.model.faces.len(),
        source_edges: ir.model.edges.len(),
        source_vertices: ir.model.vertices.len(),
        target_archive_version: options.target_version,
    })
}

/// Import multiple independent STEP files. Each source gets a separate exact
/// Rhino document. This deliberately does not masquerade as a block assembly:
/// placement-aware `.3dm` instance-definition writing will be added only once
/// the writer preserves definition membership and occurrence transforms.
pub fn import_exact_steps<I, P>(
    inputs: I,
    output_directory: impl AsRef<Path>,
    options: ExactStepOptions,
) -> Result<Vec<ExactStepReport>, ExactStepError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let output_directory = output_directory.as_ref();
    fs::create_dir_all(output_directory)?;
    inputs
        .into_iter()
        .map(|input| {
            let input = input.as_ref();
            let stem = input
                .file_stem()
                .filter(|stem| !stem.is_empty())
                .ok_or_else(|| ExactStepError::Decode("STEP input has no file stem".into()))?;
            let mut output = output_directory.join(stem);
            output.set_extension("3dm");
            import_exact_step(input, output, options)
        })
        .collect()
}

fn reject_engineering_losses(losses: &[LossNote]) -> Result<(), ExactStepError> {
    let losses = losses
        .iter()
        .filter(|loss| {
            matches!(
                loss.code.category(),
                LossCategory::Geometry
                    | LossCategory::Topology
                    | LossCategory::Units
                    | LossCategory::Product
            ) || loss.severity >= Severity::Error
        })
        .map(|loss| format!("{} [{}]: {}", loss.code, loss.severity, loss.message))
        .collect::<Vec<_>>();
    if losses.is_empty() {
        Ok(())
    } else {
        Err(ExactStepError::Losses(losses))
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ExactStepError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        ExactStepError::Decode("Rhino output path must include a file name".into())
    })?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| ExactStepError::Decode(error.to_string()))?
        .as_nanos();
    for attempt in 0..32_u8 {
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            stamp + u128::from(attempt)
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let write_result = (|| -> Result<(), ExactStepError> {
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return write_result;
    }
    Err(ExactStepError::Decode(
        "could not allocate a unique temporary 3DM output path".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_codec_step::{write_step, StepWriteOptions};
    use cadmpeg_ir::examples::unit_cube;

    #[test]
    fn invalid_step_topology_never_creates_a_rhino_output() {
        let root =
            std::env::temp_dir().join(format!("rhino3dm-rs-step-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let step = root.join("cube.step");
        let output = root.join("cube.3dm");
        let mut bytes = Vec::new();
        write_step(&unit_cube(), &mut bytes, &StepWriteOptions::default()).unwrap();
        fs::write(&step, bytes).unwrap();

        // The current STEP reader rejects the writer's deliberately complex
        // multi-face planar ownership witness. The important contract here is
        // that an exact-import refusal never leaves a misleading 3DM behind.
        assert!(import_exact_step(&step, &output, ExactStepOptions::default()).is_err());
        assert!(!output.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
