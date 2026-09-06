use rhino3dm_render::{render_technical_svg, TechnicalRenderOptions, TechnicalView};
use rhino3dm_rs::scene::SceneDocument;
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let input = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: rhino3dm-svg INPUT.3dm OUTPUT.svg [--top|--front|--right|--isometric]")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: rhino3dm-svg INPUT.3dm OUTPUT.svg [--top|--front|--right|--isometric]")?;
    let mut options = TechnicalRenderOptions::default();
    for argument in arguments {
        options.view = match argument.to_string_lossy().as_ref() {
            "--top" => TechnicalView::Top,
            "--front" => TechnicalView::Front,
            "--right" => TechnicalView::Right,
            "--isometric" => TechnicalView::Isometric,
            other => return Err(format!("unknown render option: {other}").into()),
        };
    }
    let scene = SceneDocument::read(&input)?;
    let image = render_technical_svg(&scene, &options)?;
    std::fs::write(&output, image.svg)?;
    eprintln!(
        "wrote {} ({} points, {} edges, {} omitted objects)",
        output.display(),
        image.rendered_points,
        image.rendered_edges,
        image.omitted_objects.len()
    );
    if !image.omitted_objects.is_empty() {
        eprintln!(
            "the omitted IDs are available through TechnicalSvg::omitted_objects for structured callers"
        );
    }
    Ok(())
}
