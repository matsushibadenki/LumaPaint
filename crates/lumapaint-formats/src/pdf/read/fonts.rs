//! Bounded embedded TrueType / CID OpenType CFF fonts and Identity-H / Identity-V CID fonts.
use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
#[derive(Clone)]
pub(super) struct Font {
    pub data: Arc<[u8]>,
    pub index: u32,
    pub cid: bool,
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
impl Interpreter<'_> {
    pub(super) fn font(
        &mut self,
        name: &[u8],
        resources: &Dictionary,
    ) -> Result<Option<Arc<Font>>, ImportError> {
        let dict = resource(self.doc, resources, b"Font", name)?
            .as_dict()
            .map_err(|_| malformed())?;
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
        let cid = subtype == b"Type0";
        let dict = if cid {
            if ![b"Identity-H".as_slice(), b"Identity-V"].contains(
                &resolve(self.doc, root.get(b"Encoding").map_err(|_| malformed())?)?
                    .as_name()
                    .map_err(|_| ImportError::Unsupported("pdf.font_encoding"))?,
            ) {
                return Err(ImportError::Unsupported("pdf.font_encoding"));
            }
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
            if ![b"TrueType".as_slice(), b"Type1"].contains(&subtype) {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            root
        };
        let vertical = if cid
            && resolve(self.doc, root.get(b"Encoding").map_err(|_| malformed())?)?
                .as_name()
                .ok()
                == Some(b"Identity-V")
        {
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
        });
        let (data, index, substituted) = if let Some(program) = embedded {
            let program = resolve(self.doc, program)?
                .as_stream()
                .map_err(|_| malformed())?;
            if let Ok(kind) = program.dict.get(b"Subtype") {
                if kind.as_name().ok() != Some(b"OpenType") {
                    return Err(ImportError::Unsupported("pdf.font_program"));
                }
            }
            let data = program
                .get_plain_content_with_limit(4 * 1024 * 1024)
                .map_err(|_| ImportError::LimitExceeded("pdf.font_program"))?;
            (data, 0, false)
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
                db.load_system_fonts();
                db
            });
            use usvg::fontdb::{Family, Query, Style, Weight};
            let families = [Family::Name(family), Family::SansSerif];
            let id = db
                .query(&Query {
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
                .ok_or(ImportError::Unsupported("pdf.font_program"))?;
            let (data, index) = db
                .with_face_data(id, |data, index| {
                    if data.len() <= 4 * 1024 * 1024 {
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
        let face = ttf_parser::Face::parse(&data, index)
            .map_err(|_| ImportError::Unsupported("pdf.font_program"))?;
        let tables = face.tables();
        if face.is_variable()
            || tables.colr.is_some()
            || tables.svg.is_some()
            || tables.sbix.is_some()
            || tables.cbdt.is_some()
            || tables.bdat.is_some()
            || tables.ebdt.is_some()
        {
            return Err(ImportError::Unsupported("pdf.font_program"));
        }
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
                let cff = face
                    .tables()
                    .cff
                    .as_ref()
                    .ok_or(ImportError::Unsupported("pdf.font_program"))?;
                let mut pairs = Vec::new();
                let mut max = 0u16;
                for gid in 0..face.number_of_glyphs() {
                    let cid = cff
                        .glyph_cid(ttf_parser::GlyphId(gid))
                        .ok_or(ImportError::Unsupported("pdf.font_program"))?;
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
                if face.tables().glyf.is_none() {
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
                return Ok(Font {
                    data: data.into(),
                    index,
                    cid,
                    gids,
                    widths,
                    default_width,
                    vertical,
                });
            }
            if !substituted {
                return Err(ImportError::Unsupported("pdf.font_encoding"));
            }
            let gids: Vec<_> = mapping
                .iter()
                .map(|g| g.and_then(|c| face.glyph_index(c)).map(|g| g.0))
                .collect();
            self.simple_widths(dict, &face, &gids, substituted, &mut widths)?;
            gids
        };
        Ok(Font {
            data: data.into(),
            index,
            cid,
            gids,
            widths,
            default_width,
            vertical,
        })
    }
    fn simple_widths(
        &self,
        dict: &Dictionary,
        face: &ttf_parser::Face<'_>,
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
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
        }
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
