//! Bounded PDF graphics interpreter. Unsupported operators are never silently accepted.
use super::*;
use crate::io::ReadContent;
use lopdf::{Dictionary, Object, ObjectId};
use std::fmt::Write;
mod cmap;
mod font_collection;
mod font_program;
mod fonts;
mod gradients;
mod images;
mod masks;
mod text;
mod type3;
const LIMIT: usize = 16 * 1024 * 1024;
fn malformed() -> ImportError {
    ImportError::Malformed("pdf.graphics")
}
fn resolve<'a>(doc: &'a lopdf::Document, mut value: &'a Object) -> Result<&'a Object, ImportError> {
    for _ in 0..32 {
        if let Object::Reference(id) = value {
            value = doc.get_object(*id).map_err(|_| malformed())?;
        } else {
            return Ok(value);
        }
    }
    Err(ImportError::LimitExceeded("pdf.reference_depth"))
}
fn num(o: &Object) -> Result<f32, ImportError> {
    let n = match o {
        Object::Integer(n) => *n as f32,
        Object::Real(n) => *n,
        _ => return Err(malformed()),
    };
    if !n.is_finite() || n.abs() > 1e7 {
        return Err(malformed());
    }
    Ok(n)
}
fn nums(o: &[Object], n: usize) -> Result<Vec<f32>, ImportError> {
    if o.len() != n {
        return Err(malformed());
    }
    o.iter().map(num).collect()
}
fn matrix(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
fn inherited<'a>(
    d: &'a lopdf::Document,
    id: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, ImportError> {
    let mut dict = d.get_dictionary(id).map_err(|_| malformed())?;
    for _ in 0..32 {
        if let Ok(v) = dict.get(key) {
            return resolve(d, v).map(Some);
        }
        let Ok(parent) = dict.get(b"Parent") else {
            return Ok(None);
        };
        dict = resolve(d, parent)?.as_dict().map_err(|_| malformed())?;
    }
    Err(ImportError::LimitExceeded("pdf.page_depth"))
}
fn resource<'a>(
    d: &'a lopdf::Document,
    r: &'a Dictionary,
    kind: &[u8],
    name: &[u8],
) -> Result<&'a Object, ImportError> {
    let group = resolve(d, r.get(kind).map_err(|_| malformed())?)?
        .as_dict()
        .map_err(|_| malformed())?;
    resolve(d, group.get(name).map_err(|_| malformed())?)
}
#[derive(Clone)]
struct State {
    ctm: [f32; 6],
    pattern_base: [f32; 6],
    fill_pattern: Option<gradients::GradientPaint>,
    stroke_pattern: Option<gradients::GradientPaint>,
    fill: String,
    stroke: String,
    fill_space: usize,
    stroke_space: usize,
    width: f32,
    dash: Vec<f32>,
    offset: f32,
    cap: u8,
    join: u8,
    miter: f32,
    fill_alpha: f32,
    stroke_alpha: f32,
    blend: String,
    clips: Vec<String>,
    soft_mask: Option<String>,
    empty_clip: bool,
    text: text::TextStyle,
}
impl Default for State {
    fn default() -> Self {
        Self {
            ctm: [1., 0., 0., 1., 0., 0.],
            pattern_base: [1., 0., 0., 1., 0., 0.],
            fill_pattern: None,
            stroke_pattern: None,
            fill: "#000000".into(),
            stroke: "#000000".into(),
            fill_space: 3,
            stroke_space: 3,
            width: 1.,
            dash: vec![],
            offset: 0.,
            cap: 0,
            join: 0,
            miter: 10.,
            fill_alpha: 1.,
            stroke_alpha: 1.,
            blend: "normal".into(),
            clips: vec![],
            soft_mask: None,
            empty_clip: false,
            text: Default::default(),
        }
    }
}
struct Interpreter<'a> {
    doc: &'a lopdf::Document,
    body: String,
    defs: String,
    issues: Vec<ConversionIssue>,
    remaining: usize,
    operations: usize,
    serial: usize,
    allow_lossy: bool,
    page_bounds: [f32; 4],
    font_cache: std::collections::HashMap<usize, Option<std::sync::Arc<fonts::Font>>>,
    text_glyphs: usize,
    glyph_depth: usize,
    image_remaining: usize,
    image_cache: std::collections::HashMap<(usize, usize), String>,
}
impl Interpreter<'_> {
    fn unsupported(&mut self, code: &'static str) -> Result<(), ImportError> {
        self.conversion(code, CompatibilityTier::D)
    }
    fn conversion(
        &mut self,
        code: &'static str,
        tier: CompatibilityTier,
    ) -> Result<(), ImportError> {
        if !self.issues.iter().any(|i| i.code == code) {
            self.issues.push(ConversionIssue { code, tier });
        }
        if !self.allow_lossy {
            return Err(ImportError::LossyConversionRequiresConsent(
                ConversionReport {
                    issues: self.issues.clone(),
                },
            ));
        }
        Ok(())
    }
    fn space(&mut self, object: &Object) -> Result<usize, ImportError> {
        let object = resolve(self.doc, object)?;
        if let Ok(name) = object.as_name() {
            return match name {
                b"DeviceRGB" => Ok(3),
                b"DeviceGray" => Ok(1),
                b"DeviceCMYK" => Ok(4),
                b"Pattern" => Ok(0),
                _ => Err(ImportError::Unsupported("pdf.color_space")),
            };
        }
        let array = object.as_array().map_err(|_| malformed())?;
        if array.first().and_then(|v| v.as_name().ok()) == Some(b"Separation")
            && array.len() == 4
            && array[1].as_name().ok() == Some(b"All")
            && array[2].as_name().ok() == Some(b"DeviceCMYK")
        {
            // Recognize the standard linear all-plate tint used by LP printer marks.
            // Keep other spot functions unsupported rather than silently miscoloring them.
            let function = resolve(self.doc, &array[3])?
                .as_dict()
                .map_err(|_| malformed())?;
            let number = |key: &[u8]| num(function.get(key).map_err(|_| malformed())?);
            let values = |key: &[u8], count| {
                nums(
                    function
                        .get(key)
                        .map_err(|_| malformed())?
                        .as_array()
                        .map_err(|_| malformed())?,
                    count,
                )
            };
            if number(b"FunctionType")? == 2.
                && number(b"N")? == 1.
                && values(b"Domain", 2)? == [0., 1.]
                && values(b"C0", 4)? == [0., 0., 0., 0.]
                && values(b"C1", 4)? == [1., 1., 1., 1.]
            {
                self.conversion("pdf.registration_preview_only", CompatibilityTier::C)?;
                return Ok(2); // A one-component registration tint, distinct from DeviceGray.
            }
            return Err(ImportError::Unsupported("pdf.color_space"));
        }
        if array.first().and_then(|v| v.as_name().ok()) != Some(b"ICCBased") {
            return Err(ImportError::Unsupported("pdf.color_space"));
        }
        let profile = resolve(self.doc, array.get(1).ok_or_else(malformed)?)?
            .as_stream()
            .map_err(|_| malformed())?;
        let n = num(profile.dict.get(b"N").map_err(|_| malformed())?)? as usize;
        if ![1, 3, 4].contains(&n) {
            return Err(malformed());
        }
        let data = profile
            .get_plain_content_with_limit(4 * 1024 * 1024)
            .map_err(|_| ImportError::LimitExceeded("pdf.profile"))?;
        use sha2::Digest;
        let digest = sha2::Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if ![
            "c56e1685d888f5edb92fe07f2750f387f8fe8e91b32ff8fb0b56bfbbb9458353",
            "00c0f94e09127520a17dc0e1d9264b5702081d96dbdb1549ee88e3631ce42a9d",
        ]
        .contains(&digest.as_str())
        {
            self.unsupported("pdf.icc_profile_conversion")?;
        }
        Ok(n)
    }
    fn color(&mut self, args: &[Object], channels: usize) -> Result<String, ImportError> {
        let p = nums(args, if channels == 2 { 1 } else { channels })?;
        let rgb = match channels {
            2 => [(1. - p[0].clamp(0., 1.)).powi(2); 3],
            1 => [p[0]; 3],
            3 => [p[0], p[1], p[2]],
            4 => {
                self.unsupported("pdf.cmyk_profile_conversion")?;
                [
                    (1. - p[0]) * (1. - p[3]),
                    (1. - p[1]) * (1. - p[3]),
                    (1. - p[2]) * (1. - p[3]),
                ]
            }
            _ => return Err(ImportError::Unsupported("pdf.color_space")),
        };
        let c = rgb.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
        Ok(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
    }
    fn draw(&mut self, element: String, state: &State) {
        if state.empty_clip {
            return;
        }
        for id in &state.clips {
            let _ = write!(self.body, "<g clip-path=\"url(#{id})\">");
        }
        if let Some(id) = &state.soft_mask {
            let _ = write!(self.body, "<g mask=\"url(#{id})\">");
        }
        self.body.push_str(&element);
        if state.soft_mask.is_some() {
            self.body.push_str("</g>");
        }
        for _ in &state.clips {
            self.body.push_str("</g>");
        }
    }
    fn content(
        &mut self,
        bytes: &[u8],
        resources: &Dictionary,
        mut state: State,
        depth: usize,
    ) -> Result<(), ImportError> {
        if depth > 16 || bytes.len() > self.remaining {
            return Err(ImportError::LimitExceeded("pdf.content_budget"));
        }
        self.remaining -= bytes.len();
        let content = lopdf::content::Content::decode(bytes).map_err(|_| malformed())?;
        self.operations += content.operations.len();
        if self.operations > 200_000 {
            return Err(ImportError::LimitExceeded("pdf.operations"));
        }
        let mut text_position = text::TextPosition::default();
        let mut stack = vec![];
        let mut path = String::new();
        let mut path_matrix = state.ctm;
        let mut current = [0.; 2];
        let mut pending_clip = None;
        for op in content.operations {
            let args = &op.operands;
            match op.operator.as_str() {
                "g" | "rg" | "k" | "cs" => state.fill_pattern = None,
                "G" | "RG" | "K" | "CS" => state.stroke_pattern = None,
                _ => {}
            }
            match op.operator.as_str() {
                "q" => {
                    if stack.len() >= 64 {
                        return Err(ImportError::LimitExceeded("pdf.graphics_stack"));
                    }
                    stack.push(state.clone());
                }
                "Q" => state = stack.pop().ok_or_else(malformed)?,
                "cm" => {
                    if !path.is_empty() {
                        return Err(ImportError::Unsupported("pdf.ctm_inside_path"));
                    }
                    state.ctm = matrix(state.ctm, nums(args, 6)?.try_into().unwrap());
                }
                "m" | "l" => {
                    let p = nums(args, 2)?;
                    if path.is_empty() {
                        path_matrix = state.ctm;
                    }
                    let _ = write!(
                        path,
                        "{} {} {} ",
                        if op.operator == "m" { "M" } else { "L" },
                        p[0],
                        p[1]
                    );
                    current = [p[0], p[1]];
                }
                "c" => {
                    let p = nums(args, 6)?;
                    let _ = write!(
                        path,
                        "C {} {} {} {} {} {} ",
                        p[0], p[1], p[2], p[3], p[4], p[5]
                    );
                    current = [p[4], p[5]];
                }
                "v" => {
                    let p = nums(args, 4)?;
                    let _ = write!(
                        path,
                        "C {} {} {} {} {} {} ",
                        current[0], current[1], p[0], p[1], p[2], p[3]
                    );
                    current = [p[2], p[3]];
                }
                "y" => {
                    let p = nums(args, 4)?;
                    let _ = write!(
                        path,
                        "C {} {} {} {} {} {} ",
                        p[0], p[1], p[2], p[3], p[2], p[3]
                    );
                    current = [p[2], p[3]];
                }
                "h" => path.push_str("Z "),
                "re" => {
                    let p = nums(args, 4)?;
                    if path.is_empty() {
                        path_matrix = state.ctm;
                    }
                    let _ = write!(
                        path,
                        "M {} {} h {} v {} h {} Z ",
                        p[0], p[1], p[2], p[3], -p[2]
                    );
                    current = [p[0], p[1]];
                }
                "W" => pending_clip = Some("nonzero"),
                "W*" => pending_clip = Some("evenodd"),
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    if matches!(op.operator.as_str(), "s" | "b" | "b*") {
                        path.push_str("Z ");
                    }
                    let [a, b, c, d, e, f] = path_matrix;
                    if op.operator != "n" && !path.is_empty() {
                        let fill_paint =
                            self.gradient_paint(&state.fill_pattern, &state.fill, path_matrix)?;
                        let stroke_paint =
                            self.gradient_paint(&state.stroke_pattern, &state.stroke, path_matrix)?;
                        let fill = if matches!(op.operator.as_str(), "S" | "s") {
                            "none"
                        } else {
                            &fill_paint
                        };
                        let stroke = if matches!(op.operator.as_str(), "f" | "F" | "f*") {
                            "none"
                        } else {
                            &stroke_paint
                        };
                        let rule = if op.operator.ends_with('*') {
                            "evenodd"
                        } else {
                            "nonzero"
                        };
                        let dash = state
                            .dash
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" ");
                        self.draw(format!("<path d=\"{path}\" transform=\"matrix({a} {b} {c} {d} {e} {f})\" fill=\"{fill}\" stroke=\"{stroke}\" fill-rule=\"{rule}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-miterlimit=\"{}\" stroke-dasharray=\"{dash}\" stroke-dashoffset=\"{}\" fill-opacity=\"{}\" stroke-opacity=\"{}\" style=\"mix-blend-mode:{}\"/>",state.width,["butt","round","square"][state.cap as usize],["miter","round","bevel"][state.join as usize],state.miter,state.offset,state.fill_alpha,state.stroke_alpha,state.blend),&state);
                    }
                    if let Some(rule) = pending_clip.take() {
                        let id = format!("pdf-clip-{}", self.serial);
                        self.serial += 1;
                        let _=write!(self.defs,"<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"{path}\" transform=\"matrix({a} {b} {c} {d} {e} {f})\" clip-rule=\"{rule}\"/></clipPath>");
                        state.clips.push(id);
                    }
                    path.clear();
                }
                "w" => {
                    state.width = nums(args, 1)?[0];
                    if state.width < 0. {
                        return Err(malformed());
                    }
                }
                "J" | "j" => {
                    let n = nums(args, 1)?[0];
                    if !(0. ..=2.).contains(&n) || n.fract() != 0. {
                        return Err(malformed());
                    }
                    if op.operator == "J" {
                        state.cap = n as u8
                    } else {
                        state.join = n as u8
                    }
                }
                "M" => {
                    state.miter = nums(args, 1)?[0];
                    if state.miter < 1. {
                        return Err(malformed());
                    }
                }
                "d" => {
                    if args.len() != 2 {
                        return Err(malformed());
                    }
                    state.dash = args[0]
                        .as_array()
                        .map_err(|_| malformed())?
                        .iter()
                        .map(num)
                        .collect::<Result<_, _>>()?;
                    state.offset = num(&args[1])?;
                    if state.dash.len() > 128 || state.dash.iter().any(|n| *n < 0.) {
                        return Err(malformed());
                    }
                }
                "g" | "G" | "rg" | "RG" | "k" | "K" => {
                    let count = match op.operator.as_str() {
                        "g" | "G" => 1,
                        "rg" | "RG" => 3,
                        _ => 4,
                    };
                    let color = self.color(args, count)?;
                    if op.operator.chars().next().unwrap().is_uppercase() {
                        state.stroke = color;
                        state.stroke_space = count;
                    } else {
                        state.fill = color;
                        state.fill_space = count;
                    }
                }
                "cs" | "CS" => {
                    let name = args
                        .first()
                        .ok_or_else(malformed)?
                        .as_name()
                        .map_err(|_| malformed())?;
                    let direct = Object::Name(name.to_vec());
                    let object = if [
                        b"DeviceRGB".as_slice(),
                        b"DeviceGray",
                        b"DeviceCMYK",
                        b"Pattern",
                    ]
                    .contains(&name)
                    {
                        &direct
                    } else {
                        resource(self.doc, resources, b"ColorSpace", name)?
                    };
                    let space = self.space(object)?;
                    if op.operator == "cs" {
                        state.fill_space = space;
                        state.fill = "#000000".into();
                    } else {
                        state.stroke_space = space;
                        state.stroke = "#000000".into();
                    }
                }
                "sc" | "SC" | "scn" | "SCN" => {
                    let stroke = op.operator.chars().next().unwrap().is_uppercase();
                    let channels = if stroke {
                        state.stroke_space
                    } else {
                        state.fill_space
                    };
                    if channels == 0 {
                        if args.len() != 1 {
                            self.unsupported("pdf.pattern_conversion")?;
                            continue;
                        }
                        let name = args[0].as_name().map_err(|_| malformed())?;
                        let paint = self.pattern(name, resources, state.pattern_base)?;
                        if stroke {
                            state.stroke_pattern = paint;
                            state.stroke = "none".into();
                        } else {
                            state.fill_pattern = paint;
                            state.fill = "none".into();
                        }
                        continue;
                    }
                    let color = self.color(args, channels)?;
                    if stroke {
                        state.stroke = color
                    } else {
                        state.fill = color
                    }
                }
                "gs" => {
                    let name = args
                        .first()
                        .ok_or_else(malformed)?
                        .as_name()
                        .map_err(|_| malformed())?;
                    let gs = resource(self.doc, resources, b"ExtGState", name)?
                        .as_dict()
                        .map_err(|_| malformed())?;
                    if let Ok(v) = gs.get(b"ca") {
                        state.fill_alpha = num(resolve(self.doc, v)?)?.clamp(0., 1.);
                    }
                    if let Ok(v) = gs.get(b"CA") {
                        state.stroke_alpha = num(resolve(self.doc, v)?)?.clamp(0., 1.);
                    }
                    if let Ok(v) = gs.get(b"SMask") {
                        state.soft_mask = self.soft_mask(v, resources, &state, depth)?;
                    }
                    if let Ok(v) = gs.get(b"BM") {
                        let name = resolve(self.doc, v)?.as_name().map_err(|_| malformed())?;
                        state.blend = match name {
                            b"Normal" => "normal",
                            b"Multiply" => "multiply",
                            b"Screen" => "screen",
                            b"Overlay" => "overlay",
                            b"Darken" => "darken",
                            b"Lighten" => "lighten",
                            b"Difference" => "difference",
                            _ => {
                                self.unsupported("pdf.blend_mode")?;
                                "normal"
                            }
                        }
                        .into();
                    }
                    if let Ok(font) = gs.get(b"Font") {
                        let font = resolve(self.doc, font)?
                            .as_array()
                            .map_err(|_| malformed())?;
                        if font.len() != 2 {
                            return Err(malformed());
                        }
                        let dict = resolve(self.doc, &font[0])?
                            .as_dict()
                            .map_err(|_| malformed())?;
                        let size = num(resolve(self.doc, &font[1])?)?;
                        state.text.set_font(self.font_dictionary(dict)?, size);
                    }
                    for key in [b"TR".as_slice(), b"TR2", b"HT", b"BG", b"UCR", b"OP", b"op"] {
                        if gs.get(key).is_ok() {
                            self.unsupported("pdf.graphics_effect")?;
                        }
                    }
                }
                "Do" => {
                    let name = args
                        .first()
                        .ok_or_else(malformed)?
                        .as_name()
                        .map_err(|_| malformed())?;
                    let obj = resource(self.doc, resources, b"XObject", name)?
                        .as_stream()
                        .map_err(|_| malformed())?;
                    if obj.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image") {
                        if let Some(id) = self.image(obj, resources)? {
                            let t = matrix(state.ctm, [1., 0., 0., -1., 0., 1.]);
                            self.draw(format!("<use href=\"#{id}\" transform=\"matrix({} {} {} {} {} {})\" opacity=\"{}\" style=\"mix-blend-mode:{}\"/>",t[0],t[1],t[2],t[3],t[4],t[5],state.fill_alpha,state.blend),&state);
                        }
                        if self.body.len() + self.defs.len() > 4 * 1024 * 1024 {
                            return Err(ImportError::LimitExceeded("pdf.svg_source"));
                        }
                        continue;
                    }
                    if obj.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
                        self.unsupported("pdf.image_xobject")?;
                        continue;
                    }
                    let transparency = if let Ok(group) = obj.dict.get(b"Group") {
                        let group = resolve(self.doc, group)?
                            .as_dict()
                            .map_err(|_| malformed())?;
                        let isolated = group.get(b"I").and_then(Object::as_bool).unwrap_or(false);
                        let knockout = group.get(b"K").and_then(Object::as_bool).unwrap_or(false);
                        if group.get(b"S").and_then(Object::as_name).ok() != Some(b"Transparency")
                            || knockout
                            || (!isolated
                                && (state.fill_alpha != 1.
                                    || state.blend != "normal"
                                    || state.soft_mask.is_some()))
                        {
                            self.unsupported("pdf.transparency_group")?;
                            None
                        } else {
                            if let Ok(space) = group.get(b"CS") {
                                self.space(space)?;
                            }
                            Some(isolated)
                        }
                    } else {
                        None
                    };
                    let mut nested = state.clone();
                    if let Ok(m) = obj.dict.get(b"Matrix") {
                        nested.ctm = matrix(
                            nested.ctm,
                            nums(
                                resolve(self.doc, m)?.as_array().map_err(|_| malformed())?,
                                6,
                            )?
                            .try_into()
                            .unwrap(),
                        );
                    }
                    nested.pattern_base = nested.ctm;
                    if let Ok(bbox) = obj.dict.get(b"BBox") {
                        let p = nums(
                            resolve(self.doc, bbox)?
                                .as_array()
                                .map_err(|_| malformed())?,
                            4,
                        )?;
                        let [a, b, c, d, e, f] = nested.ctm;
                        let id = format!("pdf-form-{}", self.serial);
                        self.serial += 1;
                        let _=write!(self.defs,"<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"M {} {} H {} V {} H {} Z\" transform=\"matrix({a} {b} {c} {d} {e} {f})\"/></clipPath>",p[0],p[1],p[2],p[3],p[0]);
                        nested.clips.push(id);
                    }
                    let res = if let Ok(r) = obj.dict.get(b"Resources") {
                        resolve(self.doc, r)?.as_dict().map_err(|_| malformed())?
                    } else {
                        resources
                    };
                    let data = obj
                        .get_plain_content_with_limit(self.remaining)
                        .map_err(|_| ImportError::LimitExceeded("pdf.stream"))?;
                    if let Some(isolated) = transparency {
                        let clips = nested.clips.len();
                        for id in &nested.clips {
                            let _ = write!(self.body, "<g clip-path=\"url(#{id})\">");
                        }
                        if let Some(id) = &state.soft_mask {
                            let _ = write!(self.body, "<g mask=\"url(#{id})\">");
                        }
                        let _ = write!(
                            self.body,
                            "<g opacity=\"{}\" style=\"mix-blend-mode:{};isolation:{}\">",
                            state.fill_alpha,
                            state.blend,
                            if isolated { "isolate" } else { "auto" }
                        );
                        nested.clips.clear();
                        nested.soft_mask = None;
                        nested.fill_alpha = 1.;
                        nested.stroke_alpha = 1.;
                        nested.blend = "normal".into();
                        self.content(&data, res, nested, depth + 1)?;
                        self.body.push_str("</g>");
                        if state.soft_mask.is_some() {
                            self.body.push_str("</g>");
                        }
                        for _ in 0..clips {
                            self.body.push_str("</g>");
                        }
                    } else {
                        self.content(&data, res, nested, depth + 1)?;
                    }
                }
                "BT" | "ET" | "Tf" | "Tm" | "Td" | "TD" | "T*" | "Tc" | "Tw" | "Tz" | "TL"
                | "Tr" | "Ts" | "Tj" | "TJ" | "'" | "\"" => self.text_operator(
                    &op.operator,
                    args,
                    resources,
                    &mut state,
                    &mut text_position,
                )?,
                "sh" => {
                    if args.len() != 1 {
                        return Err(malformed());
                    }
                    let name = args[0].as_name().map_err(|_| malformed())?;
                    self.shading_draw(name, resources, &state)?;
                }
                "ri" | "i" | "MP" | "DP" | "BMC" | "EMC" => {}
                "BDC" => {
                    if args.first().and_then(|v| v.as_name().ok()) == Some(b"OC") {
                        self.unsupported("pdf.optional_content")?;
                    }
                }
                _ => self.unsupported("pdf.unsupported_operator")?,
            }
            if self.body.len() + self.defs.len() > 4 * 1024 * 1024 {
                return Err(ImportError::LimitExceeded("pdf.svg_source"));
            }
        }
        if !stack.is_empty() {
            return Err(malformed());
        }
        text_position.finish()?;
        Ok(())
    }
}
fn load(bytes: &[u8]) -> Result<lopdf::Document, ImportError> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(ImportError::LimitExceeded("pdf.file"));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(ImportError::Malformed("pdf.header"));
    }
    let doc = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            strict: true,
            max_decompressed_size: Some(LIMIT),
            ..Default::default()
        },
    )
    .map_err(|_| ImportError::Malformed("pdf.document"))?;
    if doc.is_encrypted() {
        return Err(ImportError::Unsupported("pdf.encrypted"));
    }
    if doc.objects.len() > 100_000 {
        return Err(ImportError::LimitExceeded("pdf.objects"));
    }
    Ok(doc)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub index: u32,
    pub width_points: f32,
    pub height_points: f32,
}
/// Inspect page geometry without decoding artwork or mutating a document.
pub fn inspect(bytes: &[u8]) -> Result<Vec<PageInfo>, ImportError> {
    let doc = load(bytes)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(ImportError::Malformed("pdf.page_index"));
    }
    if pages.len() > 4096 {
        return Err(ImportError::LimitExceeded("pdf.pages"));
    }
    pages
        .values()
        .enumerate()
        .map(|(index, id)| {
            let bbox = inherited(&doc, *id, b"CropBox")?
                .or(inherited(&doc, *id, b"MediaBox")?)
                .ok_or_else(malformed)?;
            let p = nums(bbox.as_array().map_err(|_| malformed())?, 4)?;
            let unit = doc
                .get_dictionary(*id)
                .map_err(|_| malformed())?
                .get(b"UserUnit")
                .ok()
                .map(|v| resolve(&doc, v).and_then(num))
                .transpose()?
                .unwrap_or(1.);
            let rotate = inherited(&doc, *id, b"Rotate")?
                .map(num)
                .transpose()?
                .unwrap_or(0.);
            if unit <= 0. || unit > 75000. || rotate.fract() != 0. {
                return Err(malformed());
            }
            let rotate = (rotate as i32).rem_euclid(360);
            if ![0, 90, 180, 270].contains(&rotate) {
                return Err(ImportError::Unsupported("pdf.page_rotation"));
            }
            let (w, h) = ((p[2] - p[0]) * unit, (p[3] - p[1]) * unit);
            if w <= 0. || h <= 0. || !w.is_finite() || !h.is_finite() {
                return Err(malformed());
            }
            let (width_points, height_points) = if rotate == 90 || rotate == 270 {
                (h, w)
            } else {
                (w, h)
            };
            Ok(PageInfo {
                index: index as u32,
                width_points,
                height_points,
            })
        })
        .collect()
}
pub fn read(bytes: &[u8], options: ReadOptions) -> Result<ReadDocument, ImportError> {
    options.validate(FormatId::Pdf)?;
    if options.raster_dpi > 1200 {
        return Err(ImportError::Unsupported("pdf.document_resolution"));
    }
    let doc = load(bytes)?;
    let pages = doc.get_pages();
    read_page(&doc, &pages, options, true)
}
/// Decode the shared PDF object graph once and construct an atomic publication.
pub fn read_all(bytes: &[u8], options: ReadOptions) -> Result<ReadDocument, ImportError> {
    read_publication(bytes, options, None)
}
/// Decode selected pages once, in source order, without duplicates.
pub fn read_selected(
    bytes: &[u8],
    options: ReadOptions,
    selected: &[u32],
) -> Result<ReadDocument, ImportError> {
    read_publication(bytes, options, Some(selected))
}
fn read_publication(
    bytes: &[u8],
    options: ReadOptions,
    selected: Option<&[u32]>,
) -> Result<ReadDocument, ImportError> {
    options.validate(FormatId::Pdf)?;
    if options.raster_dpi > 1200 {
        return Err(ImportError::Unsupported("pdf.document_resolution"));
    }
    let doc = load(bytes)?;
    let pages = doc.get_pages();
    let indices: std::collections::BTreeSet<u32> = selected.map_or_else(
        || (0..pages.len() as u32).collect(),
        |s| s.iter().copied().collect(),
    );
    if indices.is_empty()
        || indices.len() > 512
        || indices.iter().any(|i| *i as usize >= pages.len())
    {
        return Err(ImportError::LimitExceeded("pdf.publication_pages"));
    }
    let mut states = Vec::with_capacity(indices.len());
    let mut report = ConversionReport::default();
    let mut total = 0usize;
    for index in indices {
        let decoded = read_page(
            &doc,
            &pages,
            ReadOptions {
                page_index: index,
                ..options
            },
            false,
        )?;
        let ReadContent::Vector(document) = decoded.content else {
            return Err(malformed());
        };
        for layer in document.svg_layers() {
            total = total
                .checked_add(layer.source.len())
                .ok_or(ImportError::LimitExceeded("pdf.publication_content"))?;
        }
        if total > 128 * 1024 * 1024 {
            return Err(ImportError::LimitExceeded("pdf.publication_content"));
        }
        states.push(document.document_state());
        for issue in decoded.report.issues {
            if !report.issues.contains(&issue) {
                report.issues.push(issue);
            }
        }
    }
    let mut first = states.remove(0);
    if !states.is_empty() {
        use lumapaint_core::document::{PageBinding, PageBookState, PageState};
        let count = states.len() + 1;
        let mut records = vec![PageState {
            id: "page-1".into(),
            content: None,
        }];
        records.extend(states.into_iter().enumerate().map(|(i, state)| PageState {
            id: format!("page-{}", i + 2),
            content: Some(Box::new(state)),
        }));
        first.pages = Some(PageBookState {
            facing: false,
            binding: PageBinding::LeftToRight,
            active: 0,
            next_id: count as u64 + 1,
            pages: records,
        });
    }
    let document =
        lumapaint_core::document::Document::from_document_state(first).map_err(|_| malformed())?;
    Ok(ReadDocument {
        content: ReadContent::Vector(Box::new(document)),
        report,
    })
}
fn read_page(
    doc: &lopdf::Document,
    pages: &std::collections::BTreeMap<u32, ObjectId>,
    options: ReadOptions,
    selected_only: bool,
) -> Result<ReadDocument, ImportError> {
    let id = *pages
        .values()
        .nth(options.page_index as usize)
        .ok_or(ImportError::Malformed("pdf.page_index"))?;
    let bbox = inherited(doc, id, b"CropBox")?
        .or(inherited(doc, id, b"MediaBox")?)
        .ok_or_else(malformed)?;
    let p = nums(bbox.as_array().map_err(|_| malformed())?, 4)?;
    let (w, h) = (p[2] - p[0], p[3] - p[1]);
    if w <= 0. || h <= 0. {
        return Err(malformed());
    }
    let rotate = inherited(doc, id, b"Rotate")?
        .map(num)
        .transpose()?
        .unwrap_or(0.);
    if rotate.fract() != 0. {
        return Err(malformed());
    }
    let rotate = (rotate as i32).rem_euclid(360);
    let user_unit = doc
        .get_dictionary(id)
        .map_err(|_| malformed())?
        .get(b"UserUnit")
        .ok()
        .map(|o| resolve(doc, o).and_then(num))
        .transpose()?
        .unwrap_or(1.);
    if user_unit <= 0. || user_unit > 75000. {
        return Err(malformed());
    }
    let scale = options.raster_dpi as f32 / 72. * user_unit;
    let transform = match rotate {
        0 => [scale, 0., 0., -scale, -p[0] * scale, p[3] * scale],
        90 => [0., scale, scale, 0., -p[1] * scale, -p[0] * scale],
        180 => [-scale, 0., 0., scale, p[2] * scale, -p[1] * scale],
        270 => [0., -scale, -scale, 0., p[3] * scale, p[2] * scale],
        _ => return Err(ImportError::Unsupported("pdf.page_rotation")),
    };
    let (width, height) = if rotate == 90 || rotate == 270 {
        (h * scale, w * scale)
    } else {
        (w * scale, h * scale)
    };
    if width > 8192. || height > 8192. {
        return Err(ImportError::LimitExceeded("pdf.page_size"));
    }
    let empty = Dictionary::new();
    let resources = inherited(doc, id, b"Resources")?
        .map(|o| o.as_dict().map_err(|_| malformed()))
        .transpose()?
        .unwrap_or(&empty);
    let mut parser = Interpreter {
        doc,
        body: String::new(),
        defs: String::new(),
        issues: vec![],
        remaining: LIMIT,
        operations: 0,
        serial: 0,
        allow_lossy: options.allow_lossy,
        page_bounds: p.clone().try_into().unwrap(),
        font_cache: Default::default(),
        text_glyphs: 0,
        glyph_depth: 0,
        image_remaining: 64 * 1024 * 1024,
        image_cache: Default::default(),
    };
    if selected_only && pages.len() > 1 {
        parser.unsupported("pdf.selected_page_only")?;
    }
    if doc
        .get_dictionary(id)
        .map_err(|_| malformed())?
        .get(b"Annots")
        .is_ok()
    {
        parser.unsupported("pdf.annotations_omitted")?;
    }
    let data = doc
        .get_page_content_with_limit(id, LIMIT)
        .map_err(|_| ImportError::LimitExceeded("pdf.page_content"))?;
    parser.content(&data, resources, State::default(), 0)?;
    let [a, b, c, d, e, f] = transform;
    let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><defs>{}</defs><g transform=\"matrix({a} {b} {c} {d} {e} {f})\">{}</g></svg>",width.ceil(),height.ceil(),parser.defs,parser.body);
    let document = crate::svg::import(format!("PDF page {}", options.page_index + 1), source)
        .map_err(|_| malformed())?;
    let mut state = document.document_state();
    state.resolution = Some(options.raster_dpi);
    let document =
        lumapaint_core::document::Document::from_document_state(state).map_err(|_| malformed())?;
    Ok(ReadDocument {
        content: ReadContent::Vector(Box::new(document)),
        report: ConversionReport {
            issues: parser.issues,
        },
    })
}

#[cfg(test)]
mod transparency_tests {
    use super::*;
    use lopdf::{dictionary, Stream};
    fn interpret(isolated: bool, knockout: bool, alpha: f32) -> Result<String, ImportError> {
        let mut doc = lopdf::Document::new();
        let form = doc.add_object(Stream::new(
            dictionary! {
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Group" => dictionary! { "S" => "Transparency", "I" => isolated, "K" => knockout },
            },
            b"0 0 10 10 re f".to_vec(),
        ));
        let resources = dictionary! { "XObject" => dictionary! { "F" => form } };
        let mut parser = Interpreter {
            doc: &doc,
            body: String::new(),
            defs: String::new(),
            issues: vec![],
            remaining: LIMIT,
            operations: 0,
            serial: 0,
            allow_lossy: false,
            page_bounds: [0., 0., 10., 10.],
            font_cache: Default::default(),
            text_glyphs: 0,
            glyph_depth: 0,
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
        };
        parser.content(
            b"/F Do",
            &resources,
            State {
                fill_alpha: alpha,
                ..Default::default()
            },
            0,
        )?;
        Ok(parser.body)
    }
    #[test]
    fn supported_groups_apply_outer_alpha_once() {
        let body = interpret(true, false, 0.5).unwrap();
        assert!(body.contains("opacity=\"0.5\""));
        assert!(body.contains("fill-opacity=\"1\""));
        assert!(body.contains("isolation:isolate"));
        assert!(interpret(false, false, 1.)
            .unwrap()
            .contains("isolation:auto"));
    }
    #[test]
    fn knockout_and_nonisolated_group_opacity_require_consent() {
        for (isolated, knockout, alpha) in [(true, true, 1.), (false, false, 0.5)] {
            assert!(matches!(
                interpret(isolated, knockout, alpha),
                Err(ImportError::LossyConversionRequiresConsent(_))
            ));
        }
    }
}
