//! Dump the semantic pure-Rust bridge representation for diagnostics and
//! fixture development. This is deliberately separate from the stable public
//! `rhino3dm-rs` API so its JSON shape can evolve with `cadmpeg`.

use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: rhino3dm-dump SOURCE.3dm")?;
    let mut input = File::open(path)?;
    let decoded = RhinoCodec.decode(&mut input, &DecodeOptions::default())?;
    println!("{}", decoded.ir().to_canonical_json()?);
    Ok(())
}
