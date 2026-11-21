use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustybuzz::{Direction, Face, Language, UnicodeBuffer, ttf_parser::Permissions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use unicode_linebreak::{BreakOpportunity, linebreaks};
use unicode_script::{Script, UnicodeScript};

mod outline;
mod svg;

const FORMAT: &str = "EDFX/0.1";
const MAX_FILE: usize = 96 * 1024 * 1024;
pub const MAX_FONT: usize = 64 * 1024 * 1024;
pub const MAX_TEXT: usize = 8192;

const PAGE_WIDTH: i64 = 300_000;
const PAGE_HEIGHT: i64 = 420_000;
const MARGIN: i64 = 30_000;
const FONT_SIZE: i64 = 12_000;
const FIRST_BASELINE: i64 = MARGIN + FONT_SIZE;
const LINE_HEIGHT: i64 = 18_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Locale {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "zh-Hans")]
    ZhHans,
}

impl Locale {
    fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
            Self::ZhHans => "zh-Hans",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Paragraph {
    pub locale: Locale,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Glyph {
    pub id: u16,
    pub cluster: usize,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub baseline: i64,
    pub glyphs: Vec<Glyph>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub width: i64,
    pub height: i64,
    pub font_size: i64,
    pub lines: Vec<Line>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedFont {
    face_index: u32,
    data: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    format: String,
    paragraphs: Vec<Paragraph>,
    font: EmbeddedFont,
    snapshot: Snapshot,
    sha256: String,
}

impl Document {
    pub fn new(paragraphs: Vec<Paragraph>, font_bytes: &[u8], face_index: u32) -> Result<Self> {
        let face = parse_font(font_bytes, face_index)?;
        let snapshot = layout(&paragraphs, &face)?;
        let mut document = Self {
            format: FORMAT.into(),
            paragraphs,
            font: EmbeddedFont {
                face_index,
                data: STANDARD.encode(font_bytes),
            },
            snapshot,
            sha256: String::new(),
        };
        document.sha256 = document.digest()?;
        Ok(document)
    }

    pub fn paragraphs(&self) -> &[Paragraph] {
        &self.paragraphs
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn replace_text(&mut self, index: usize, text: String) -> Result<()> {
        ensure!(
            index < self.paragraphs.len(),
            "paragraph index out of bounds"
        );
        ensure!(
            text.len() <= MAX_TEXT,
            "replacement text exceeds the size limit"
        );
        let mut paragraphs = self.paragraphs.clone();
        paragraphs[index].text = text;
        self.replace_paragraphs(paragraphs)
    }

    /// Reflow all paragraph edits as a single transaction.
    pub fn replace_paragraphs(&mut self, paragraphs: Vec<Paragraph>) -> Result<()> {
        if paragraphs == self.paragraphs {
            return Ok(());
        }
        let bytes = self.font_bytes()?;
        let face = parse_font(&bytes, self.font.face_index)?;
        let snapshot = layout(&paragraphs, &face)?;
        let sha256 = digest(&self.format, &paragraphs, &self.font, &snapshot)?;
        self.paragraphs = paragraphs;
        self.snapshot = snapshot;
        self.sha256 = sha256;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut data = Vec::new();
        File::open(path)?
            .take((MAX_FILE + 1) as u64)
            .read_to_end(&mut data)?;
        Self::from_json(&data)
    }

    pub fn from_json(data: &[u8]) -> Result<Self> {
        ensure!(data.len() <= MAX_FILE, "EDFX file exceeds the size limit");
        let document: Self = serde_json::from_slice(data)?;
        ensure!(document.format == FORMAT, "unsupported EDFX version");
        document.validate_snapshot()?;
        ensure!(
            document.digest()? == document.sha256,
            "EDFX integrity check failed"
        );
        Ok(document)
    }

    pub fn to_json(&self) -> Result<String> {
        let text = serde_json::to_string(self)?;
        ensure!(text.len() <= MAX_FILE, "EDFX file exceeds the size limit");
        Ok(text)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut tmp, self)?;
        tmp.write_all(b"\n")?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)?;
        Ok(())
    }

    fn font_bytes(&self) -> Result<Vec<u8>> {
        let bytes = STANDARD.decode(&self.font.data)?;
        ensure!(bytes.len() <= MAX_FONT, "font exceeds the size limit");
        Ok(bytes)
    }

    fn digest(&self) -> Result<String> {
        digest(&self.format, &self.paragraphs, &self.font, &self.snapshot)
    }

    fn validate_snapshot(&self) -> Result<()> {
        validate_text(&self.paragraphs)?;
        let bytes = self.font_bytes()?;
        let face = parse_font(&bytes, self.font.face_index)?;
        let snapshot = &self.snapshot;
        ensure!(
            (snapshot.width, snapshot.height, snapshot.font_size)
                == (PAGE_WIDTH, PAGE_HEIGHT, FONT_SIZE),
            "invalid page geometry"
        );
        let mut lines = snapshot.lines.iter();
        let mut baseline = FIRST_BASELINE;
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            let mut offset = 0;
            loop {
                let line = lines.next().context("missing line")?;
                ensure!(
                    line.paragraph == index
                        && line.start == offset
                        && line.baseline == baseline
                        && baseline <= PAGE_HEIGHT - MARGIN,
                    "invalid line geometry or sequence"
                );
                let text = paragraph
                    .text
                    .get(line.start..line.end)
                    .context("invalid line range")?;
                ensure!(
                    line.end > line.start || paragraph.text.is_empty(),
                    "empty line range"
                );
                ensure!(
                    text.is_empty() == line.glyphs.is_empty(),
                    "invalid glyph list"
                );
                for glyph in &line.glyphs {
                    ensure!(
                        glyph.id > 0
                            && glyph.id < face.number_of_glyphs()
                            && (line.start..line.end).contains(&glyph.cluster)
                            && paragraph.text.is_char_boundary(glyph.cluster),
                        "invalid glyph reference"
                    );
                }
                baseline += LINE_HEIGHT;
                offset = line.end;
                if offset == paragraph.text.len() {
                    break;
                }
            }
        }
        ensure!(lines.next().is_none(), "unexpected extra lines");
        Ok(())
    }
}

fn digest(
    format: &str,
    paragraphs: &[Paragraph],
    font: &EmbeddedFont,
    snapshot: &Snapshot,
) -> Result<String> {
    let mut hash = Sha256::new();
    serde_json::to_writer(&mut hash, &(format, paragraphs, font, snapshot))?;
    Ok(format!("{:x}", hash.finalize()))
}

fn parse_font(bytes: &[u8], index: u32) -> Result<Face<'_>> {
    ensure!(bytes.len() <= MAX_FONT, "font exceeds the size limit");
    let face = Face::from_slice(bytes, index).context("invalid OpenType font or face index")?;
    ensure!(
        matches!(
            face.permissions(),
            Some(Permissions::Installable | Permissions::Editable)
        ),
        "font must permit editable embedding"
    );
    Ok(face)
}

fn validate_text(paragraphs: &[Paragraph]) -> Result<()> {
    ensure!(
        !paragraphs.is_empty() && paragraphs.len() <= 20,
        "expected 1 to 20 paragraphs"
    );
    let mut total = 0;
    for paragraph in paragraphs {
        total += paragraph.text.len();
        ensure!(total <= MAX_TEXT, "document text exceeds the size limit");
        ensure!(
            paragraph.text.chars().all(|c| !c.is_control() && matches!(
                c.script(), Script::Latin | Script::Han | Script::Hiragana | Script::Katakana | Script::Common | Script::Inherited
            ) && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}')),
            "unsupported character in paragraph"
        );
    }
    Ok(())
}

fn layout(paragraphs: &[Paragraph], face: &Face<'_>) -> Result<Snapshot> {
    validate_text(paragraphs)?;
    let mut lines = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let text = paragraph.text.as_str();
        let mut breaks = linebreaks(text).peekable();
        let mut start = 0;
        loop {
            let baseline = FIRST_BASELINE + lines.len() as i64 * LINE_HEIGHT;
            ensure!(
                baseline <= PAGE_HEIGHT - MARGIN,
                "text overflows the fixed page"
            );
            let mut best = None;
            if text.is_empty() {
                best = Some((0, Vec::new()));
            }
            while let Some(&(end, opportunity)) = breaks.peek() {
                let (width, glyphs) =
                    shape_line(face, paragraph.locale, &text[start..end], start, baseline)?;
                if width > PAGE_WIDTH - 2 * MARGIN {
                    break;
                }
                best = Some((end, glyphs));
                breaks.next();
                if opportunity == BreakOpportunity::Mandatory {
                    break;
                }
            }
            let Some((end, glyphs)) = best else {
                bail!("a line segment exceeds the fixed text width in paragraph {index}");
            };
            lines.push(Line {
                paragraph: index,
                start,
                end,
                baseline,
                glyphs,
            });
            if end == text.len() {
                break;
            }
            start = end;
        }
    }
    let snapshot = Snapshot {
        width: PAGE_WIDTH,
        height: PAGE_HEIGHT,
        font_size: FONT_SIZE,
        lines,
    };
    let scale = snapshot.outline_scale(face);
    for line in &snapshot.lines {
        for glyph in &line.glyphs {
            snapshot.checked_outline(face, line.paragraph, glyph, scale)?;
        }
    }
    Ok(snapshot)
}

fn shape_line(
    face: &Face<'_>,
    locale: Locale,
    text: &str,
    start: usize,
    baseline: i64,
) -> Result<(i64, Vec<Glyph>)> {
    let language: Language = locale
        .tag()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid locale"))?;
    let scale = |units: i64| {
        let units_per_em = i64::from(face.units_per_em());
        let numerator = units * FONT_SIZE;
        (numerator + numerator.signum() * (units_per_em / 2)) / units_per_em
    };
    let mut glyphs = Vec::new();
    let mut pen = 0;
    let mut boundaries = Vec::new();
    let mut active = Script::Common;
    for (offset, character) in text.char_indices() {
        let script = character.script();
        if matches!(script, Script::Common | Script::Inherited) {
            continue;
        }
        if active != Script::Common && active != script {
            boundaries.push(offset);
        }
        active = script;
    }
    let mut run_start = 0;
    for run_end in boundaries.into_iter().chain(std::iter::once(text.len())) {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(&text[run_start..run_end]);
        buffer.set_direction(Direction::LeftToRight);
        buffer.set_language(language.clone());
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(face, &[], buffer);
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            ensure!(
                info.glyph_id != 0,
                "missing glyph at byte {}",
                start + run_start + info.cluster as usize
            );
            glyphs.push(Glyph {
                id: info.glyph_id as u16,
                cluster: start + run_start + info.cluster as usize,
                x: MARGIN + scale(pen + i64::from(position.x_offset)),
                y: baseline - scale(i64::from(position.y_offset)),
            });
            pen += i64::from(position.x_advance);
        }
        run_start = run_end;
    }
    Ok((scale(pen), glyphs))
}
