//! Bounded embedded TrueType / CID OpenType CFF fonts and Encoding CMaps.
use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
#[derive(Clone)]
pub(super) struct Font {
    pub data: Arc<[u8]>,
    pub index: u32,
    pub outlines: Option<font_program::Outlines>,
    pub type3: Option<type3::Type3>,
    pub encoding: Option<cmap::CMap>,
    pub gids: Vec<Option<u16>>,
    pub widths: HashMap<u16, f32>,
    pub default_width: f32,
    pub vertical: Option<VerticalMetrics>,
}
#[derive(Clone)]
pub(super) struct VerticalMetrics {
    default: [f32; 2],
    values: HashMap<u16, [f32; 3]>,
}
impl VerticalMetrics {
    pub fn glyph(&self, code: u16, width: f32) -> [f32; 3] {
        self.values
            .get(&code)
            .copied()
            .unwrap_or([self.default[1], width / 2., self.default[0]])
    }
}
fn vertical_metrics(
    doc: &lopdf::Document,
    dict: &Dictionary,
) -> Result<VerticalMetrics, ImportError> {
    let default = if let Ok(value) = dict.get(b"DW2") {
        let array = resolve(doc, value)?.as_array().map_err(|_| malformed())?;
        if array.len() != 2 {
            return Err(malformed());
        }
        [
            num(resolve(doc, &array[0])?)?,
            num(resolve(doc, &array[1])?)?,
        ]
    } else {
        [880., -1000.]
    };
    let mut values = HashMap::new();
    if let Ok(value) = dict.get(b"W2") {
        let array = resolve(doc, value)?.as_array().map_err(|_| malformed())?;
        if array.len() > 65536 {
            return Err(ImportError::LimitExceeded("pdf.font_widths"));
        }
        let mut i = 0;
        let mut assignments = 0usize;
        while i < array.len() {
            let start = code(resolve(doc, &array[i])?)?;
            i += 1;
            let next = resolve(doc, array.get(i).ok_or_else(malformed)?)?;
            i += 1;
            if let Ok(triples) = next.as_array() {
                if triples.len() % 3 != 0 || usize::from(start) + triples.len() / 3 > 65536 {
                    return Err(malformed());
                }
                assignments += triples.len() / 3;
                if assignments > 65536 {
                    return Err(ImportError::LimitExceeded("pdf.font_widths"));
                }
                for (j, triple) in triples.as_chunks::<3>().0.iter().enumerate() {
                    values.insert(
                        start + j as u16,
                        [
                            num(resolve(doc, &triple[0])?)?,
                            num(resolve(doc, &triple[1])?)?,
                            num(resolve(doc, &triple[2])?)?,
                        ],
                    );
                }
            } else {
                let end = code(next)?;
                if end < start {
                    return Err(malformed());
                }
                assignments += usize::from(end - start) + 1;
                if assignments > 65536 {
                    return Err(ImportError::LimitExceeded("pdf.font_widths"));
                }
                let triple = [
                    num(resolve(doc, array.get(i).ok_or_else(malformed)?)?)?,
                    num(resolve(doc, array.get(i + 1).ok_or_else(malformed)?)?)?,
                    num(resolve(doc, array.get(i + 2).ok_or_else(malformed)?)?)?,
                ];
                i += 3;
                for c in start..=end {
                    values.insert(c, triple);
                }
            }
        }
    }
    Ok(VerticalMetrics { default, values })
}
fn code(object: &Object) -> Result<u16, ImportError> {
    let n = num(object)?;
    if n.fract() != 0. || !(0. ..=65535.).contains(&n) {
        return Err(malformed());
    }
    Ok(n as u16)
}
/// Adapt a standalone CID CFF to the outline parser's sfnt container.
/// PDF widths still come from W/DW, never from the synthetic metrics tables.
fn wrap_cid_cff(cff: Vec<u8>) -> Result<Vec<u8>, ImportError> {
    let table =
        ttf_parser::cff::Table::parse(&cff).ok_or(ImportError::Unsupported("pdf.font_program"))?;
    let m = table.matrix();
    if m.sx != 0.001 || m.sy != 0.001 || m.kx != 0. || m.ky != 0. || m.tx != 0. || m.ty != 0. {
        return Err(ImportError::Unsupported("pdf.font_matrix"));
    }
    let count = table.number_of_glyphs();
    for gid in 0..count {
        if table.glyph_cid(ttf_parser::GlyphId(gid)).is_none() {
            return Err(ImportError::Unsupported("pdf.font_program"));
        }
    }
    let mut head = vec![0u8; 54];
    head[..4].copy_from_slice(&0x00010000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5f0f3cf5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[..4].copy_from_slice(&0x00010000u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
    let mut maxp = 0x00005000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&count.to_be_bytes());
    let mut out = vec![0u8; 76];
    out[..4].copy_from_slice(b"OTTO");
    out[4..6].copy_from_slice(&4u16.to_be_bytes());
    out[6..8].copy_from_slice(&64u16.to_be_bytes());
    out[8..10].copy_from_slice(&2u16.to_be_bytes());
    let mut head_offset = 0;
    for (i, (tag, mut data)) in [
        (b"CFF ", cff),
        (b"head", head),
        (b"hhea", hhea),
        (b"maxp", maxp),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = out.len();
        let len = data.len();
        if tag == b"head" {
            head_offset = offset;
        }
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        let sum = data
            .as_chunks::<4>()
            .0
            .iter()
            .fold(0u32, |sum, b| sum.wrapping_add(u32::from_be_bytes(*b)));
        let r = 12 + i * 16;
        out[r..r + 4].copy_from_slice(tag);
        out[r + 4..r + 8].copy_from_slice(&sum.to_be_bytes());
        out[r + 8..r + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[r + 12..r + 16].copy_from_slice(&(len as u32).to_be_bytes());
        out.extend(data);
    }
    let sum = out
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0u32, |sum, b| sum.wrapping_add(u32::from_be_bytes(*b)));
    out[head_offset + 8..head_offset + 12]
        .copy_from_slice(&0xB1B0_AFBAu32.wrapping_sub(sum).to_be_bytes());
    Ok(out)
}

impl Interpreter<'_> {
    pub(super) fn font(
        &mut self,
        name: &[u8],
        resources: &Dictionary,
    ) -> Result<Option<Arc<Font>>, ImportError> {
        let dict = resource(self.doc, resources, b"Font", name)?
            .as_dict()
            .map_err(|_| malformed())?;
        self.font_dictionary(dict)
    }
    pub(super) fn font_dictionary(
        &mut self,
        dict: &Dictionary,
    ) -> Result<Option<Arc<Font>>, ImportError> {
        let key = dict as *const _ as usize;
        if let Some(font) = self.font_cache.get(&key) {
            return Ok(font.clone());
        }
        if self.font_cache.len() >= 128 {
            return Err(ImportError::LimitExceeded("pdf.font_count"));
        }
        let result = self.font_inner(dict);
        let font = match result {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                None
            }
            other => Some(Arc::new(other?)),
        };
        self.font_cache.insert(key, font.clone());
        Ok(font)
    }
    fn font_inner(&mut self, root: &Dictionary) -> Result<Font, ImportError> {
        let subtype = root
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map_err(|_| malformed())?;
        if subtype == b"Type3" {
            return self.type3_font(root);
        }
        let cid = subtype == b"Type0";
        let encoding = if cid {
            let object = resolve(self.doc, root.get(b"Encoding").map_err(|_| malformed())?)?;
            Some(if let Ok(name) = object.as_name() {
                match name {
                    b"Identity-H" => cmap::CMap::identity(false),
                    b"Identity-V" => cmap::CMap::identity(true),
                    _ => return Err(ImportError::Unsupported("pdf.font_encoding")),
                }
            } else {
                let stream = object.as_stream().map_err(|_| malformed())?;
                let bytes = stream
                    .get_plain_content_with_limit(1024 * 1024)
                    .map_err(|_| ImportError::LimitExceeded("pdf.font_mapping"))?;
                if bytes.len() > self.remaining {
                    return Err(ImportError::LimitExceeded("pdf.content_budget"));
                }
                self.remaining -= bytes.len();
                let parent = stream
                    .dict
                    .get(b"UseCMap")
                    .ok()
                    .map(|v| resolve(self.doc, v))
                    .transpose()?;
                let parent = parent
                    .map(|v| {
                        v.as_name()
                            .map_err(|_| ImportError::Unsupported("pdf.font_encoding"))
                    })
                    .transpose()?;
                let mut map = cmap::parse(&bytes, parent)?;
                let retained = map.memory_bytes();
                if retained > self.remaining {
                    return Err(ImportError::LimitExceeded("pdf.content_budget"));
                }
                self.remaining -= retained;
                if let Ok(mode) = stream.dict.get(b"WMode") {
                    let mode = num(resolve(self.doc, mode)?)?;
                    if ![0., 1.].contains(&mode) {
                        return Err(malformed());
                    }
                    map.vertical = mode == 1.;
                }
                map
            })
        } else {
            None
        };
        let dict = if cid {
            let descendants = resolve(
                self.doc,
                root.get(b"DescendantFonts").map_err(|_| malformed())?,
            )?
            .as_array()
            .map_err(|_| malformed())?;
            if descendants.len() != 1 {
                return Err(malformed());
            }
            let dict = resolve(self.doc, &descendants[0])?
                .as_dict()
                .map_err(|_| malformed())?;
            if ![b"CIDFontType2".as_slice(), b"CIDFontType0"].contains(
                &dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map_err(|_| malformed())?,
            ) {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            dict
        } else {
            if ![b"TrueType".as_slice(), b"Type1", b"MMType1"].contains(&subtype) {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            root
        };
        let vertical = if encoding.as_ref().is_some_and(|m| m.vertical) {
            Some(vertical_metrics(self.doc, dict)?)
        } else {
            None
        };
        let descriptor = dict
            .get(b"FontDescriptor")
            .ok()
            .map(|v| resolve(self.doc, v))
            .transpose()?
            .map(|v| v.as_dict().map_err(|_| malformed()))
            .transpose()?;
        let embedded = descriptor.and_then(|d| {
            d.get(b"FontFile2")
                .ok()
                .or_else(|| d.get(b"FontFile3").ok())
                .or_else(|| d.get(b"FontFile").ok())
        });
        let (data, index, substituted) = if let Some(program) = embedded {
            let program = resolve(self.doc, program)?
                .as_stream()
                .map_err(|_| malformed())?;
            let kind = program
                .dict
                .get(b"Subtype")
                .ok()
                .map(|v| v.as_name().map_err(|_| malformed()))
                .transpose()?;
            let raw_cid = kind == Some(b"CIDFontType0C")
                && cid
                && dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"CIDFontType0");
            if kind.is_some() && kind != Some(b"OpenType") && kind != Some(b"Type1C") && !raw_cid {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            let data = program
                .get_plain_content_with_limit(4 * 1024 * 1024)
                .map_err(|_| ImportError::LimitExceeded("pdf.font_program"))?;
            let data = if raw_cid {
                match wrap_cid_cff(data.clone()) {
                    Ok(wrapped) => wrapped,
                    Err(ImportError::Unsupported(_)) => data,
                    Err(e) => return Err(e),
                }
            } else {
                data
            };
            let names: Vec<_> = [
                root.get(b"BaseFont").ok(),
                dict.get(b"BaseFont").ok(),
                descriptor.and_then(|d| d.get(b"FontName").ok()),
            ]
            .into_iter()
            .flatten()
            .map(|o| resolve(self.doc, o))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|o| o.as_name().ok())
            .collect();
            let index = font_collection::face_index(&data, &names)?;
            (data, index, false)
        } else {
            if cid || descriptor.is_some_and(|d| d.get(b"FontFile").is_ok()) {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            self.conversion("pdf.font_substitution", CompatibilityTier::C)?;
            let name = std::str::from_utf8(
                root.get(b"BaseFont")
                    .and_then(Object::as_name)
                    .map_err(|_| malformed())?,
            )
            .map_err(|_| malformed())?;
            let base = name.split_once('+').map_or(name, |(_, n)| n);
            if ["Symbol", "ZapfDingbats"].contains(&base) {
                return Err(ImportError::Unsupported("pdf.font_encoding"));
            }
            let family = if base.starts_with("Helvetica") {
                "Arial"
            } else if base.starts_with("Times") {
                "Times New Roman"
            } else if base.starts_with("Courier") {
                "Courier New"
            } else {
                base
            };
            static DB: OnceLock<usvg::fontdb::Database> = OnceLock::new();
            let db = DB.get_or_init(|| {
                let mut db = usvg::fontdb::Database::new();
                lumapaint_fonts::load_system_fonts(&mut db);
                db
            });
            use usvg::fontdb::{Family, Query, Style, Weight};
            let families = [Family::Name(family), Family::SansSerif];
            let id = db
                .faces()
                .find(|face| face.post_script_name == base)
                .map(|face| face.id)
                .or_else(|| {
                    db.query(&Query {
                        families: &families,
                        weight: if base.contains("Bold") {
                            Weight::BOLD
                        } else {
                            Weight::NORMAL
                        },
                        style: if base.contains("Italic") || base.contains("Oblique") {
                            Style::Italic
                        } else {
                            Style::Normal
                        },
                        ..Default::default()
                    })
                })
                .ok_or(ImportError::Unsupported("pdf.font_program"))?;
            let (data, index) = db
                .with_face_data(id, |data, index| {
                    if data.len() <= 64 * 1024 * 1024 {
                        Some((data.to_vec(), index))
                    } else {
                        None
                    }
                })
                .flatten()
                .ok_or(ImportError::LimitExceeded("pdf.font_program"))?;
            (data, index, true)
        };
        if data.len() > self.remaining {
            return Err(ImportError::LimitExceeded("pdf.content_budget"));
        }
        self.remaining -= data.len();
        let face = font_program::Program::parse(&data, index)?;
        let mut widths = HashMap::new();
        let default_width = if cid {
            dict.get(b"DW").ok().map(num).transpose()?.unwrap_or(1000.)
        } else {
            descriptor
                .and_then(|d| d.get(b"MissingWidth").ok())
                .map(num)
                .transpose()?
                .unwrap_or(0.)
        };
        if !cid && !substituted && face.native().is_some_and(|f| f.tables().glyf.is_some()) {
            let symbolic = descriptor
                .and_then(|d| d.get(b"Flags").ok())
                .map(|o| resolve(self.doc, o))
                .transpose()?
                .map(|o| o.as_i64().map_err(|_| malformed()))
                .transpose()?
                .is_some_and(|f| f & 4 != 0);
            if symbolic || root.get(b"Encoding").is_err() {
                let gids = face.truetype_builtin_mapping()?;
                self.simple_widths(dict, &face, &gids, false, &mut widths)?;
                return Ok(Font {
                    data: data.into(),
                    index,
                    outlines: None,
                    type3: None,
                    encoding: None,
                    gids,
                    widths,
                    default_width,
                    vertical,
                });
            }
        }
        let gids = if cid {
            if let Ok(w) = dict.get(b"W") {
                let w = resolve(self.doc, w)?.as_array().map_err(|_| malformed())?;
                let mut i = 0;
                let mut assignments = 0usize;
                if w.len() > 65536 {
                    return Err(ImportError::LimitExceeded("pdf.font_widths"));
                }
                while i < w.len() {
                    let start = code(&w[i])?;
                    i += 1;
                    let next = resolve(self.doc, w.get(i).ok_or_else(malformed)?)?;
                    i += 1;
                    if let Ok(array) = next.as_array() {
                        if usize::from(start) + array.len() > 65536 {
                            return Err(malformed());
                        }
                        assignments += array.len();
                        if assignments > 65536 {
                            return Err(ImportError::LimitExceeded("pdf.font_widths"));
                        }
                        for (j, v) in array.iter().enumerate() {
                            widths.insert(start + j as u16, num(v)?);
                        }
                    } else {
                        let end = code(next)?;
                        let width = num(w.get(i).ok_or_else(malformed)?)?;
                        i += 1;
                        if end < start {
                            return Err(malformed());
                        }
                        assignments += usize::from(end - start) + 1;
                        if assignments > 65536 {
                            return Err(ImportError::LimitExceeded("pdf.font_widths"));
                        }
                        for c in start..=end {
                            widths.insert(c, width);
                        }
                    }
                    if widths.len() > 65536 || w.len() > 65536 {
                        return Err(ImportError::LimitExceeded("pdf.font_widths"));
                    }
                }
            }
            if dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"CIDFontType0") {
                if dict.get(b"CIDToGIDMap").is_ok() {
                    return Err(ImportError::Unsupported("pdf.font_mapping"));
                }
                let mut pairs = Vec::new();
                let mut max = 0u16;
                for gid in 0..face.number_of_glyphs() {
                    let Some(cid) = face.cid(gid) else {
                        if face.native().is_none() {
                            continue;
                        }
                        return Err(ImportError::Unsupported("pdf.font_mapping"));
                    };
                    max = max.max(cid);
                    pairs.push((cid, gid));
                }
                let mut mapping = vec![None; usize::from(max) + 1];
                for (cid, gid) in pairs {
                    if mapping[usize::from(cid)].replace(gid).is_some() {
                        return Err(malformed());
                    }
                }
                mapping
            } else {
                if face.native().is_none_or(|f| f.tables().glyf.is_none()) {
                    return Err(ImportError::Unsupported("pdf.font_program"));
                }
                let map = dict
                    .get(b"CIDToGIDMap")
                    .ok()
                    .map(|v| resolve(self.doc, v))
                    .transpose()?;
                if let Some(map) = map.filter(|m| m.as_name().ok() != Some(b"Identity")) {
                    let stream = map.as_stream().map_err(|_| malformed())?;
                    let data = stream
                        .get_plain_content_with_limit(131072)
                        .map_err(|_| ImportError::LimitExceeded("pdf.font_mapping"))?;
                    if data.len() % 2 != 0 {
                        return Err(malformed());
                    }
                    data.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| Some(u16::from_be_bytes(*p)))
                        .collect()
                } else {
                    (0..face.number_of_glyphs()).map(Some).collect()
                }
            }
        } else {
            let mut mapping = charmap(self.doc, b"StandardEncoding")?;
            if let Ok(encoding) = root.get(b"Encoding") {
                let encoding = resolve(self.doc, encoding)?;
                let (base, differences) = if let Ok(dict) = encoding.as_dict() {
                    (
                        dict.get(b"BaseEncoding")
                            .ok()
                            .map(|v| resolve(self.doc, v))
                            .transpose()?
                            .and_then(|v| v.as_name().ok())
                            .unwrap_or(b"StandardEncoding"),
                        dict.get(b"Differences").ok(),
                    )
                } else {
                    (encoding.as_name().map_err(|_| malformed())?, None)
                };
                mapping = charmap(self.doc, base)?;
                let mut gids: Vec<_> = mapping
                    .iter()
                    .map(|g| g.and_then(|c| face.glyph_index(c)).map(|g| g.0))
                    .collect();
                if let Some(differences) = differences {
                    let mut c = 256usize;
                    for value in resolve(self.doc, differences)?
                        .as_array()
                        .map_err(|_| malformed())?
                    {
                        if let Ok(name) = value.as_name() {
                            if c > 255 {
                                return Err(malformed());
                            }
                            gids[c] = std::str::from_utf8(name)
                                .ok()
                                .and_then(|n| face.glyph_index_by_name(n))
                                .or_else(|| {
                                    unicode_name(self.doc, name).and_then(|c| face.glyph_index(c))
                                })
                                .map(|g| g.0);
                            c += 1;
                        } else {
                            c = usize::from(code(value)?);
                            if c > 255 {
                                return Err(malformed());
                            }
                        }
                    }
                }
                self.simple_widths(dict, &face, &gids, substituted, &mut widths)?;
                let outlines = face.collect(&gids, &mut self.remaining)?;
                return Ok(Font {
                    data: data.into(),
                    index,
                    outlines,
                    type3: None,
                    encoding: None,
                    gids,
                    widths,
                    default_width,
                    vertical,
                });
            }
            if !substituted && face.native().is_some_and(|f| f.tables().cff.is_none()) {
                return Err(ImportError::Unsupported("pdf.font_encoding"));
            }
            if !substituted {
                let gids: Vec<_> = (0..256).map(|c| face.builtin_index(c)).collect();
                self.simple_widths(dict, &face, &gids, false, &mut widths)?;
                let outlines = face.collect(&gids, &mut self.remaining)?;
                return Ok(Font {
                    data: data.into(),
                    index,
                    outlines,
                    type3: None,
                    encoding: None,
                    gids,
                    widths,
                    default_width,
                    vertical,
                });
            }
            let gids: Vec<_> = mapping
                .iter()
                .map(|g| g.and_then(|c| face.glyph_index(c)).map(|g| g.0))
                .collect();
            self.simple_widths(dict, &face, &gids, substituted, &mut widths)?;
            gids
        };
        let outlines = face.collect(&gids, &mut self.remaining)?;
        Ok(Font {
            data: data.into(),
            index,
            outlines,
            type3: None,
            encoding,
            gids,
            widths,
            default_width,
            vertical,
        })
    }
    fn simple_widths(
        &self,
        dict: &Dictionary,
        face: &font_program::Program<'_>,
        gids: &[Option<u16>],
        substituted: bool,
        widths: &mut HashMap<u16, f32>,
    ) -> Result<(), ImportError> {
        if let Ok(array) = dict.get(b"Widths") {
            let array = resolve(self.doc, array)?
                .as_array()
                .map_err(|_| malformed())?;
            let start = code(resolve(
                self.doc,
                dict.get(b"FirstChar").map_err(|_| malformed())?,
            )?)?;
            let end = code(resolve(
                self.doc,
                dict.get(b"LastChar").map_err(|_| malformed())?,
            )?)?;
            if end > 255 || end < start || array.len() != usize::from(end - start) + 1 {
                return Err(malformed());
            }
            for (i, v) in array.iter().enumerate() {
                widths.insert(start + i as u16, num(v)?);
            }
        } else if substituted {
            for (c, gid) in gids.iter().enumerate() {
                if let Some(gid) = gid {
                    widths.insert(
                        c as u16,
                        f32::from(
                            face.glyph_hor_advance(ttf_parser::GlyphId(*gid))
                                .unwrap_or(0),
                        ) * 1000.
                            / f32::from(face.units_per_em()),
                    );
                }
            }
        } else {
            return Err(ImportError::Unsupported("pdf.font_widths"));
        }
        Ok(())
    }
}

fn single_char(text: String) -> Option<char> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        Some(c)
    } else {
        None
    }
}
fn charmap(doc: &lopdf::Document, name: &[u8]) -> Result<Vec<Option<char>>, ImportError> {
    if ![
        b"StandardEncoding".as_slice(),
        b"WinAnsiEncoding",
        b"MacRomanEncoding",
    ]
    .contains(&name)
    {
        return Err(ImportError::Unsupported("pdf.font_encoding"));
    }
    let dict = lopdf::dictionary! {"Type"=>"Font","Encoding"=>Object::Name(name.to_vec())};
    let encoding = dict
        .get_font_encoding_with_limit(doc, 65536)
        .map_err(|_| malformed())?;
    (0..=255)
        .map(|n| {
            encoding
                .bytes_to_string(&[n])
                .map(single_char)
                .map_err(|_| malformed())
        })
        .collect()
}
fn unicode_name(doc: &lopdf::Document, name: &[u8]) -> Option<char> {
    let dict = lopdf::dictionary! {"Type"=>"Font","Encoding"=>lopdf::dictionary! {"BaseEncoding"=>"StandardEncoding","Differences"=>vec![0.into(),Object::Name(name.to_vec())]}};
    single_char(
        dict.get_font_encoding_with_limit(doc, 65536)
            .ok()?
            .bytes_to_string(&[0])
            .ok()?,
    )
}

#[cfg(test)]
pub(super) fn fixture_resources(doc: &mut lopdf::Document, encoding: Object) -> Dictionary {
    let program = doc.add_object(lopdf::Stream::new(
        lopdf::dictionary! {},
        include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf").to_vec(),
    ));
    let descriptor = doc.add_object(lopdf::dictionary! {"FontFile2"=>program,"Flags"=>32});
    lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>lopdf::dictionary! {"Type"=>"Font","Subtype"=>"TrueType","FontDescriptor"=>descriptor,"Encoding"=>encoding,"FirstChar"=>32,"LastChar"=>67,"Widths"=>vec![600.into();36]}}}
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parser(doc: &lopdf::Document) -> Interpreter<'_> {
        Interpreter {
            doc,
            body: String::new(),
            defs: String::new(),
            issues: vec![],
            remaining: LIMIT,
            operations: 0,
            serial: 0,
            allow_lossy: true,
            page_bounds: [0., 0., 100., 80.],
            font_cache: Default::default(),
            text_glyphs: 0,
            glyph_depth: 0,
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
        }
    }
    fn builtin_truetype(platform: u16, prefix: u16, ambiguous: bool) -> Vec<u8> {
        let mut data = include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf").to_vec();
        let mut segments = vec![(prefix + 65, prefix + 66, 1u16.wrapping_sub(prefix + 65))];
        if ambiguous {
            segments.insert(0, (65, 66, 2u16.wrapping_sub(65)));
        }
        segments.push((65535, 65535, 1));
        let n = segments.len() as u16;
        let mut sub = Vec::new();
        let power = 2;
        for v in [4u16, 16 + 8 * n, 0, 2 * n, 2 * power, 1, 2 * n - 2 * power] {
            sub.extend(v.to_be_bytes());
        }
        for (_, end, _) in &segments {
            sub.extend(end.to_be_bytes());
        }
        sub.extend([0, 0]);
        for (start, _, _) in &segments {
            sub.extend(start.to_be_bytes());
        }
        for (_, _, delta) in &segments {
            sub.extend(delta.to_be_bytes());
        }
        sub.resize(sub.len() + segments.len() * 2, 0);
        let mut cmap = vec![0, 0, 0, 1];
        cmap.extend(platform.to_be_bytes());
        cmap.extend([0, 0]);
        cmap.extend(12u32.to_be_bytes());
        cmap.extend(sub);
        let count = u16::from_be_bytes(data[4..6].try_into().unwrap());
        for i in 0..usize::from(count) {
            let r = 12 + i * 16;
            if &data[r..r + 4] == b"cmap" {
                let offset = u32::from_be_bytes(data[r + 8..r + 12].try_into().unwrap()) as usize;
                let length = u32::from_be_bytes(data[r + 12..r + 16].try_into().unwrap()) as usize;
                assert!(cmap.len() <= length);
                data[offset..offset + length].fill(0);
                data[offset..offset + cmap.len()].copy_from_slice(&cmap);
                data[r + 12..r + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
            }
        }
        data
    }
    #[test]
    fn symbolic_and_missing_encoding_truetype_keep_builtin_glyphs() {
        fn render(data: Vec<u8>, encoding: Option<Object>, symbolic: bool) -> String {
            let mut doc = lopdf::Document::new();
            let mut resources =
                fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
            let font = resources
                .get_mut(b"Font")
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .get_mut(b"F")
                .unwrap()
                .as_dict_mut()
                .unwrap();
            match encoding {
                Some(v) => font.set("Encoding", v),
                None => {
                    font.remove(b"Encoding");
                }
            }
            let descriptor = font.get(b"FontDescriptor").unwrap().as_reference().unwrap();
            let desc = doc
                .get_object_mut(descriptor)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            desc.set("Flags", if symbolic { 4 } else { 32 });
            let program = desc.get(b"FontFile2").unwrap().as_reference().unwrap();
            *doc.get_object_mut(program).unwrap() =
                Object::Stream(lopdf::Stream::new(Dictionary::new(), data));
            let mut parser = parser(&doc);
            parser
                .content(
                    b"BT /F 20 Tf 1 0 0 1 10 30 Tm (AB) Tj ET",
                    &resources,
                    State::default(),
                    0,
                )
                .unwrap();
            assert!(!parser
                .issues
                .iter()
                .any(|i| i.code == "pdf.font_encoding" || i.code == "pdf.glyph_missing"));
            parser.body
        }
        let expected = render(
            include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf").to_vec(),
            Some(Object::Name(b"WinAnsiEncoding".to_vec())),
            false,
        );
        for prefix in [0, 0xf000, 0xf100, 0xf200] {
            let data = builtin_truetype(3, prefix, false);
            assert_eq!(render(data.clone(), None, false), expected);
            // Symbolic fonts must use their built-in cmap even if an
            // unrelated PDF Encoding is present (ISO §9.6.6.4).
            assert_eq!(
                render(data, Some(Object::Name(b"Ignored".to_vec())), true),
                expected
            );
        }
        assert_eq!(render(builtin_truetype(1, 0, false), None, false), expected);
        let data = builtin_truetype(3, 0xf000, true);
        let program = font_program::Program::parse(&data, 0).unwrap();
        assert!(matches!(
            program.truetype_builtin_mapping(),
            Err(ImportError::Unsupported("pdf.font_mapping"))
        ));
    }
    #[test]
    fn embedded_ttc_selected_face_matches_its_standalone_outlines() {
        let original = include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf");
        let second = font_collection::rename_and_scale(original, "LPPDFBest", 2000);
        fn render(data: Vec<u8>) -> (String, u32) {
            let mut doc = lopdf::Document::new();
            let mut resources =
                fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
            let font = resources
                .get_mut(b"Font")
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .get_mut(b"F")
                .unwrap()
                .as_dict_mut()
                .unwrap();
            font.set("BaseFont", Object::Name(b"ABCDEF+LPPDFBest".to_vec()));
            let descriptor = font.get(b"FontDescriptor").unwrap().as_reference().unwrap();
            let program = doc
                .get_object(descriptor)
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"FontFile2")
                .unwrap()
                .as_reference()
                .unwrap();
            *doc.get_object_mut(program).unwrap() =
                Object::Stream(lopdf::Stream::new(Dictionary::new(), data));
            let mut parser = parser(&doc);
            let index = parser.font(b"F", &resources).unwrap().unwrap().index;
            parser
                .content(
                    b"BT /F 20 Tf 1 0 0 1 10 30 Tm (ABC) Tj ET",
                    &resources,
                    State::default(),
                    0,
                )
                .unwrap();
            assert!(!parser.body.is_empty());
            (parser.body, index)
        }
        let (expected, index) = render(second.clone());
        assert_eq!(index, 0);
        let (actual, index) = render(font_collection::collection(&[original.to_vec(), second]));
        assert_eq!(index, 1);
        assert_eq!(actual, expected);
    }
    #[test]
    fn embedded_otc_selected_cid_face_matches_standalone_pdf() {
        let original = include_bytes!("../../../tests/fixtures/lp-japanese-cid.otf");
        let second = font_collection::rename_and_scale(original, "LPJapaneseCIDBest", 2000);
        fn render(program: Vec<u8>) -> String {
            let mut pdf = lopdf::Document::load_mem(include_bytes!(
                "../../../tests/fixtures/lp-japanese-cid-horizontal.pdf"
            ))
            .unwrap();
            for object in pdf.objects.values_mut() {
                if let Ok(dict) = object.as_dict_mut() {
                    if dict.get(b"BaseFont").is_ok() {
                        dict.set(
                            "BaseFont",
                            Object::Name(b"ABCDEF+LPJapaneseCIDBest".to_vec()),
                        );
                    }
                    if dict.get(b"FontName").is_ok() {
                        dict.set("FontName", Object::Name(b"LPJapaneseCIDBest".to_vec()));
                    }
                }
                if let Ok(stream) = object.as_stream_mut() {
                    if stream
                        .dict
                        .get(b"Subtype")
                        .ok()
                        .and_then(|o| o.as_name().ok())
                        == Some(b"OpenType")
                    {
                        stream.content = program.clone();
                        stream.dict.set("Length", program.len() as i64);
                        stream.dict.remove(b"Filter");
                    }
                }
            }
            let mut bytes = vec![];
            pdf.save_to(&mut bytes).unwrap();
            let result = crate::pdf::read(
                &bytes,
                crate::io::ReadOptions {
                    allow_lossy: true,
                    raster_dpi: 72,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(!result
                .report
                .issues
                .iter()
                .any(|i| i.code == "pdf.font_collection_face"
                    || i.code == "pdf.font_program"
                    || i.code == "pdf.glyph_missing"));
            let ReadContent::Vector(document) = result.content else {
                panic!()
            };
            let source = document.svg_layers().next().unwrap().source.to_string();
            source
        }
        assert_eq!(
            render(second.clone()),
            render(font_collection::collection(&[original.to_vec(), second]))
        );
    }
    #[test]
    fn type1c_pdf_embedded_program_preserves_differences_and_widths() {
        let mut doc = lopdf::Document::new();
        let program = doc.add_object(lopdf::Stream::new(
            lopdf::dictionary! {"Subtype"=>"Type1C"},
            font_program::test_cff(),
        ));
        let descriptor = doc.add_object(lopdf::dictionary! {"FontFile3"=>program});
        let font = lopdf::dictionary! {"Subtype"=>"Type1","FontDescriptor"=>descriptor,"Encoding"=>lopdf::dictionary! {"Differences"=>vec![66.into(),Object::Name(b"A".to_vec())]},"FirstChar"=>66,"LastChar"=>66,"Widths"=>vec![900.into()]};
        let resources = lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>font}};
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 1 0 0 1 10 30 Tm (BB) Tj ET",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert!(p.body.contains("M12 32"));
        assert!(p.body.contains("M30 32"));
        assert!(!p
            .issues
            .iter()
            .any(|i| i.code == "pdf.font_program" || i.code == "pdf.glyph_missing"));
    }
    #[test]
    fn legacy_type1_pdf_differences_and_pdf_widths_control_positions() {
        let mut doc = lopdf::Document::new();
        let program = doc.add_object(lopdf::Stream::new(
            Dictionary::new(),
            font_program::test_type1(),
        ));
        let descriptor = doc.add_object(lopdf::dictionary! {"FontFile"=>program});
        let font = lopdf::dictionary! {"Subtype"=>"Type1","FontDescriptor"=>descriptor,"Encoding"=>lopdf::dictionary! {"Differences"=>vec![66.into(),Object::Name(b"A".to_vec())]},"FirstChar"=>66,"LastChar"=>66,"Widths"=>vec![900.into()]};
        let resources = lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>font}};
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 1 0 0 1 10 30 Tm (BB) Tj ET",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert!(p.body.contains("M12 32"), "{}", p.body);
        assert!(p.body.contains("M30 32"), "{}", p.body);
        assert!(!p.issues.iter().any(|i| i.code == "pdf.font_program"
            || i.code == "pdf.font_substitution"
            || i.code == "pdf.glyph_missing"));
    }
    #[test]
    fn type3_graphics_use_font_matrix_and_pdf_widths_without_substitution() {
        let mut doc = lopdf::Document::new();
        let glyph = doc.add_object(lopdf::Stream::new(
            Dictionary::new(),
            b"600 0 0 0 300 500 d1 100 100 200 400 re f".to_vec(),
        ));
        let font = lopdf::dictionary! {"Subtype"=>"Type3","FontMatrix"=>vec![0.001.into(),0.into(),0.into(),0.001.into(),0.into(),0.into()],"CharProcs"=>lopdf::dictionary! {"A"=>glyph},"Encoding"=>lopdf::dictionary! {"Differences"=>vec![65.into(),Object::Name(b"A".to_vec())]},"FirstChar"=>65,"LastChar"=>65,"Widths"=>vec![900.into()]};
        let resources = lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>font}};
        let mut limited = parser(&doc);
        limited.remaining = 1;
        assert!(matches!(
            limited.font(b"F", &resources),
            Err(ImportError::LimitExceeded("pdf.content_budget"))
        ));
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 1 0 0 1 10 30 Tm (AA) Tj ET",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert!(
            p.body.contains("matrix(0.020000001 0 0 0.020000001 10 30)"),
            "{}",
            p.body
        );
        assert!(p.body.contains(" 28 30)"), "{}", p.body);
        assert!(!p
            .issues
            .iter()
            .any(|i| i.code == "pdf.font_program" || i.code == "pdf.font_substitution"));
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 7 Tr (A) Tj ET 1 1 10 10 re f",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert_eq!(p.body.matches("<path").count(), 2);
        assert!(p.defs.is_empty());
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 3 Tr (A) Tj ET",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert!(p.body.is_empty());
        *doc.get_object_mut(glyph).unwrap() = Object::Stream(lopdf::Stream::new(
            Dictionary::new(),
            b"600 0 d0 BT /F 20 Tf (A) Tj ET".to_vec(),
        ));
        let mut p = parser(&doc);
        assert!(matches!(
            p.content(b"BT /F 20 Tf (A) Tj ET", &resources, State::default(), 0),
            Err(ImportError::LimitExceeded("pdf.glyph_depth"))
        ));
    }
    #[test]
    fn embedded_font_cache_avoids_repeat_decode_and_differences_select_actual_glyphs() {
        let mut doc = lopdf::Document::new();
        let encoding = lopdf::dictionary! {"BaseEncoding"=>"WinAnsiEncoding","Differences"=>vec![65.into(),Object::Name(b"B".to_vec()),Object::Name(b"C".to_vec())]};
        let mut resources = fixture_resources(&mut doc, encoding.into());
        let widths = doc.add_object(vec![600.into(); 36]);
        resources
            .get_mut(b"Font")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"F")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Widths", widths);
        let mut p = parser(&doc);
        let first = p.font(b"F", &resources).unwrap().unwrap();
        let remaining = p.remaining;
        let second = p.font(b"F", &resources).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(p.remaining, remaining);
        assert_eq!(first.gids[65], Some(2));
        assert_eq!(first.gids[66], Some(5));
        assert_eq!(first.widths[&65], 600.);
    }
    #[test]
    fn unsupported_encoding_is_reported_and_not_cached_as_a_substitute() {
        let mut doc = lopdf::Document::new();
        let resources = fixture_resources(&mut doc, Object::Name(b"Unsupported".to_vec()));
        let mut p = parser(&doc);
        p.allow_lossy = false;
        assert!(matches!(
            p.font(b"F", &resources),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        p.allow_lossy = true;
        assert!(p.font(b"F", &resources).unwrap().is_none());
        assert_eq!(p.issues[0].code, "pdf.font_encoding");
    }
    #[test]
    fn font_count_and_invalid_widths_are_rejected() {
        let mut doc = lopdf::Document::new();
        let mut resources = fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
        let mut p = parser(&doc);
        for key in 0..128 {
            p.font_cache.insert(key, None);
        }
        assert!(matches!(
            p.font(b"F", &resources),
            Err(ImportError::LimitExceeded("pdf.font_count"))
        ));
        resources
            .get_mut(b"Font")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"F")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("LastChar", 300);
        assert!(matches!(
            parser(&doc).font(b"F", &resources),
            Err(ImportError::Malformed(_))
        ));
    }
    #[test]
    fn arbitrary_cmap_stream_is_reported_as_unsupported() {
        let mut doc = lopdf::Document::new();
        let cmap = doc.add_object(lopdf::Stream::new(
            Dictionary::new(),
            b"/WMode 1 def".to_vec(),
        ));
        let resources = lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>lopdf::dictionary! {"Subtype"=>"Type0","Encoding"=>cmap}}};
        let mut p = parser(&doc);
        assert!(p.font(b"F", &resources).unwrap().is_none());
        assert_eq!(p.issues[0].code, "pdf.font_encoding");
    }
    #[test]
    fn vertical_defaults_ranges_indirection_and_limits() {
        let mut doc = lopdf::Document::new();
        let defaults = doc.add_object(vec![900.into(), (-1100).into()]);
        let entries = doc.add_object(vec![
            1.into(),
            vec![(-1200).into(), 300.into(), 800.into()].into(),
            2.into(),
            3.into(),
            (-900).into(),
            400.into(),
            850.into(),
        ]);
        let dict = lopdf::dictionary! {"DW2"=>defaults,"W2"=>entries};
        let metrics = vertical_metrics(&doc, &dict).unwrap();
        assert_eq!(metrics.glyph(0, 600.), [-1100., 300., 900.]);
        assert_eq!(metrics.glyph(1, 600.), [-1200., 300., 800.]);
        assert_eq!(metrics.glyph(3, 600.), [-900., 400., 850.]);
        let mut malformed_dict = Dictionary::new();
        for entries in [
            vec![1.into(), vec![1.into(), 2.into()].into()],
            vec![4.into(), 2.into(), 1.into(), 2.into(), 3.into()],
            vec![65535.into(), vec![1.into(); 6].into()],
        ] {
            malformed_dict.set("W2", entries);
            assert!(matches!(
                vertical_metrics(&doc, &malformed_dict),
                Err(ImportError::Malformed(_))
            ));
        }
        let mut entries = Vec::<Object>::new();
        for _ in 0..2 {
            entries.extend([
                0.into(),
                65535.into(),
                (-1000).into(),
                500.into(),
                880.into(),
            ]);
        }
        malformed_dict.set("W2", entries);
        assert!(matches!(
            vertical_metrics(&doc, &malformed_dict),
            Err(ImportError::LimitExceeded("pdf.font_widths"))
        ));
    }
    #[test]
    fn unembedded_fonts_require_explicit_substitution_consent_before_system_lookup() {
        let doc = lopdf::Document::new();
        let resources = lopdf::dictionary! {"Font"=>lopdf::dictionary! {"F"=>lopdf::dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"}}};
        let mut p = parser(&doc);
        p.allow_lossy = false;
        let Err(ImportError::LossyConversionRequiresConsent(report)) = p.font(b"F", &resources)
        else {
            panic!()
        };
        assert_eq!(report.issues[0].code, "pdf.font_substitution");
    }
}
