//! PDF Type 3 glyphs are graphics streams, not executable PostScript fonts.
use super::*;
#[derive(Clone)]
pub(super) struct Type3 {
    pub matrix: [f32; 6],
    pub resources: Option<Dictionary>,
    pub glyphs: Vec<Option<Vec<u8>>>,
}
impl Interpreter<'_> {
    pub(super) fn type3_font(&mut self, root: &Dictionary) -> Result<fonts::Font, ImportError> {
        let matrix: [f32; 6] = nums(
            resolve(self.doc, root.get(b"FontMatrix").map_err(|_| malformed())?)?
                .as_array()
                .map_err(|_| malformed())?,
            6,
        )?
        .try_into()
        .unwrap();
        let resources = root
            .get(b"Resources")
            .ok()
            .map(|o| resolve(self.doc, o))
            .transpose()?
            .map(|o| o.as_dict().map_err(|_| malformed()))
            .transpose()?;
        let cost = resources
            .map(|r| resource_bytes(r, 0))
            .transpose()?
            .unwrap_or(0)
            .saturating_add(16384);
        self.remaining = self
            .remaining
            .checked_sub(cost)
            .ok_or(ImportError::LimitExceeded("pdf.content_budget"))?;
        let resources = resources.cloned();
        let procs = resolve(self.doc, root.get(b"CharProcs").map_err(|_| malformed())?)?
            .as_dict()
            .map_err(|_| malformed())?;
        let encoding = resolve(self.doc, root.get(b"Encoding").map_err(|_| malformed())?)?
            .as_dict()
            .map_err(|_| ImportError::Unsupported("pdf.font_encoding"))?;
        let differences = resolve(
            self.doc,
            encoding
                .get(b"Differences")
                .map_err(|_| ImportError::Unsupported("pdf.font_encoding"))?,
        )?
        .as_array()
        .map_err(|_| malformed())?;
        let mut glyphs = vec![None; 256];
        let mut code = 256usize;
        for value in differences {
            if let Ok(name) = value.as_name() {
                if code >= 256 {
                    return Err(malformed());
                }
                if let Ok(proc) = procs.get(name) {
                    let stream = resolve(self.doc, proc)?
                        .as_stream()
                        .map_err(|_| malformed())?;
                    let bytes = stream
                        .get_plain_content_with_limit(1024 * 1024)
                        .map_err(|_| ImportError::LimitExceeded("pdf.glyph_outline"))?;
                    let content = glyph_content(&bytes)?;
                    let bytes = content.encode().map_err(|_| malformed())?;
                    self.remaining = self
                        .remaining
                        .checked_sub(bytes.len())
                        .ok_or(ImportError::LimitExceeded("pdf.content_budget"))?;
                    glyphs[code] = Some(bytes);
                }
                code += 1;
            } else {
                let n = num(value)?;
                if n.fract() != 0. || !(0. ..=255.).contains(&n) {
                    return Err(malformed());
                }
                code = n as usize;
            }
        }
        let first = num(resolve(
            self.doc,
            root.get(b"FirstChar").map_err(|_| malformed())?,
        )?)?;
        let last = num(resolve(
            self.doc,
            root.get(b"LastChar").map_err(|_| malformed())?,
        )?)?;
        let widths = resolve(self.doc, root.get(b"Widths").map_err(|_| malformed())?)?
            .as_array()
            .map_err(|_| malformed())?;
        if first.fract() != 0.
            || last.fract() != 0.
            || first < 0.
            || last > 255.
            || last < first
            || widths.len() != (last - first) as usize + 1
        {
            return Err(malformed());
        }
        let widths = widths
            .iter()
            .enumerate()
            .map(|(i, v)| Ok((first as u16 + i as u16, num(resolve(self.doc, v)?)?)))
            .collect::<Result<_, ImportError>>()?;
        Ok(fonts::Font {
            data: [].as_slice().into(),
            index: 0,
            outlines: None,
            type3: Some(Type3 {
                matrix,
                resources,
                glyphs,
            }),
            encoding: None,
            gids: vec![],
            widths,
            default_width: 0.,
            vertical: None,
        })
    }
    pub(super) fn type3_show(
        &mut self,
        font: &fonts::Font,
        bytes: &[u8],
        state: &State,
        resources: &Dictionary,
        tm: &mut [f32; 6],
    ) -> Result<(), ImportError> {
        let glyphs = font.type3.as_ref().ok_or_else(malformed)?;
        if self.glyph_depth >= 16 {
            return Err(ImportError::LimitExceeded("pdf.glyph_depth"));
        }
        for &code in bytes {
            if state.text.mode() != 3 {
                if let Some(content) = &glyphs.glyphs[usize::from(code)] {
                    let mut child = state.clone();
                    let (size, hscale, rise) = state.text.geometry();
                    child.ctm = matrix(
                        state.ctm,
                        matrix(
                            *tm,
                            matrix([size * hscale, 0., 0., size, 0., rise], glyphs.matrix),
                        ),
                    );
                    self.glyph_depth += 1;
                    let result = self.content(
                        content,
                        glyphs.resources.as_ref().unwrap_or(resources),
                        child,
                        self.glyph_depth,
                    );
                    self.glyph_depth -= 1;
                    result?;
                } else {
                    self.unsupported("pdf.glyph_missing")?;
                }
            }
            let width = font.widths.get(&u16::from(code)).copied().unwrap_or(0.);
            let (size, hscale, _) = state.text.geometry();
            let spacing = state.text.spacing_for(code == 32);
            *tm = matrix(
                *tm,
                [
                    1.,
                    0.,
                    0.,
                    1.,
                    (width * glyphs.matrix[0] * size + spacing) * hscale,
                    width * glyphs.matrix[1] * size,
                ],
            );
        }
        Ok(())
    }
}

// lopdf's content lexer splits the digit-bearing operators d0/d1 into d
// and an operand. Parse this required initial declaration before that lexer.
fn glyph_content(bytes: &[u8]) -> Result<lopdf::content::Content, ImportError> {
    let mut i = 0;
    for _ in 0..8 {
        while i < bytes.len() {
            if bytes[i].is_ascii_whitespace() || bytes[i] == 0 {
                i += 1;
            } else if bytes[i] == b'%' {
                while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                    i += 1;
                }
            } else {
                break;
            }
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'%'
            && bytes[i] != 0
        {
            i += 1;
        }
        if matches!(&bytes[start..i], b"d0" | b"d1") {
            let count = if bytes[start + 1] == b'0' { 2 } else { 6 };
            let mut declaration = bytes[..start].to_vec();
            declaration.extend_from_slice(b" d");
            let parsed = lopdf::content::Content::decode(&declaration).map_err(|_| malformed())?;
            if parsed.operations.len() != 1 {
                return Err(malformed());
            }
            nums(&parsed.operations[0].operands, count)?;
            return lopdf::content::Content::decode(&bytes[i..]).map_err(|_| malformed());
        }
        if start == i || i > 4096 {
            break;
        }
    }
    Err(malformed())
}

fn resource_bytes(dict: &Dictionary, depth: usize) -> Result<usize, ImportError> {
    if depth > 16 {
        return Err(ImportError::LimitExceeded("pdf.resource_depth"));
    }
    fn value(o: &Object, depth: usize) -> Result<usize, ImportError> {
        if depth > 16 {
            return Err(ImportError::LimitExceeded("pdf.resource_depth"));
        }
        Ok(match o {
            Object::Name(b) | Object::String(b, _) => b.len().saturating_add(64),
            Object::Array(a) => {
                let mut size = 64;
                for o in a {
                    size = usize::saturating_add(size, value(o, depth + 1)?);
                }
                size
            }
            Object::Dictionary(d) => resource_bytes(d, depth + 1)?,
            Object::Stream(s) => resource_bytes(&s.dict, depth + 1)?
                .saturating_add(s.content.len())
                .saturating_add(64),
            _ => 64,
        })
    }
    let mut size = 64usize;
    for (key, o) in dict.iter() {
        size = size
            .saturating_add(key.len())
            .saturating_add(value(o, depth + 1)?);
    }
    Ok(size)
}
