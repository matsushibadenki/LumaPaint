use anyhow::{Context, Result, bail, ensure};
use edfx::{Document, MAX_FONT, MAX_TEXT, Paragraph};
use std::{
    env,
    fs::File,
    io::{self, Read},
    path::Path,
};

const USAGE: &str = "Usage:
  edfx new PARAGRAPHS.json FONT OUTPUT.edfx [FACE_INDEX]
  edfx inspect INPUT.edfx
  edfx export-svg INPUT.edfx OUTPUT.svg
  edfx edit INPUT.edfx PARAGRAPH_INDEX REPLACEMENT.txt OUTPUT.edfx

FONT is a path to an OpenType font (.otf, .ttf or collection).
Paragraph indices are zero-based. Replacement text may end with one newline.";

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "{} exceeds the size limit",
        path.display()
    );
    Ok(bytes)
}

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let args = args.iter().map(Path::new).collect::<Vec<_>>();
    match args.as_slice() {
        [flag] if *flag == Path::new("--help") => println!("{USAGE}"),
        [command, source, font, output, index @ ..]
            if *command == Path::new("new") && index.len() <= 1 =>
        {
            let face_index = match index.first() {
                Some(index) => index.to_str().context("invalid face index")?.parse()?,
                None => 0,
            };
            let model = read_bounded(source, 64 * 1024)?;
            let paragraphs: Vec<Paragraph> = serde_json::from_slice(&model)?;
            let bytes = read_bounded(font, MAX_FONT)?;
            Document::new(paragraphs, &bytes, face_index)?.save(output)?;
        }
        [command, source] if *command == Path::new("inspect") => {
            let document = Document::load(source)?;
            serde_json::to_writer_pretty(io::stdout().lock(), document.snapshot())?;
            println!();
        }
        [command, source, output] if *command == Path::new("export-svg") => {
            Document::load(source)?.export_svg(output)?;
        }
        [command, source, index, replacement, output] if *command == Path::new("edit") => {
            let index = index.to_str().context("invalid paragraph index")?.parse()?;
            let bytes = read_bounded(replacement, MAX_TEXT + 2)?;
            let text = std::str::from_utf8(&bytes)?;
            let text = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'))
                .unwrap_or(text);
            let mut document = Document::load(source)?;
            document.replace_text(index, text.to_owned())?;
            document.save(output)?;
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}
