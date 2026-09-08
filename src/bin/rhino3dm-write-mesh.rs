//! Write one source-less mesh for cross-language conformance checks.

use rhino3dm_rs::{Mesh, Point3d, Vector3f};
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("usage: rhino3dm-write-mesh OUTPUT")?,
    );
    if env::args_os().nth(2).is_some() {
        return Err("too many arguments".into());
    }
    let mut mesh = Mesh::new();
    for point in [
        Point3d::new(0.0, 0.0, 0.0),
        Point3d::new(1.0, 0.0, 0.0),
        Point3d::new(1.0, 1.0, 0.0),
        Point3d::new(0.0, 1.0, 0.0),
    ] {
        mesh.add_vertex(point);
    }
    mesh.add_quad([0, 1, 2, 3]);
    for _ in 0..4 {
        mesh.add_normal(Vector3f::new(0.0, 0.0, 1.0));
    }
    for color in [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ] {
        mesh.add_color(color);
    }
    for uv in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
        mesh.add_texture_coordinate(uv);
    }
    mesh.write(output)?;
    Ok(())
}
