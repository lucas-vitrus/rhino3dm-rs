//! Exact engineering STEP → Rhino B-rep command.

use rhino3dm_rs::step::{import_exact_step, ExactStepOptions};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next() else {
        usage();
    };
    let Some(output) = args.next() else {
        usage();
    };
    let allow_sheets = args.next().is_some_and(|flag| flag == "--allow-sheets");
    if args.next().is_some() {
        usage();
    }
    let options = ExactStepOptions {
        require_closed_manifold_solids: !allow_sheets,
        ..ExactStepOptions::default()
    };
    match import_exact_step(PathBuf::from(input), PathBuf::from(output), options) {
        Ok(report) => {
            println!("output={}", report.output.display());
            println!("bodies={}", report.source_bodies);
            println!("faces={}", report.source_faces);
            println!("edges={}", report.source_edges);
            println!("vertices={}", report.source_vertices);
            println!("archive_version={:?}", report.target_archive_version);
        }
        Err(error) => {
            eprintln!("rhino3dm-step: {error}");
            std::process::exit(1);
        }
    }
}

fn usage() -> ! {
    eprintln!("Usage: rhino3dm-step INPUT.step OUTPUT.3dm [--allow-sheets]");
    std::process::exit(2);
}
