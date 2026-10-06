//! Font-program parsing stays on the Rust side. FreeType handles PostScript
//! programs; only bounded, owned outline commands survive parsing.
use super::*;
use std::collections::HashMap;
#[derive(Clone, Debug)]
pub(super) enum Command {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}
#[derive(Clone)]
pub(super) struct Outlines {
    pub units: u16,
    pub count: u16,
    pub glyphs: HashMap<u16, Vec<Command>>,
}
pub(super) enum Program<'a> {
    Sfnt(Box<ttf_parser::Face<'a>>),
    Postscript {
        face: freetype::Face,
        cids: Option<Vec<u16>>,
        cff: Option<Box<ttf_parser::cff::Table<'a>>>,
    },
}
impl<'a> Program<'a> {
    pub fn parse(data: &'a [u8], index: u32) -> Result<Self, ImportError> {
        if let Ok(face) = ttf_parser::Face::parse(data, index) {
            let t = face.tables();
            if face.is_variable()
                || t.colr.is_some()
                || t.svg.is_some()
                || t.sbix.is_some()
                || t.cbdt.is_some()
                || t.bdat.is_some()
                || t.ebdt.is_some()
            {
                return Err(ImportError::Unsupported("pdf.font_program"));
            }
            return Ok(Self::Sfnt(Box::new(face)));
        }
        let library =
            freetype::Library::init().map_err(|_| ImportError::Unsupported("pdf.font_program"))?;
        let face = library
            .new_memory_face(data.to_vec(), index as isize)
            .map_err(|_| ImportError::Unsupported("pdf.font_program"))?;
        if !face.is_scalable()
            || face.num_glyphs() <= 0
            || face.num_glyphs() > 65535
            || face.em_size() <= 0
        {
            return Err(ImportError::Unsupported("pdf.font_program"));
        }
        let cff = ttf_parser::cff::Table::parse(data).map(Box::new);
        let mut cids = cff.as_ref().and_then(|t| {
            (0..t.number_of_glyphs())
                .map(|g| t.glyph_cid(ttf_parser::GlyphId(g)))
                .collect::<Option<Vec<_>>>()
        });
        if let Some(cids) = &mut cids {
            cids.sort_unstable();
            cids.dedup();
        }
        Ok(Self::Postscript { face, cids, cff })
    }
    pub fn native(&self) -> Option<&ttf_parser::Face<'a>> {
        if let Self::Sfnt(f) = self {
            Some(f)
        } else {
            None
        }
    }
    pub fn units_per_em(&self) -> u16 {
        match self {
            Self::Sfnt(f) => f.units_per_em(),
            Self::Postscript { face: f, .. } => f.em_size() as u16,
        }
    }
    pub fn number_of_glyphs(&self) -> u16 {
        match self {
            Self::Sfnt(f) => f.number_of_glyphs(),
            Self::Postscript { face: f, .. } => f.num_glyphs() as u16,
        }
    }
    pub fn glyph_index(&self, c: char) -> Option<ttf_parser::GlyphId> {
        match self {
            Self::Sfnt(f) => f.glyph_index(c),
            Self::Postscript { face: f, .. } => f
                .get_char_index(c as usize)
                .and_then(|v| u16::try_from(v).ok())
                .map(ttf_parser::GlyphId),
        }
    }
    pub fn glyph_index_by_name(&self, n: &str) -> Option<ttf_parser::GlyphId> {
        match self {
            Self::Sfnt(f) => f.glyph_index_by_name(n),
            Self::Postscript { face: f, .. } => f
                .get_name_index(n)
                .and_then(|v| u16::try_from(v).ok())
                .map(ttf_parser::GlyphId),
        }
    }
    pub fn builtin_index(&self, code: u16) -> Option<u16> {
        match self {
            Self::Postscript { face: f, .. } => {
                // Adobe custom charmap preserves the program's built-in byte encoding.
                for i in 0..f.num_charmaps() {
                    let map = f.get_charmap(i as isize);
                    if map.encoding() == freetype::ffi::FT_ENCODING_ADOBE_CUSTOM
                        || map.encoding() == freetype::ffi::FT_ENCODING_ADOBE_STANDARD
                        || map.encoding() == freetype::ffi::FT_ENCODING_ADOBE_EXPERT
                    {
                        f.set_charmap(&map).ok()?;
                        return f
                            .get_char_index(usize::from(code))
                            .and_then(|g| u16::try_from(g).ok());
                    }
                }
                None
            }
            Self::Sfnt(f) => f
                .tables()
                .cff
                .as_ref()
                .and_then(|cff| u8::try_from(code).ok().and_then(|c| cff.glyph_index(c)))
                .map(|g| g.0),
        }
    }
    pub fn truetype_builtin_mapping(&self) -> Result<Vec<Option<u16>>, ImportError> {
        let face = self
            .native()
            .ok_or(ImportError::Unsupported("pdf.font_encoding"))?;
        let cmap = face
            .tables()
            .cmap
            .as_ref()
            .ok_or(ImportError::Unsupported("pdf.font_encoding"))?;
        let windows = cmap
            .subtables
            .into_iter()
            .any(|t| t.platform_id == ttf_parser::PlatformId::Windows && t.encoding_id == 0);
        let mut gids = vec![None; 256];
        for table in cmap.subtables {
            let prefixes: &[u32] = if windows {
                if table.platform_id != ttf_parser::PlatformId::Windows || table.encoding_id != 0 {
                    continue;
                }
                &[0, 0xf000, 0xf100, 0xf200]
            } else {
                if table.platform_id != ttf_parser::PlatformId::Macintosh || table.encoding_id != 0
                {
                    continue;
                }
                &[0]
            };
            for (code, gid) in gids.iter_mut().enumerate() {
                for prefix in prefixes {
                    if let Some(value) = table
                        .glyph_index(*prefix + code as u32)
                        .map(|g| g.0)
                        .filter(|g| *g != 0)
                    {
                        if gid.is_some_and(|old| old != value) {
                            return Err(ImportError::Unsupported("pdf.font_mapping"));
                        }
                        *gid = Some(value);
                    }
                }
            }
        }
        if gids.iter().all(Option::is_none) {
            return Err(ImportError::Unsupported("pdf.font_encoding"));
        }
        Ok(gids)
    }
    pub fn glyph_hor_advance(&self, gid: ttf_parser::GlyphId) -> Option<u16> {
        match self {
            Self::Sfnt(f) => f.glyph_hor_advance(gid),
            Self::Postscript { face: f, .. } => {
                f.load_glyph(u32::from(gid.0), freetype::face::LoadFlag::NO_SCALE)
                    .ok()?;
                u16::try_from(f.glyph().metrics().horiAdvance).ok()
            }
        }
    }
    pub fn cid(&self, gid: u16) -> Option<u16> {
        match self {
            Self::Sfnt(f) => f.tables().cff.as_ref()?.glyph_cid(ttf_parser::GlyphId(gid)),
            Self::Postscript { face: f, cids, .. } => {
                if let Some(cids) = cids {
                    return cids.binary_search(&gid).is_ok().then_some(gid);
                }
                unsafe extern "C" {
                    fn FT_Get_CID_From_Glyph_Index(
                        face: freetype::ffi::FT_Face,
                        glyph: u32,
                        cid: *mut u32,
                    ) -> i32;
                }
                let mut cid = 0;
                // The live Face owns this pointer; FreeType writes exactly one FT_UInt.
                let result = unsafe {
                    FT_Get_CID_From_Glyph_Index(
                        f.raw() as *const _ as *mut _,
                        u32::from(gid),
                        &mut cid,
                    )
                };
                (result == 0).then(|| u16::try_from(cid).ok()).flatten()
            }
        }
    }
    pub fn collect(
        &self,
        gids: &[Option<u16>],
        remaining: &mut usize,
    ) -> Result<Option<Outlines>, ImportError> {
        let Self::Postscript { face: f, cff, .. } = self else {
            return Ok(None);
        };
        // At 72 dpi and one em in points, 26.6 coordinates represent
        // font units / 64. Standard CFF outlines below use the source parser
        // directly because FreeType may still round during decoding.
        f.set_char_size(self.units_per_em() as isize * 64, 0, 72, 72)
            .map_err(|_| ImportError::Unsupported("pdf.glyph_outline"))?;
        let mut glyphs = HashMap::new();
        let point = |p: &freetype::Vector| [p.x as f32 / 64., p.y as f32 / 64.];
        for gid in gids.iter().flatten().copied() {
            if glyphs.contains_key(&gid) || gid >= self.number_of_glyphs() {
                continue;
            }
            let commands = if let Some(cff) = cff.as_ref().filter(|t| {
                let m = t.matrix();
                t.glyph_cid(ttf_parser::GlyphId(0)).is_none()
                    && m.sx == 0.001
                    && m.sy == 0.001
                    && m.kx == 0.
                    && m.ky == 0.
                    && m.tx == 0.
                    && m.ty == 0.
            }) {
                let mut builder = Commands::default();
                match cff.outline(ttf_parser::GlyphId(gid), &mut builder) {
                    Ok(_) => {}
                    Err(ttf_parser::CFFError::ZeroBBox) if builder.items.is_empty() => {}
                    Err(_) => return Err(ImportError::Unsupported("pdf.glyph_outline")),
                }
                if builder.limited {
                    return Err(ImportError::LimitExceeded("pdf.glyph_outline"));
                }
                builder.items
            } else {
                if let Err(error) = f.load_glyph(
                    u32::from(gid),
                    freetype::face::LoadFlag::NO_HINTING | freetype::face::LoadFlag::NO_BITMAP,
                ) {
                    if error == freetype::Error::InvalidArgument {
                        continue;
                    }
                    return Err(ImportError::Unsupported("pdf.glyph_outline"));
                }
                let outline = f
                    .glyph()
                    .outline()
                    .ok_or(ImportError::Unsupported("pdf.glyph_outline"))?;
                let mut commands = Vec::new();
                if !outline.contours().is_empty() {
                    for contour in outline.contours_iter() {
                        commands.push(Command::Move(point(contour.start())));
                        for curve in contour {
                            commands.push(match curve {
                                freetype::outline::Curve::Line(p) => Command::Line(point(&p)),
                                freetype::outline::Curve::Bezier2(a, b) => {
                                    Command::Quad(point(&a), point(&b))
                                }
                                freetype::outline::Curve::Bezier3(a, b, c) => {
                                    Command::Cubic(point(&a), point(&b), point(&c))
                                }
                            });
                            if commands.len() > 4096 {
                                return Err(ImportError::LimitExceeded("pdf.glyph_outline"));
                            }
                        }
                        commands.push(Command::Close);
                    }
                }
                commands
            };
            let cost = commands.capacity() * std::mem::size_of::<Command>() + 64;
            *remaining = remaining
                .checked_sub(cost)
                .ok_or(ImportError::LimitExceeded("pdf.content_budget"))?;
            glyphs.insert(gid, commands);
        }
        Ok(Some(Outlines {
            units: self.units_per_em(),
            count: self.number_of_glyphs(),
            glyphs,
        }))
    }
}

#[derive(Default)]
struct Commands {
    items: Vec<Command>,
    limited: bool,
}
impl Commands {
    fn push(&mut self, command: Command) {
        if self.items.len() >= 4096 {
            self.limited = true;
        } else {
            self.items.push(command);
        }
    }
}
impl ttf_parser::OutlineBuilder for Commands {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(Command::Move([x, y]));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(Command::Line([x, y]));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.push(Command::Quad([a, b], [x, y]));
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.push(Command::Cubic([a, b], [c, d], [x, y]));
    }
    fn close(&mut self) {
        self.push(Command::Close);
    }
}
#[cfg(test)]
pub(super) fn test_type1() -> Vec<u8> {
    // Original rectangle outline. lenIV=-1 keeps charstrings unencrypted;
    // the Private/CharStrings section still uses standard eexec encryption.
    let header = b"%!PS-AdobeFont-1.0: LPTest 1.0\n11 dict begin /FontName /LPTest def /FontType 1 def /PaintType 0 def /FontMatrix [0.001 0 0 0.001 0 0] def /FontBBox [0 0 500 500] def /Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /A put def currentdict end currentfile eexec\n";
    let mut private = b"abcddup /Private 8 dict dup begin /RD {string currentfile exch readstring pop} executeonly def /ND {noaccess def} executeonly def /NP {noaccess put} executeonly def /lenIV -1 def /BlueValues [] def /Subrs 0 array def end noaccess put dup /CharStrings 2 dict dup begin /.notdef 4 RD ".to_vec();
    private.extend_from_slice(&[139, 248, 236, 13]);
    private.extend_from_slice(b" ND /A 24 RD ");
    let outline = [
        139, 248, 236, 13, 239, 239, 21, 247, 92, 139, 5, 139, 248, 36, 5, 251, 92, 139, 5, 139,
        252, 36, 5, 9, 14,
    ];
    private.extend_from_slice(&outline);
    private.extend_from_slice(
        b" ND end readonly put dup /FontName get exch definefont pop mark currentfile closefile\n",
    );
    let mut r = 55665u16;
    let mut output = header.to_vec();
    for b in private {
        let cipher = b ^ (r >> 8) as u8;
        r = (u32::from(cipher)
            .wrapping_add(u32::from(r))
            .wrapping_mul(52845)
            .wrapping_add(22719)
            & 65535) as u16;
        output.extend_from_slice(format!("{cipher:02X}").as_bytes());
    }
    output.extend_from_slice(b"\n");
    output.extend(std::iter::repeat_n(b'0', 512));
    output.extend_from_slice(b"\ncleartomark\n");
    output
}
#[cfg(test)]
pub(super) fn test_cff() -> Vec<u8> {
    test_cff_at([100., 100.])
}
#[cfg(test)]
fn test_cff_at(origin: [f32; 2]) -> Vec<u8> {
    fn index(items: &[&[u8]]) -> Vec<u8> {
        if items.is_empty() {
            return vec![0, 0];
        }
        let mut bytes = (items.len() as u16).to_be_bytes().to_vec();
        bytes.push(1);
        let mut offset = 1;
        for item in items {
            bytes.push(offset);
            offset += item.len() as u8;
        }
        bytes.push(offset);
        for item in items {
            bytes.extend_from_slice(item);
        }
        bytes
    }
    let names = index(&[b"LPTest"]);
    let base = 4 + names.len() + 23 + 2 + 2;
    let mut dict = Vec::new();
    for (offset, op) in [(base, 15), (base + 3, 16), (base + 6, 17)] {
        dict.push(29);
        dict.extend_from_slice(&(offset as i32).to_be_bytes());
        dict.push(op);
    }
    let mut bytes = vec![1, 0, 4, 4];
    bytes.extend(names);
    bytes.extend(index(&[&dict]));
    bytes.extend([0, 0, 0, 0, 0, 0, 34, 0, 1, 65]);
    let mut glyph = vec![248, 236];
    for value in origin {
        glyph.push(255);
        glyph.extend_from_slice(&((value * 65536.) as i32).to_be_bytes());
    }
    glyph.extend([
        21, 247, 92, 139, 139, 248, 36, 251, 92, 139, 139, 252, 36, 5, 14,
    ]);
    bytes.extend(index(&[&[14], &glyph]));
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postscript_fractional_coordinates_are_not_rounded_to_whole_units() {
        let data = test_cff_at([100.25, 100.5]);
        let font = Program::parse(&data, 0).unwrap();
        let gid = font.glyph_index_by_name("A").unwrap().0;
        let o = font.collect(&[Some(gid)], &mut 65536).unwrap().unwrap();
        assert!(
            o.glyphs[&gid]
                .iter()
                .any(|c| matches!(c, Command::Move([100.25, 100.5]))),
            "{:?}",
            o.glyphs[&gid]
        );
    }
    #[test]
    fn bare_type1c_and_binary_type1_pfb_keep_outlines() {
        let cff = test_cff();
        let face = Program::parse(&cff, 0).unwrap();
        let gid = face.glyph_index_by_name("A").unwrap().0;
        let o = face
            .collect(&[Some(0), Some(gid)], &mut 65536)
            .unwrap()
            .unwrap();
        assert!(o.glyphs[&0].is_empty());
        assert!(o.glyphs[&gid]
            .iter()
            .any(|c| matches!(c, Command::Line([300., 500.]))));
        let pfa = test_type1();
        let split = pfa.windows(6).position(|v| v == b"eexec\n").unwrap() + 6;
        let end = pfa[split..].iter().position(|v| *v == b'\n').unwrap() + split;
        let binary: Vec<_> = pfa[split..end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect();
        let mut pdf_binary = pfa[..split].to_vec();
        pdf_binary.extend_from_slice(&binary);
        pdf_binary.extend_from_slice(&pfa[end..]);
        let face = Program::parse(&pdf_binary, 0).unwrap();
        let gid = face.glyph_index_by_name("A").unwrap().0;
        assert!(face
            .collect(&[Some(gid)], &mut 65536)
            .unwrap()
            .unwrap()
            .glyphs[&gid]
            .iter()
            .any(|c| matches!(c, Command::Line([300., 500.]))));
        let mut pfb = Vec::new();
        for (kind, segment) in [(1, &pfa[..split]), (2, binary.as_slice()), (1, &pfa[end..])] {
            pfb.extend([128, kind]);
            pfb.extend((segment.len() as u32).to_le_bytes());
            pfb.extend(segment);
        }
        pfb.extend([128, 3]);
        let face = Program::parse(&pfb, 0).unwrap();
        let gid = face.glyph_index_by_name("A").unwrap().0;
        assert_eq!(face.builtin_index(65), Some(gid));
        assert!(face
            .collect(&[Some(gid)], &mut 65536)
            .unwrap()
            .unwrap()
            .glyphs[&gid]
            .iter()
            .any(|c| matches!(c, Command::Line([300., 500.]))));
    }
    #[test]
    fn legacy_type1_preserves_original_outline_and_encoding() {
        let bytes = test_type1();
        let font = Program::parse(&bytes, 0).unwrap();
        assert!(font.native().is_none());
        let gid = font.glyph_index_by_name("A").unwrap().0;
        assert_eq!(font.builtin_index(65), Some(gid));
        let mut budget = 65536;
        let outlines = font.collect(&[Some(gid)], &mut budget).unwrap().unwrap();
        assert_eq!(outlines.units, 1000);
        assert!(outlines.glyphs[&gid]
            .iter()
            .any(|c| matches!(c, Command::Move([100., 100.]))));
        assert!(outlines.glyphs[&gid]
            .iter()
            .any(|c| matches!(c, Command::Line([300., 500.]))));
        assert!(font.collect(&[Some(gid)], &mut 1).is_err());
    }
    #[test]
    fn raw_cid_cff_uses_actual_cids_and_bounded_outlines() {
        let bytes = include_bytes!("../../../tests/fixtures/lp-japanese-cid.otf");
        let face = ttf_parser::Face::parse(bytes, 0).unwrap();
        let raw = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"CFF "))
            .unwrap();
        let font = Program::parse(raw, 0).unwrap();
        assert_eq!(font.cid(42), Some(42));
        assert_eq!(font.cid(7), Some(7));
        let o = font
            .collect(&[Some(42), Some(7)], &mut 65536)
            .unwrap()
            .unwrap();
        assert_eq!(o.glyphs.len(), 2);
        assert!(o.glyphs.values().all(|v| !v.is_empty()));
    }
}
