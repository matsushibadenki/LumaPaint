//! PDF text state and glyph outlines. No host renderer or webview font is involved.
use super::*;
use std::sync::Arc;
#[derive(Clone)]
pub(super) struct TextStyle {
    font: Option<Arc<fonts::Font>>,
    size: f32,
    spacing: f32,
    word_spacing: f32,
    hscale: f32,
    leading: f32,
    rise: f32,
    mode: u8,
}
impl TextStyle {
    pub(super) fn mode(&self) -> u8 {
        self.mode
    }
    pub(super) fn geometry(&self) -> (f32, f32, f32) {
        (self.size, self.hscale, self.rise)
    }
    pub(super) fn spacing_for(&self, word: bool) -> f32 {
        self.spacing + if word { self.word_spacing } else { 0. }
    }

    pub(super) fn set_font(&mut self, font: Option<Arc<fonts::Font>>, size: f32) {
        self.font = font;
        self.size = size;
    }
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: None,
            size: 0.,
            spacing: 0.,
            word_spacing: 0.,
            hscale: 1.,
            leading: 0.,
            rise: 0.,
            mode: 0,
        }
    }
}
pub(super) struct TextPosition {
    tm: [f32; 6],
    line: [f32; 6],
    active: bool,
    valid: bool,
    clip: String,
    clip_active: bool,
}
impl Default for TextPosition {
    fn default() -> Self {
        Self {
            tm: [1., 0., 0., 1., 0., 0.],
            line: [1., 0., 0., 1., 0., 0.],
            active: false,
            valid: true,
            clip: String::new(),
            clip_active: false,
        }
    }
}
struct Outline {
    path: String,
    matrix: [f32; 6],
    limited: bool,
}
impl Outline {
    fn point(&self, x: f32, y: f32) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.matrix;
        [a * x + c * y + e, b * x + d * y + f]
    }
    fn append(&mut self, command: &str, points: &[[f32; 2]]) {
        if self.path.len() > 65536 {
            self.limited = true;
            return;
        }
        self.path.push_str(command);
        for &[x, y] in points {
            let [x, y] = self.point(x, y);
            if !x.is_finite() || !y.is_finite() {
                self.limited = true;
                return;
            }
            let _ = write!(self.path, "{x} {y} ");
        }
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.append("M", &[[x, y]]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.append("L", &[[x, y]]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.append("Q", &[[x1, y1], [x, y]]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.append("C", &[[x1, y1], [x2, y2], [x, y]]);
    }
    fn close(&mut self) {
        self.append("Z", &[]);
    }
}
impl TextPosition {
    pub(super) fn finish(&self) -> Result<(), ImportError> {
        if self.active {
            Err(malformed())
        } else {
            Ok(())
        }
    }
    fn require(&self) -> Result<(), ImportError> {
        if !self.active {
            Err(malformed())
        } else {
            Ok(())
        }
    }
    fn next_line(&mut self, dx: f32, dy: f32) {
        self.line = matrix(self.line, [1., 0., 0., 1., dx, dy]);
        self.tm = self.line;
        self.valid = true;
    }
}
impl Interpreter<'_> {
    pub(super) fn text_operator(
        &mut self,
        operator: &str,
        args: &[Object],
        resources: &Dictionary,
        state: &mut State,
        position: &mut TextPosition,
    ) -> Result<(), ImportError> {
        match operator {
            "BT" => {
                if !args.is_empty() || position.active {
                    return Err(malformed());
                }
                *position = TextPosition {
                    active: true,
                    ..Default::default()
                };
            }
            "ET" => {
                position.require()?;
                if !args.is_empty() {
                    return Err(malformed());
                }
                position.active = false;
                if position.clip_active && position.clip.is_empty() {
                    state.empty_clip = true;
                }
                if !position.clip.is_empty() {
                    let id = format!("pdf-text-clip-{}", self.serial);
                    self.serial += 1;
                    let _ = write!(
                        self.defs,
                        "<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\">{}</clipPath>",
                        position.clip
                    );
                    position.clip.clear();
                    state.clips.push(id);
                }
            }
            "Tf" => {
                if args.len() != 2 {
                    return Err(malformed());
                }
                state.text.font =
                    self.font(args[0].as_name().map_err(|_| malformed())?, resources)?;
                state.text.size = num(&args[1])?;
            }
            "Tm" => {
                position.require()?;
                position.tm = nums(args, 6)?.try_into().unwrap();
                position.line = position.tm;
                position.valid = true;
            }
            "Td" | "TD" => {
                position.require()?;
                let p = nums(args, 2)?;
                if operator == "TD" {
                    state.text.leading = -p[1];
                }
                position.next_line(p[0], p[1]);
            }
            "T*" => {
                position.require()?;
                if !args.is_empty() {
                    return Err(malformed());
                }
                position.next_line(0., -state.text.leading);
            }
            "Tc" | "Tw" | "Tz" | "TL" | "Ts" => {
                let n = nums(args, 1)?[0];
                match operator {
                    "Tc" => state.text.spacing = n,
                    "Tw" => state.text.word_spacing = n,
                    "Tz" => state.text.hscale = n / 100.,
                    "TL" => state.text.leading = n,
                    _ => state.text.rise = n,
                };
            }
            "Tr" => {
                let n = nums(args, 1)?[0];
                if !(0. ..=7.).contains(&n) || n.fract() != 0. {
                    return Err(malformed());
                }
                state.text.mode = n as u8;
            }
            "Tj" | "'" => {
                position.require()?;
                if args.len() != 1 {
                    return Err(malformed());
                }
                if operator == "'" {
                    position.next_line(0., -state.text.leading);
                }
                self.text_show(&args[0], state, position, resources)?;
            }
            "\"" => {
                position.require()?;
                if args.len() != 3 {
                    return Err(malformed());
                }
                state.text.word_spacing = num(&args[0])?;
                state.text.spacing = num(&args[1])?;
                position.next_line(0., -state.text.leading);
                self.text_show(&args[2], state, position, resources)?;
            }
            "TJ" => {
                position.require()?;
                if args.len() != 1 {
                    return Err(malformed());
                }
                let array = args[0].as_array().map_err(|_| malformed())?;
                if array.len() > 65536 {
                    return Err(ImportError::LimitExceeded("pdf.text_array"));
                }
                for value in array {
                    if value.as_str().is_ok() {
                        self.text_show(value, state, position, resources)?;
                    } else {
                        let shift = -num(value)? / 1000. * state.text.size;
                        let vertical = state
                            .text
                            .font
                            .as_ref()
                            .is_some_and(|f| f.vertical.is_some());
                        let (dx, dy) = if vertical {
                            (0., shift)
                        } else {
                            (shift * state.text.hscale, 0.)
                        };
                        position.tm = matrix(position.tm, [1., 0., 0., 1., dx, dy]);
                    }
                }
            }
            _ => return Err(malformed()),
        }
        Ok(())
    }
    fn text_show(
        &mut self,
        value: &Object,
        state: &State,
        position: &mut TextPosition,
        resources: &Dictionary,
    ) -> Result<(), ImportError> {
        let bytes = value.as_str().map_err(|_| malformed())?;
        if bytes.is_empty() {
            return Ok(());
        }
        let Some(font) = state.text.font.clone() else {
            self.unsupported("pdf.text_conversion")?;
            position.valid = false;
            return Ok(());
        };
        // ISO 32000-1 §9.3.6: Type 3 responds only to mode 3; it
        // does not contribute outlines to the text clipping path.
        if state.text.mode >= 4 && font.type3.is_none() {
            position.clip_active = true;
        }
        if !position.valid {
            self.unsupported("pdf.text_position")?;
            return Ok(());
        }
        let codes: Vec<(u16, bool)> = if let Some(map) = &font.encoding {
            map.decode(bytes)?
        } else {
            bytes.iter().map(|b| (u16::from(*b), *b == 32)).collect()
        };
        let count = codes.len();
        self.text_glyphs += count;
        if self.text_glyphs > 65536 {
            return Err(ImportError::LimitExceeded("pdf.text_glyphs"));
        }
        if state.text.mode == 3 {
            self.conversion("pdf.invisible_text_omitted", CompatibilityTier::C)?;
        }
        if !bytes.is_empty() && state.text.mode != 3 {
            self.conversion("pdf.text_outlined", CompatibilityTier::B)?;
        }
        if font.type3.is_some() {
            return self.type3_show(&font, bytes, state, resources, &mut position.tm);
        }
        let face = if font.outlines.is_none() {
            Some(ttf_parser::Face::parse(&font.data, font.index).map_err(|_| malformed())?)
        } else {
            None
        };
        let units = font
            .outlines
            .as_ref()
            .map(|o| o.units)
            .or_else(|| face.as_ref().map(|f| f.units_per_em()))
            .ok_or_else(malformed)?;
        let glyph_count = font
            .outlines
            .as_ref()
            .map(|o| o.count)
            .or_else(|| face.as_ref().map(|f| f.number_of_glyphs()))
            .ok_or_else(malformed)?;
        let scale = state.text.size / f32::from(units);
        for (code, word_space) in codes {
            let width = font
                .widths
                .get(&code)
                .copied()
                .unwrap_or(font.default_width);
            let vertical = font.vertical.as_ref().map(|v| v.glyph(code, width));
            let spacing = state.text.spacing
                + if word_space {
                    state.text.word_spacing
                } else {
                    0.
                };
            let (dx, dy, ox, oy) = if let Some([advance, vx, vy]) = vertical {
                (
                    0.,
                    advance / 1000. * state.text.size + spacing,
                    -vx / 1000. * state.text.size * state.text.hscale,
                    -vy / 1000. * state.text.size,
                )
            } else {
                (
                    (width / 1000. * state.text.size + spacing) * state.text.hscale,
                    0.,
                    0.,
                    0.,
                )
            };
            if state.text.mode != 3 {
                if let Some(gid) = font
                    .gids
                    .get(usize::from(code))
                    .copied()
                    .flatten()
                    .filter(|g| *g < glyph_count && *g != 0)
                {
                    let mut outline = Outline {
                        path: String::new(),
                        matrix: matrix(
                            position.tm,
                            [
                                scale * state.text.hscale,
                                0.,
                                0.,
                                scale,
                                ox,
                                oy + state.text.rise,
                            ],
                        ),
                        limited: false,
                    };
                    let valid = if let Some(face) = &face {
                        face.outline_glyph(ttf_parser::GlyphId(gid), &mut outline)
                            .is_some()
                    } else if let Some(commands) =
                        font.outlines.as_ref().and_then(|o| o.glyphs.get(&gid))
                    {
                        use ttf_parser::OutlineBuilder;
                        for command in commands {
                            match *command {
                                font_program::Command::Move([x, y]) => outline.move_to(x, y),
                                font_program::Command::Line([x, y]) => outline.line_to(x, y),
                                font_program::Command::Quad([a, b], [x, y]) => {
                                    outline.quad_to(a, b, x, y)
                                }
                                font_program::Command::Cubic([a, b], [c, d], [x, y]) => {
                                    outline.curve_to(a, b, c, d, x, y)
                                }
                                font_program::Command::Close => outline.close(),
                            }
                        }
                        true
                    } else {
                        false
                    };
                    if !valid && (font.outlines.is_some() || !outline.path.is_empty()) {
                        self.unsupported("pdf.glyph_outline")?;
                        outline.path.clear();
                    }
                    if outline.limited || outline.path.len() > 65536 {
                        return Err(ImportError::LimitExceeded("pdf.glyph_outline"));
                    }
                    if !outline.path.is_empty() {
                        let [a, b, c, d, e, f] = state.ctm;
                        if state.text.mode >= 4 {
                            let _ = write!(
                                position.clip,
                                "<path d=\"{}\" transform=\"matrix({a} {b} {c} {d} {e} {f})\"/>",
                                outline.path
                            );
                            if position.clip.len() > 4 * 1024 * 1024 {
                                return Err(ImportError::LimitExceeded("pdf.text_clip"));
                            }
                        }
                        if state.text.mode != 7 {
                            let fill = if matches!(state.text.mode, 1 | 5) {
                                "none".into()
                            } else {
                                self.gradient_paint(&state.fill_pattern, &state.fill, state.ctm)?
                            };
                            let stroke = if matches!(state.text.mode, 1 | 2 | 5 | 6) {
                                self.gradient_paint(
                                    &state.stroke_pattern,
                                    &state.stroke,
                                    state.ctm,
                                )?
                            } else {
                                "none".into()
                            };
                            let dash = state
                                .dash
                                .iter()
                                .map(|n| n.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                            self.draw(format!("<path d=\"{}\" transform=\"matrix({a} {b} {c} {d} {e} {f})\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-miterlimit=\"{}\" stroke-dasharray=\"{dash}\" stroke-dashoffset=\"{}\" fill-opacity=\"{}\" stroke-opacity=\"{}\" style=\"mix-blend-mode:{}\"/>",outline.path,state.width,["butt","round","square"][state.cap as usize],["miter","round","bevel"][state.join as usize],state.miter,state.offset,state.fill_alpha,state.stroke_alpha,state.blend),state);
                        }
                    }
                } else {
                    self.unsupported("pdf.glyph_missing")?;
                }
            }
            position.tm = matrix(position.tm, [1., 0., 0., 1., dx, dy]);
            if self.body.len() + self.defs.len() > 4 * 1024 * 1024 {
                return Err(ImportError::LimitExceeded("pdf.svg_source"));
            }
        }
        Ok(())
    }
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
    #[test]
    fn composite_word_spacing_uses_source_byte_not_cid() {
        let doc = lopdf::Document::new();
        for (cmap, bytes, expected) in [(b"1 begincodespacerange <00> <ff> endcodespacerange 1 begincidchar <20> 65 endcidchar".as_slice(),vec![32,32],34.),(b"1 begincodespacerange <0000> <ffff> endcodespacerange 1 begincidchar <0020> 65 endcidchar".as_slice(),vec![0,32,0,32],24.)] {
            let mut gids=vec![None;256];gids[65]=Some(1);
            let font=Arc::new(fonts::Font{data:include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf").as_slice().into(),index:0,outlines:None,type3:None,encoding:Some(cmap::parse(cmap,None).unwrap()),gids,widths:Default::default(),default_width:600.,vertical:None});
            let state=State{text:TextStyle{font:Some(font),size:20.,word_spacing:5.,..Default::default()},..Default::default()};
            let mut position=TextPosition{active:true,..Default::default()};
            parser(&doc).text_show(&Object::String(bytes,lopdf::StringFormat::Hexadecimal),&state,&mut position,&Dictionary::new()).unwrap();
            assert_eq!(position.tm[4],expected);
        }
    }
    #[test]
    fn graphics_state_font_preserves_spacing_and_text_transform() {
        let mut doc = lopdf::Document::new();
        let mut resources =
            fonts::fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
        let font = resources
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"F")
            .unwrap()
            .clone();
        resources.set(
            "ExtGState",
            lopdf::dictionary! {"GS"=>lopdf::dictionary!{"Font"=>vec![font,20.into()]}},
        );
        let mut reference = parser(&doc);
        reference
            .content(
                b"BT 2 Tc 1 0 0 1 10 40 Tm /F 20 Tf (AB) Tj ET",
                &resources,
                State::default(),
                0,
            )
            .unwrap();
        let mut actual = parser(&doc);
        actual
            .content(
                b"BT 2 Tc 1 0 0 1 10 40 Tm /GS gs (AB) Tj ET",
                &resources,
                State::default(),
                0,
            )
            .unwrap();
        assert_eq!(actual.body, reference.body);
        assert!(!actual.issues.iter().any(|i| i.code == "pdf.font_state"));
    }
    #[test]
    fn text_requires_balanced_objects_and_valid_arguments() {
        let doc = lopdf::Document::new();
        let resources = Dictionary::new();
        for content in [
            b"(A) Tj".as_slice(),
            b"ET",
            b"BT BT ET",
            b"BT",
            b"BT 1 Tr (A) 2 Tj ET",
            b"BT 8 Tr ET",
        ] {
            assert!(parser(&doc)
                .content(content, &resources, State::default(), 0)
                .is_err());
        }
    }
    #[test]
    fn outlines_require_consent_and_text_glyph_budget_is_bounded() {
        let mut doc = lopdf::Document::new();
        let resources =
            fonts::fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
        let mut p = parser(&doc);
        p.allow_lossy = false;
        let Err(ImportError::LossyConversionRequiresConsent(report)) =
            p.content(b"BT /F 20 Tf (A) Tj ET", &resources, State::default(), 0)
        else {
            panic!()
        };
        assert_eq!(report.issues[0].code, "pdf.text_outlined");
        assert_eq!(report.issues[0].tier, CompatibilityTier::B);
        let mut p = parser(&doc);
        p.text_glyphs = 65536;
        assert!(matches!(
            p.content(b"BT /F 20 Tf (A) Tj ET", &resources, State::default(), 0),
            Err(ImportError::LimitExceeded("pdf.text_glyphs"))
        ));
    }
    #[test]
    fn invisible_text_advances_and_text_parameters_follow_graphics_state_restore() {
        let mut doc = lopdf::Document::new();
        let resources =
            fonts::fixture_resources(&mut doc, Object::Name(b"WinAnsiEncoding".to_vec()));
        let mut p = parser(&doc);
        p.content(
            b"BT /F 20 Tf 3 Tr (A) Tj q 3 Tc Q 0 Tr (A) Tj (A) Tj ET",
            &resources,
            State::default(),
            0,
        )
        .unwrap();
        assert_eq!(p.body.matches("<path").count(), 2);
        assert!(p.body.contains("12 0"));
        assert!(p.body.contains("24 0"));
        assert!(p
            .issues
            .iter()
            .any(|i| i.code == "pdf.invisible_text_omitted"));
    }
    #[test]
    fn odd_length_cid_strings_and_unknown_text_positions_are_not_guessed() {
        let doc = lopdf::Document::new();
        let mut p = parser(&doc);
        let mut gids = vec![None; 256];
        gids[65] = Some(1);
        let font = Arc::new(fonts::Font {
            data: include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf")
                .as_slice()
                .into(),
            index: 0,
            outlines: None,
            type3: None,
            encoding: Some(cmap::CMap::identity(false)),
            gids,
            widths: Default::default(),
            default_width: 600.,
            vertical: None,
        });
        let state = State {
            text: TextStyle {
                font: Some(font),
                size: 20.,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut position = TextPosition {
            active: true,
            ..Default::default()
        };
        assert!(matches!(
            p.text_show(
                &Object::string_literal("A"),
                &state,
                &mut position,
                &Dictionary::new()
            ),
            Err(ImportError::Malformed(_))
        ));
        position.valid = false;
        p.text_show(
            &Object::string_literal("AB"),
            &state,
            &mut position,
            &Dictionary::new(),
        )
        .unwrap();
        assert!(p.body.is_empty());
        assert_eq!(p.issues[0].code, "pdf.text_position");
    }
}
