use rhino3dm_rs::File3dm;
use serde_json::json;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: rhino3dm-bench FILE [ITERATIONS] [WARMUPS]")?,
    );
    let iterations = args
        .next()
        .map(|value| value.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(30_usize);
    let warmups = args
        .next()
        .map(|value| value.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(5_usize);
    if iterations == 0 {
        return Err("iterations must be greater than zero".into());
    }

    for _ in 0..warmups {
        black_box(inspect(&path)?);
    }

    let mut samples_ms = Vec::with_capacity(iterations);
    let mut census = None;
    for _ in 0..iterations {
        let start = Instant::now();
        let current = inspect(&path)?;
        samples_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
        if let Some(expected) = census {
            if expected != current {
                return Err("fixture census changed between iterations".into());
            }
        } else {
            census = Some(current);
        }
        black_box(current);
    }

    println!(
        "{}",
        json!({
            "runtime": "rhino3dm-rs",
            "iterations": iterations,
            "warmups": warmups,
            "samples_ms": samples_ms,
            "summary_ms": summarize(&samples_ms),
            "census": census.expect("at least one iteration"),
        })
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct Census {
    archive_version: u32,
    objects: usize,
    attributed_objects: usize,
    named_objects: usize,
    user_strings: usize,
    instance_definitions: usize,
    instance_definition_members: usize,
}

fn inspect(path: &PathBuf) -> Result<Census, rhino3dm_rs::Error> {
    let model = File3dm::read(path)?;
    let objects = &model.archive().objects;
    let definitions = &model.archive().instance_definitions;
    Ok(Census {
        archive_version: model.archive_version(),
        objects: objects.len(),
        attributed_objects: objects
            .iter()
            .filter(|object| object.attributes.is_some())
            .count(),
        named_objects: objects
            .iter()
            .filter_map(|object| object.attributes.as_ref())
            .filter(|attributes| attributes.name.is_some())
            .count(),
        user_strings: objects
            .iter()
            .filter_map(|object| object.attributes.as_ref())
            .map(|attributes| attributes.user_strings.len())
            .sum(),
        instance_definitions: definitions.len(),
        instance_definition_members: definitions
            .iter()
            .map(|definition| definition.members.len())
            .sum(),
    })
}

fn summarize(samples: &[f64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
        sorted[index]
    };
    json!({
        "min": sorted[0],
        "median": percentile(0.5),
        "mean": samples.iter().sum::<f64>() / samples.len() as f64,
        "p95": percentile(0.95),
        "max": sorted[sorted.len() - 1],
    })
}
