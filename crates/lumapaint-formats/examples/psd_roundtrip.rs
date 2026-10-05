//! Developer fixture utility: validated PSD/PSB raster import and export.
use lumapaint_formats::{
    export::{export_raster, ExportOptions},
    io::{format_from_extension, read_document, ReadContent, ReadOptions},
};
use std::{io::Read, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: psd_roundtrip INPUT.psd OUTPUT.psd|psb".into());
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let input_format = format_from_extension(
        input
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Missing input extension")?,
    )
    .ok_or("Unknown input format")?;
    let output_format = format_from_extension(
        output
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Missing output extension")?,
    )
    .ok_or("Unknown output format")?;
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(lumapaint_formats::psd::MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let loaded = read_document(
        input_format,
        "Fixture".into(),
        &bytes,
        ReadOptions::default(),
    )?;
    let ReadContent::Raster(state) = loaded.content else {
        return Err("Expected raster document".into());
    };
    let saved = export_raster(output_format, &state, ExportOptions::default())?;
    std::fs::write(output, saved.bytes)?;
    Ok(())
}
