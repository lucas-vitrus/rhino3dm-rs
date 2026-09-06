//! Lightweight technical rendering for [`rhino3dm_rs::scene::SceneDocument`].
//!
//! This crate intentionally begins with a deterministic SVG wireframe backend:
//! it is small enough for an AI design loop, needs no GPU or external renderer,
//! and never substitutes triangles for missing engineering geometry. It draws
//! source display meshes and points only; exact B-rep/NURBS objects without a
//! source display mesh are reported as omissions.

use rhino3dm_rs::scene::{RhinoId, SceneDocument, SceneGeometry};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write;

/// Orthographic view directions supported by the technical renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalView {
    Top,
    Front,
    Right,
    Isometric,
}

/// Deterministic SVG technical-render settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TechnicalRenderOptions {
    pub width: u32,
    pub height: u32,
    pub view: TechnicalView,
    pub padding: f64,
    pub background: String,
    pub stroke: String,
    pub stroke_width: f64,
    pub point_radius: f64,
}

impl Default for TechnicalRenderOptions {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
            view: TechnicalView::Isometric,
            padding: 32.0,
            background: "#ffffff".into(),
            stroke: "#17202a".into(),
            stroke_width: 1.0,
            point_radius: 2.0,
        }
    }
}

/// Rendered SVG plus explicit data-coverage diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TechnicalSvg {
    pub svg: String,
    pub width: u32,
    pub height: u32,
    /// Objects omitted because the public scene did not carry drawable source
    /// display geometry. This is not an error disguised as an empty drawing.
    pub omitted_objects: Vec<RhinoId>,
    pub rendered_points: usize,
    pub rendered_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    InvalidCanvas,
    InvalidPadding,
    NonFiniteGeometry { object_id: RhinoId },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCanvas => write!(f, "technical render canvas must be non-zero"),
            Self::InvalidPadding => write!(f, "technical render padding leaves no drawable area"),
            Self::NonFiniteGeometry { object_id } => {
                write!(
                    f,
                    "object {object_id:?} contains non-finite display geometry"
                )
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// Render source display data from a 3DM scene into a deterministic SVG.
///
/// The output is a technical *wireframe preview*, not a hidden-line drawing
/// and not a photorealistic PBR render. Exact B-rep/NURBS are never tessellated
/// implicitly; callers can distinguish omission through `omitted_objects`.
pub fn render_technical_svg(
    scene: &SceneDocument,
    options: &TechnicalRenderOptions,
) -> Result<TechnicalSvg, RenderError> {
    if options.width == 0 || options.height == 0 {
        return Err(RenderError::InvalidCanvas);
    }
    let width = f64::from(options.width);
    let height = f64::from(options.height);
    if !options.padding.is_finite()
        || options.padding < 0.0
        || options.padding * 2.0 >= width
        || options.padding * 2.0 >= height
    {
        return Err(RenderError::InvalidPadding);
    }

    let mut points = Vec::new();
    let mut edges = Vec::new();
    let mut omitted = Vec::new();

    for object in scene.objects.iter().filter(|object| object.visible) {
        match &object.geometry {
            SceneGeometry::Point { position } => {
                ensure_finite(*position, &object.id)?;
                points.push(*position);
            }
            SceneGeometry::RenderMeshes { meshes } => {
                for mesh in meshes {
                    for vertex in &mesh.vertices {
                        ensure_finite(*vertex, &object.id)?;
                    }
                    let mesh_edges = if mesh.feature_edges.is_empty() {
                        triangle_edges(&mesh.triangles)
                    } else {
                        mesh.feature_edges.clone()
                    };
                    for [first, second] in mesh_edges {
                        let (Some(a), Some(b)) = (
                            mesh.vertices.get(first as usize),
                            mesh.vertices.get(second as usize),
                        ) else {
                            continue;
                        };
                        edges.push((*a, *b));
                    }
                }
            }
            SceneGeometry::InstanceReference { .. }
            | SceneGeometry::ExactBrepCarrier
            | SceneGeometry::Unsupported => omitted.push(object.id.clone()),
        }
    }

    let projected_points = points
        .iter()
        .copied()
        .map(|point| project(point, options.view))
        .collect::<Vec<_>>();
    let projected_edges = edges
        .iter()
        .map(|(first, second)| {
            (
                project(*first, options.view),
                project(*second, options.view),
            )
        })
        .collect::<Vec<_>>();
    let viewport = viewport(&projected_points, &projected_edges, options, width, height);
    let map = |point| viewport.map(point);

    let mut svg = String::new();
    write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"{}\"/><g fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\">",
        options.width, options.height, options.width, options.height, options.background, options.stroke, options.stroke_width,
    )
    .expect("writing to String cannot fail");
    for (first, second) in &projected_edges {
        let [x1, y1] = map(*first);
        let [x2, y2] = map(*second);
        write!(svg, "<path d=\"M {x1:.3} {y1:.3} L {x2:.3} {y2:.3}\"/>")
            .expect("writing to String cannot fail");
    }
    svg.push_str("</g><g>");
    for point in projected_points {
        let [x, y] = map(point);
        write!(
            svg,
            "<circle cx=\"{x:.3}\" cy=\"{y:.3}\" r=\"{}\" fill=\"{}\"/>",
            options.point_radius, options.stroke
        )
        .expect("writing to String cannot fail");
    }
    svg.push_str("</g></svg>");

    Ok(TechnicalSvg {
        svg,
        width: options.width,
        height: options.height,
        omitted_objects: omitted,
        rendered_points: points.len(),
        rendered_edges: edges.len(),
    })
}

fn ensure_finite(point: [f64; 3], object_id: &RhinoId) -> Result<(), RenderError> {
    if point.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(RenderError::NonFiniteGeometry {
            object_id: object_id.clone(),
        })
    }
}

fn triangle_edges(triangles: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut edges = BTreeSet::new();
    for triangle in triangles {
        for [first, second] in [
            [triangle[0], triangle[1]],
            [triangle[1], triangle[2]],
            [triangle[2], triangle[0]],
        ] {
            edges.insert([first.min(second), first.max(second)]);
        }
    }
    edges.into_iter().collect()
}

fn project([x, y, z]: [f64; 3], view: TechnicalView) -> [f64; 2] {
    match view {
        TechnicalView::Top => [x, y],
        TechnicalView::Front => [x, z],
        TechnicalView::Right => [y, z],
        TechnicalView::Isometric => [x - y, z + (x + y) * 0.5],
    }
}

struct Viewport {
    min: [f64; 2],
    scale: f64,
    height: f64,
}

impl Viewport {
    fn map(&self, point: [f64; 2]) -> [f64; 2] {
        [
            (point[0] - self.min[0]) * self.scale,
            self.height - (point[1] - self.min[1]) * self.scale,
        ]
    }
}

fn viewport(
    points: &[[f64; 2]],
    edges: &[([f64; 2], [f64; 2])],
    options: &TechnicalRenderOptions,
    width: f64,
    height: f64,
) -> Viewport {
    let bounds = points
        .iter()
        .copied()
        .chain(edges.iter().flat_map(|(first, second)| [*first, *second]))
        .fold(
            None,
            |bounds: Option<([f64; 2], [f64; 2])>, point| match bounds {
                Some((min, max)) => Some((
                    [min[0].min(point[0]), min[1].min(point[1])],
                    [max[0].max(point[0]), max[1].max(point[1])],
                )),
                None => Some((point, point)),
            },
        );
    let ([min_x, min_y], [max_x, max_y]) = bounds.unwrap_or(([0.0, 0.0], [1.0, 1.0]));
    let content_width = (max_x - min_x).max(1.0);
    let content_height = (max_y - min_y).max(1.0);
    let drawable_width = width - 2.0 * options.padding;
    let drawable_height = height - 2.0 * options.padding;
    let scale = (drawable_width / content_width).min(drawable_height / content_height);
    let occupied_width = content_width * scale;
    let occupied_height = content_height * scale;
    Viewport {
        min: [
            min_x - (drawable_width - occupied_width) / (2.0 * scale),
            min_y - (drawable_height - occupied_height) / (2.0 * scale),
        ],
        scale,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhino3dm_rs::scene::{ExactGeometrySummary, RenderMesh, Rgba8, SceneObject};
    use std::collections::BTreeMap;

    fn scene() -> SceneDocument {
        SceneDocument {
            archive_version: 8,
            units: serde_json::Value::Null,
            layers: vec![],
            materials: vec![],
            textures: vec![],
            texture_assets: vec![],
            objects: vec![SceneObject {
                id: RhinoId("point".into()),
                name: "point".into(),
                visible: true,
                layer_index: 0,
                material_index: -1,
                material_source: 0,
                color: Rgba8([0, 0, 0, 255]),
                color_source: 0,
                groups: vec![],
                user_strings: BTreeMap::new(),
                geometry: SceneGeometry::Point {
                    position: [1.0, 2.0, 3.0],
                },
                source: serde_json::Value::Null,
            }],
            definitions: vec![],
            occurrences: vec![],
            exact_geometry: ExactGeometrySummary::default(),
            diagnostics: vec![],
        }
    }

    #[test]
    fn technical_svg_is_self_contained_for_a_point_scene() {
        let image = render_technical_svg(&scene(), &TechnicalRenderOptions::default()).unwrap();
        assert!(image.svg.starts_with("<svg "));
        assert!(image.svg.contains("<circle"));
        assert_eq!(image.rendered_points, 1);
        assert!(image.omitted_objects.is_empty());
    }

    #[test]
    fn triangle_edges_are_deduplicated() {
        assert_eq!(
            triangle_edges(&[[0, 1, 2], [2, 1, 3]]),
            vec![[0, 1], [0, 2], [1, 2], [1, 3], [2, 3]]
        );
    }

    #[test]
    fn mesh_preview_uses_source_feature_edges_when_available() {
        let mut scene = scene();
        scene.objects.push(SceneObject {
            id: RhinoId("mesh".into()),
            name: "mesh".into(),
            visible: true,
            layer_index: 0,
            material_index: -1,
            material_source: 0,
            color: Rgba8([0, 0, 0, 255]),
            color_source: 0,
            groups: vec![],
            user_strings: BTreeMap::new(),
            geometry: SceneGeometry::RenderMeshes {
                meshes: vec![RenderMesh {
                    vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    triangles: vec![[0, 1, 2]],
                    feature_edges: vec![[0, 1]],
                    normals: vec![],
                    corner_normals: vec![],
                    texture_assignments: vec![],
                    channels: vec![],
                }],
            },
            source: serde_json::Value::Null,
        });
        let image = render_technical_svg(&scene, &TechnicalRenderOptions::default()).unwrap();
        assert_eq!(image.rendered_edges, 1);
        assert_eq!(image.rendered_points, 1);
        assert!(image.svg.contains("<path"));
    }
}
