//! Write one source-less point document for cross-language conformance checks.

use rhino3dm_rs::{File3dm, ObjectAttributes, Point3d};
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: rhino3dm-write-point OUTPUT X Y Z [NAME]")?,
    );
    let x: f64 = args.next().ok_or("missing X")?.to_string_lossy().parse()?;
    let y: f64 = args.next().ok_or("missing Y")?.to_string_lossy().parse()?;
    let z: f64 = args.next().ok_or("missing Z")?.to_string_lossy().parse()?;
    let name = args
        .next()
        .map(|value| value.to_string_lossy().into_owned());
    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    let mut document = File3dm::new();
    let attributes = ObjectAttributes {
        name,
        ..ObjectAttributes::default()
    };
    document.add_point(Point3d::new(x, y, z), attributes);
    document.write(output)?;
    Ok(())
}
