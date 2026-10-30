//! Bounded, filesystem-free IDML reader. No network or external link resolution.
use super::*;
use crate::{
    io::{ReadContent, ReadDocument, ReadOptions},
    ImportError as E,
};
use base64::Engine;
use lumapaint_core::{
    document::{PageBookState, PageState, PageTextFrame, PageTextThread, SvgLayer, TextLayout},
    vector::{TextRun, VectorObjectKind, VectorPaint, VectorPath},
};
use std::io::Read;
type N<'a> = roxmltree::Node<'a, 'a>;
fn bad() -> E {
    E::Malformed("idml.document")
}
fn unsupported() -> E {
    E::Unsupported("idml.feature")
}
fn attr<'a>(n: N<'a>, key: &str) -> Option<&'a str> {
    n.attribute(key)
}
fn numbers(s: &str) -> Result<Vec<f32>, E> {
    let v: Vec<f32> = s
        .split_whitespace()
        .map(|v| v.parse().map_err(|_| bad()))
        .collect::<Result<_, _>>()?;
    if v.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
        return Err(bad());
    }
    Ok(v)
}
fn num(n: N<'_>, key: &str, default: f32) -> Result<f32, E> {
    match attr(n, key) {
        Some(s) => {
            let v = numbers(s)?;
            if v.len() != 1 {
                return Err(bad());
            }
            Ok(v[0])
        }
        None => Ok(default),
    }
}
fn matrix(n: N<'_>) -> Result<[f32; 6], E> {
    match attr(n, "ItemTransform") {
        Some(s) => numbers(s)?.try_into().map_err(|_| bad()),
        None => Ok([1., 0., 0., 1., 0., 0.]),
    }
}
fn flag(n: N<'_>, key: &str, default: bool) -> bool {
    attr(n, key).map_or(default, |v| v == "true")
}
fn prop<'a>(n: N<'a>, key: &str) -> Option<N<'a>> {
    n.children()
        .find(|v| v.has_tag_name("Properties"))?
        .children()
        .find(|v| v.has_tag_name(key))
}
fn path(n: N<'_>) -> Result<(VectorPath, [f32; 4]), E> {
    let p = prop(n, "PathGeometry").ok_or_else(unsupported)?;
    let mut data = String::new();
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for contour in p.children().filter(|v| v.has_tag_name("GeometryPathType")) {
        let mut points = Vec::new();
        for point in contour
            .descendants()
            .filter(|v| v.has_tag_name("PathPointType"))
        {
            let a: [f32; 2] = numbers(attr(point, "Anchor").ok_or_else(bad)?)?
                .try_into()
                .map_err(|_| bad())?;
            let l: [f32; 2] =
                numbers(attr(point, "LeftDirection").unwrap_or(attr(point, "Anchor").unwrap()))?
                    .try_into()
                    .map_err(|_| bad())?;
            let r: [f32; 2] =
                numbers(attr(point, "RightDirection").unwrap_or(attr(point, "Anchor").unwrap()))?
                    .try_into()
                    .map_err(|_| bad())?;
            bounds = [
                bounds[0].min(a[0]),
                bounds[1].min(a[1]),
                bounds[2].max(a[0]),
                bounds[3].max(a[1]),
            ];
            points.push([a, l, r]);
        }
        if points.len() < 2 {
            return Err(bad());
        }
        let first = points[0];
        let _ = write!(data, "M{} {}", first[0][0], first[0][1]);
        let mut prev = first;
        for next in points
            .iter()
            .skip(1)
            .chain((!flag(contour, "PathOpen", false)).then_some(&first))
        {
            let _ = write!(
                data,
                "C{} {} {} {} {} {}",
                prev[2][0], prev[2][1], next[1][0], next[1][1], next[0][0], next[0][1]
            );
            prev = *next;
        }
        if !flag(contour, "PathOpen", false) {
            data.push('Z');
        }
    }
    if data.is_empty() {
        return Err(bad());
    }
    Ok((
        VectorPath {
            data,
            fill_rule: Default::default(),
        },
        bounds,
    ))
}
pub(super) fn image_info(bytes: &[u8]) -> Result<(u32, u32, &'static str), E> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut r = png::Decoder::new(Cursor::new(bytes))
            .read_info()
            .map_err(|_| bad())?;
        let (w, h) = (r.info().width, r.info().height);
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_777_216 {
            return Err(E::LimitExceeded("idml.image"));
        }
        if r.info().animation_control.is_some() {
            return Err(unsupported());
        }
        let size = r
            .output_buffer_size()
            .ok_or(E::LimitExceeded("idml.image"))?;
        if size > 128 * 1024 * 1024 {
            return Err(E::LimitExceeded("idml.image"));
        }
        let mut decoded = vec![0; size];
        r.next_frame(&mut decoded).map_err(|_| bad())?;
        Ok((w, h, "PNG"))
    } else if bytes.starts_with(&[255, 216]) {
        use image::ImageDecoder;
        let r = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(|_| bad())?;
        let (w, h) = r.dimensions();
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_777_216 {
            return Err(E::LimitExceeded("idml.image"));
        }
        if r.total_bytes() > 64 * 1024 * 1024 {
            return Err(E::LimitExceeded("idml.image"));
        }
        let mut decoded = vec![0; r.total_bytes() as usize];
        r.read_image(&mut decoded).map_err(|_| bad())?;
        Ok((w, h, "JPEG"))
    } else {
        Err(unsupported())
    }
}
struct Package {
    files: BTreeMap<String, Vec<u8>>,
}
impl Package {
    fn new(bytes: &[u8]) -> Result<Self, E> {
        if bytes.len() > LIMIT {
            return Err(E::LimitExceeded("idml.archive"));
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| bad())?;
        if zip.len() > 8192 {
            return Err(E::LimitExceeded("idml.entries"));
        }
        let mut files = BTreeMap::new();
        let mut size = 0usize;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|_| bad())?;
            let name = f.name().to_owned();
            if name.starts_with('/')
                || name.contains('\\')
                || name.contains(':')
                || name.split('/').any(|c| c == ".." || c == ".")
                || name.len() > 1024
            {
                return Err(bad());
            }
            if f.is_dir() {
                continue;
            }
            if f.size() > LIMIT as u64 || f.encrypted() {
                return Err(E::LimitExceeded("idml.entry"));
            }
            let mut data = Vec::new();
            f.by_ref()
                .take((LIMIT - size + 1) as u64)
                .read_to_end(&mut data)
                .map_err(|_| bad())?;
            size += data.len();
            if size > LIMIT {
                return Err(E::LimitExceeded("idml.expanded"));
            }
            if files.insert(name, data).is_some() {
                return Err(bad());
            }
        }
        if files.get("mimetype").map(Vec::as_slice) != Some(MIME.as_bytes()) {
            return Err(bad());
        }
        Ok(Self { files })
    }
    fn xml(&self, name: &str) -> Result<roxmltree::Document<'_>, E> {
        let s = std::str::from_utf8(self.files.get(name).ok_or_else(bad)?).map_err(|_| bad())?;
        roxmltree::Document::parse(s).map_err(|_| bad())
    }
}
fn color(
    n: N<'_>,
    key: &str,
    colors: &BTreeMap<String, [u8; 3]>,
) -> Result<Option<VectorPaint>, E> {
    let Some(id) = attr(n, key) else {
        return Ok(None);
    };
    if id == "Swatch/None" {
        return Ok(None);
    }
    let c = colors.get(id).ok_or_else(unsupported)?;
    Ok(Some(VectorPaint {
        color: [c[0], c[1], c[2], 255],
        registration: false,
    }))
}
fn style(
    n: N<'_>,
    base: &lumapaint_core::vector::TextStyle,
    colors: &BTreeMap<String, [u8; 3]>,
) -> Result<lumapaint_core::vector::TextStyle, E> {
    let mut s = base.clone();
    if let Some(f) = prop(n, "AppliedFont").and_then(|n| n.text()) {
        s.font_family = f.into();
    }
    if let Some(v) = attr(n, "FontStyle") {
        s.bold = v.contains("Bold");
        s.italic = v.contains("Italic") || v.contains("Oblique");
    }
    s.font_size = num(n, "PointSize", s.font_size)?;
    s.tracking = num(n, "Tracking", s.tracking)?;
    s.scale_x = num(n, "HorizontalScale", s.scale_x * 100.)? / 100.;
    s.scale_y = num(n, "VerticalScale", s.scale_y * 100.)? / 100.;
    s.baseline_shift = num(n, "BaselineShift", s.baseline_shift)?;
    s.rotation = num(n, "CharacterRotation", s.rotation)?;
    if let Some(k) = attr(n, "KerningMethod") {
        s.kerning = match k {
            "$ID/Optical" => lumapaint_core::vector::KerningMode::Optical,
            "$ID/None" => lumapaint_core::vector::KerningMode::JapaneseMonospaced,
            _ => Default::default(),
        };
    }
    s.underline = flag(n, "Underline", s.underline);
    s.strikethrough = flag(n, "StrikeThru", s.strikethrough);
    s.rotate_latin = flag(n, "RotateSingleByteCharacters", s.rotate_latin);
    s.tate_chu_yoko = flag(n, "Tatechuyoko", s.tate_chu_yoko);
    if attr(n, "FillColor").is_some() {
        let c = color(n, "FillColor", colors)?;
        s.no_color = c.is_none();
        if let Some(c) = c {
            s.color = [c.color[0], c.color[1], c.color[2]];
        }
    }
    if !matches!(attr(n, "StrokeColor"), None | Some("Swatch/None")) {
        return Err(unsupported());
    }
    Ok(s)
}
fn story(
    n: N<'_>,
    colors: &BTreeMap<String, [u8; 3]>,
    styles: &BTreeMap<String, N<'_>>,
) -> Result<VectorText, E> {
    let mut t = VectorText::default();
    t.content.clear();
    let base = t.style_at(0, [0; 3]);
    let mut at = 0;
    if n.children().any(|v| {
        v.has_tag_name("StoryPreference") && attr(v, "StoryOrientation") == Some("Vertical")
    }) {
        t.writing_mode = WritingMode::Vertical;
    }
    for p in n
        .children()
        .filter(|v| v.has_tag_name("ParagraphStyleRange"))
    {
        let mut ps = base.clone();
        if let Some(s) = attr(p, "AppliedParagraphStyle").and_then(|id| styles.get(id)) {
            ps = style(*s, &ps, colors)?;
        }
        ps = style(p, &ps, colors)?;
        // Native paragraphs currently share settings; preserve the first and report recomposition.
        if at == 0 {
            t.alignment = match attr(p, "Justification") {
                Some("CenterAlign") => lumapaint_core::vector::TextAlignment::Center,
                Some("RightAlign") => lumapaint_core::vector::TextAlignment::Right,
                Some("LeftJustified" | "FullyJustified") => {
                    lumapaint_core::vector::TextAlignment::Justify
                }
                _ => Default::default(),
            };
            t.indent_left = num(p, "LeftIndent", 0.)?;
            t.indent_right = num(p, "RightIndent", 0.)?;
            t.indent_first = num(p, "FirstLineIndent", 0.)?;
            t.space_before = num(p, "SpaceBefore", 0.)?;
            t.space_after = num(p, "SpaceAfter", 0.)?;
            t.hyphenation = flag(p, "Hyphenation", false);
        }
        for c in p
            .children()
            .filter(|v| v.has_tag_name("CharacterStyleRange"))
        {
            let mut cs = ps.clone();
            if let Some(s) = attr(c, "AppliedCharacterStyle").and_then(|id| styles.get(id)) {
                cs = style(*s, &cs, colors)?;
            }
            cs = style(c, &cs, colors)?;
            let start = at;
            for v in c.children().filter(|v| v.is_element()) {
                match v.tag_name().name() {
                    "Content" => {
                        let s = v.text().unwrap_or("");
                        t.content.push_str(s);
                        at += s.encode_utf16().count();
                    }
                    "Br" => {
                        t.content.push('\n');
                        at += 1;
                    }
                    "Properties" => {}
                    _ => return Err(unsupported()),
                }
            }
            if at > start {
                t.runs.push(TextRun {
                    start,
                    end: at,
                    style: cs.clone(),
                });
            }
            if start == 0 {
                t.font_family = cs.font_family.clone();
                t.font_size = cs.font_size;
                t.bold = cs.bold;
                t.italic = cs.italic;
                t.kerning = cs.kerning;
                t.no_color = cs.no_color;
                t.tracking = cs.tracking;
                t.baseline_shift = cs.baseline_shift;
                t.underline = cs.underline;
                t.strikethrough = cs.strikethrough;
                t.rotate_latin = cs.rotate_latin;
                t.tate_chu_yoko = cs.tate_chu_yoko;
                if let Some(l) = prop(c, "Leading").filter(|v| attr(*v, "type") == Some("unit")) {
                    let v = numbers(l.text().unwrap_or(""))?;
                    if v.len() == 1 {
                        t.line_height = v[0] / t.font_size;
                    }
                }
            }
            if at > 65536 {
                return Err(E::LimitExceeded("idml.story"));
            }
        }
    }
    t.validate().map_err(|_| bad())?;
    Ok(t)
}

pub fn read(bytes: &[u8], options: ReadOptions) -> Result<ReadDocument, E> {
    read_with_layout(bytes, options, None)
}
#[derive(Clone)]
pub struct ResolvedImage {
    pub bytes: Vec<u8>,
    pub source_path: Option<String>,
}
pub fn read_with_layout(
    bytes: &[u8],
    options: ReadOptions,
    layout: Option<TextLayout>,
) -> Result<ReadDocument, E> {
    read_with_links(bytes, options, layout, &mut |_| {
        Err(E::Unsupported("idml.external_image"))
    })
}
/// The caller owns link access policy. The format adapter never opens files or URLs.
pub fn read_with_links(
    bytes: &[u8],
    options: ReadOptions,
    layout: Option<TextLayout>,
    resolver: &mut dyn FnMut(&str) -> Result<ResolvedImage, E>,
) -> Result<ReadDocument, E> {
    options.validate(FormatId::Idml)?;
    let package = Package::new(bytes)?;
    let mut external_bytes = 0usize;
    let mut resolved_links: BTreeMap<String, ResolvedImage> = BTreeMap::new();
    let design = package.xml("designmap.xml")?;
    let mut docs = Vec::new();
    for r in design
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(NS))
    {
        if let Some(src) = attr(r, "src") {
            docs.push(package.xml(src)?);
        }
    }
    let mut ids = BTreeSet::new();
    for d in std::iter::once(&design).chain(docs.iter()) {
        for n in d.descendants() {
            if let Some(id) = attr(n, "Self") {
                if !ids.insert(id) {
                    return Err(bad());
                }
            }
        }
    }
    let mut colors = BTreeMap::from([
        ("Color/Black".into(), [0; 3]),
        ("Color/Paper".into(), [255; 3]),
    ]);
    for n in docs
        .iter()
        .flat_map(|d| d.descendants())
        .filter(|n| n.has_tag_name("Color"))
    {
        if let (Some(id), Some(v)) = (attr(n, "Self"), attr(n, "ColorValue")) {
            let v = numbers(v)?;
            let c = match attr(n, "Space") {
                Some("RGB") if v.len() == 3 => [v[0], v[1], v[2]],
                Some("CMYK") if v.len() == 4 => [
                    (1. - v[0] / 100.) * (1. - v[3] / 100.) * 255.,
                    (1. - v[1] / 100.) * (1. - v[3] / 100.) * 255.,
                    (1. - v[2] / 100.) * (1. - v[3] / 100.) * 255.,
                ],
                _ => return Err(unsupported()),
            };
            colors.insert(id.into(), c.map(|v| v.clamp(0., 255.).round() as u8));
        }
    }
    let styles: BTreeMap<_, _> = docs
        .iter()
        .flat_map(|d| d.descendants())
        .filter(|n| matches!(n.tag_name().name(), "ParagraphStyle" | "CharacterStyle"))
        .filter_map(|n| Some((attr(n, "Self")?.to_owned(), n)))
        .collect();
    let stories: BTreeMap<_, _> = docs
        .iter()
        .flat_map(|d| d.descendants())
        .filter(|n| n.has_tag_name("Story") && attr(*n, "Self").is_some())
        .map(|n| {
            Ok((
                attr(n, "Self").unwrap().to_owned(),
                story(n, &colors, &styles)?,
            ))
        })
        .collect::<Result<_, E>>()?;
    let layer_defs: Vec<_> = design
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Layer"))
        .collect();
    let all_frames: Vec<_> = docs
        .iter()
        .flat_map(|d| d.descendants())
        .filter(|n| n.has_tag_name("Spread") && attr(*n, "Self").is_some())
        .flat_map(|s| s.children().filter(|n| n.has_tag_name("TextFrame")))
        .collect();
    let frame_map: BTreeMap<_, _> = all_frames
        .iter()
        .map(|n| Ok((attr(*n, "Self").ok_or_else(bad)?, *n)))
        .collect::<Result<_, E>>()?;
    let mut seen_frames = BTreeSet::new();
    let mut used_stories = BTreeSet::new();
    let mut chains = Vec::new();
    let mut text_by_frame = BTreeMap::new();
    for first in all_frames
        .iter()
        .filter(|n| matches!(attr(**n, "PreviousTextFrame"), None | Some("n")))
    {
        let sid = attr(*first, "ParentStory").ok_or_else(bad)?;
        if !used_stories.insert(sid) {
            return Err(bad());
        }
        let source = stories.get(sid).ok_or_else(bad)?;
        let mut frame = *first;
        let mut chain = Vec::new();
        loop {
            let id = attr(frame, "Self").ok_or_else(bad)?;
            if !seen_frames.insert(id) || attr(frame, "ParentStory") != Some(sid) {
                return Err(bad());
            }
            let mut text = source.clone();
            if !chain.is_empty() {
                text.content.clear();
                text.runs.clear();
            }
            text_by_frame.insert(id, text);
            chain.push(id.to_owned());
            if chain.len() > 64 {
                return Err(E::LimitExceeded("idml.story_frames"));
            }
            match attr(frame, "NextTextFrame") {
                None | Some("n") => break,
                Some(next) => {
                    let f = *frame_map.get(next).ok_or_else(bad)?;
                    if attr(f, "PreviousTextFrame") != Some(id) {
                        return Err(bad());
                    }
                    frame = f;
                }
            }
        }
        if chain.len() > 1 {
            chains.push(chain);
        }
    }
    if seen_frames.len() != all_frames.len() {
        return Err(bad());
    }
    let mut states = Vec::new();
    let mut object_count = 0;
    for spread in docs
        .iter()
        .flat_map(|d| d.descendants())
        .filter(|n| n.has_tag_name("Spread") && attr(*n, "Self").is_some())
    {
        let pages: Vec<_> = spread
            .children()
            .filter(|n| n.has_tag_name("Page"))
            .collect();
        if pages.is_empty() || states.len() + pages.len() > 512 {
            return Err(E::LimitExceeded("idml.pages"));
        }
        let mut owned = Vec::new();
        for n in spread.children().filter(|n| n.is_element()) {
            if matches!(
                n.tag_name().name(),
                "Page" | "FlattenerPreference" | "Properties" | "Guide"
            ) {
                continue;
            }
            if !matches!(
                n.tag_name().name(),
                "TextFrame" | "Polygon" | "Rectangle" | "Oval" | "GraphicLine"
            ) {
                return Err(unsupported());
            }
            let (_, b) = path(n)?;
            let m = matrix(n)?;
            let x = (b[0] + b[2]) * 0.5;
            let y = (b[1] + b[3]) * 0.5;
            let (x, y) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
            let mut owner = 0;
            let mut nearest = f32::INFINITY;
            for (index, p) in pages.iter().enumerate() {
                let v = numbers(attr(*p, "GeometricBounds").ok_or_else(bad)?)?;
                if v.len() != 4 {
                    return Err(bad());
                }
                let inv = lumapaint_core::image_frame::inverse(matrix(*p)?).map_err(|_| bad())?;
                let (px, py) = (
                    inv[0] * x + inv[2] * y + inv[4],
                    inv[1] * x + inv[3] * y + inv[5],
                );
                let dx = (v[1] - px).max(0.).max(px - v[3]);
                let dy = (v[0] - py).max(0.).max(py - v[2]);
                let distance = dx * dx + dy * dy;
                if distance < nearest {
                    nearest = distance;
                    owner = index;
                }
            }
            owned.push((owner, n));
        }
        for (page_index, page) in pages.iter().copied().enumerate() {
            let items: Vec<_> = owned
                .iter()
                .filter(|(i, _)| *i == page_index)
                .map(|(_, n)| *n)
                .collect();
            let bounds = numbers(attr(page, "GeometricBounds").ok_or_else(bad)?)?;
            if bounds.len() != 4 {
                return Err(bad());
            }
            let (w, h) = (
                (bounds[3] - bounds[1]).ceil(),
                (bounds[2] - bounds[0]).ceil(),
            );
            if !(1. ..=8192.).contains(&w) || !(1. ..=8192.).contains(&h) {
                return Err(E::LimitExceeded("idml.page"));
            }
            let inv = lumapaint_core::image_frame::inverse(matrix(page)?).map_err(|_| bad())?;
            let mut state = Document::default().document_state();
            state.width = w as u32;
            state.height = h as u32;
            state.resolution = Some(72);
            state.svg_layers.clear();
            for (index, l) in layer_defs.iter().enumerate() {
                state.svg_layers.push(SvgLayer {
                    motion_scene: None,
                    id: format!("layer-idml-{index}"),
                    name: attr(*l, "Name")
                        .unwrap_or("Layer")
                        .chars()
                        .take(120)
                        .collect(),
                    visible: flag(*l, "Visible", true),
                    locked: flag(*l, "Locked", false),
                    opacity: 1.,
                    alpha_locked: false,
                    mask_enabled: false,
                    mask_inverted: false,
                    mask_density: 1.,
                    source: String::new(),
                    paint_layer: false,
                    vector_layer: true,
                    vector_objects: vec![],
                });
            }
            if state.svg_layers.is_empty() {
                return Err(bad());
            }
            for n in items.iter().copied() {
                if matches!(
                    n.tag_name().name(),
                    "Page" | "FlattenerPreference" | "Properties"
                ) {
                    continue;
                }
                if !matches!(
                    n.tag_name().name(),
                    "TextFrame" | "Polygon" | "Rectangle" | "Oval" | "GraphicLine"
                ) {
                    return Err(unsupported());
                }
                object_count += 1;
                if object_count > MAX_FRAMES {
                    return Err(E::LimitExceeded("idml.objects"));
                }
                let id = attr(n, "Self").ok_or_else(bad)?.to_owned();
                let li = layer_defs
                    .iter()
                    .position(|l| attr(*l, "Self") == attr(n, "ItemLayer"))
                    .ok_or_else(bad)?;
                let (path, b) = path(n)?;
                let mut transform = lumapaint_core::image_frame::multiply(inv, matrix(n)?);
                transform[4] -= bounds[1];
                transform[5] -= bounds[0];
                let mut text = text_by_frame.get(id.as_str()).cloned();
                if let Some(t) = &mut text {
                    transform = lumapaint_core::image_frame::multiply(
                        transform,
                        [1., 0., 0., 1., b[0], b[1]],
                    );
                    t.box_width = b[2] - b[0];
                    t.box_height = Some(b[3] - b[1]);
                    let pref = n.children().find(|v| v.has_tag_name("TextFramePreference"));
                    if pref.is_some_and(|p| attr(p, "TextColumnCount").is_some_and(|v| v != "1")) {
                        return Err(unsupported());
                    }
                }
                let fill = if let Some(text) = &text {
                    let style = text.style_at(0, [0; 3]);
                    (!style.no_color).then_some(VectorPaint {
                        color: [style.color[0], style.color[1], style.color[2], 255],
                        registration: false,
                    })
                } else {
                    color(n, "FillColor", &colors)?
                };
                let mut object = VectorObject {
                    image_frame: None,
                    opacity: 1.,
                    blend_mode: "normal".into(),
                    id: id.clone(),
                    name: attr(n, "Name")
                        .unwrap_or("Object")
                        .chars()
                        .take(120)
                        .collect(),
                    group_path: vec![],
                    clipping_group: None,
                    bounds_reset: false,
                    path,
                    transform,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill,
                    stroke: color(n, "StrokeColor", &colors)?,
                    stroke_width: num(n, "StrokeWeight", 0.)?,
                    stroke_style: Default::default(),
                    live_corners: None,
                    rectangle_radii: None,
                    visible: flag(n, "Visible", true),
                    kind: if text.is_some() {
                        VectorObjectKind::Text
                    } else {
                        VectorObjectKind::Path
                    },
                    control_points: vec![],
                    text,
                };
                object.stroke_style.cap = match attr(n, "EndCap") {
                    Some("RoundEndCap") => lumapaint_core::stroke::LineCap::Round,
                    Some("ProjectingEndCap") => lumapaint_core::stroke::LineCap::Square,
                    _ => Default::default(),
                };
                object.stroke_style.join = match attr(n, "EndJoin") {
                    Some("RoundEndJoin") => lumapaint_core::stroke::LineJoin::Round,
                    Some("BevelEndJoin") => lumapaint_core::stroke::LineJoin::Bevel,
                    _ => Default::default(),
                };
                object.stroke_style.miter_limit = num(n, "MiterLimit", 4.)?;
                if let Some(blend) = n
                    .children()
                    .find(|v| v.has_tag_name("TransparencySetting"))
                    .and_then(|v| v.children().find(|v| v.has_tag_name("BlendingSetting")))
                {
                    if !matches!(attr(blend, "BlendMode"), None | Some("Normal")) {
                        return Err(unsupported());
                    }
                    object.opacity = num(blend, "Opacity", 100.)? / 100.;
                }
                if let Some(image) = n.children().find(|v| v.has_tag_name("Image")) {
                    let mut source_path = None;
                    let bytes = if let Some(c) = prop(image, "Contents") {
                        let encoded: String = c
                            .text()
                            .unwrap_or("")
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect();
                        base64::engine::general_purpose::STANDARD
                            .decode(encoded)
                            .map_err(|_| bad())?
                    } else {
                        let link = image
                            .children()
                            .find(|v| v.has_tag_name("Link"))
                            .ok_or_else(unsupported)?;
                        let uri = attr(link, "LinkResourceURI").ok_or_else(unsupported)?;
                        if let Some(bytes) =
                            package.files.get(uri.strip_prefix("file:").unwrap_or(uri))
                        {
                            bytes.clone()
                        } else {
                            if !resolved_links.contains_key(uri) {
                                if uri.len() > 4096 {
                                    return Err(E::LimitExceeded("idml.link_uri"));
                                }
                                let result = resolver(uri)?;
                                external_bytes = external_bytes
                                    .checked_add(result.bytes.len())
                                    .ok_or(E::LimitExceeded("idml.link_bytes"))?;
                                if external_bytes > LIMIT || result.bytes.len() > 3 * 1024 * 1024 {
                                    return Err(E::LimitExceeded("idml.link_bytes"));
                                }
                                resolved_links.insert(uri.into(), result);
                            }
                            let result = &resolved_links[uri];
                            source_path = result.source_path.clone();
                            result.bytes.clone()
                        }
                    };
                    if bytes.len() > 3 * 1024 * 1024 {
                        return Err(E::LimitExceeded("idml.image"));
                    }
                    let (w, h, kind) = image_info(&bytes)?;
                    let gb = prop(image, "GraphicBounds").ok_or_else(unsupported)?;
                    let l = num(gb, "Left", 0.)?;
                    let t = num(gb, "Top", 0.)?;
                    let r = num(gb, "Right", w as f32)?;
                    let bot = num(gb, "Bottom", h as f32)?;
                    let fit = lumapaint_core::image_frame::multiply(
                        matrix(image)?,
                        [(r - l) / w as f32, 0., 0., (bot - t) / h as f32, l, t],
                    );
                    use sha2::{Digest, Sha256};
                    object.image_frame = Some(lumapaint_core::image_frame::ImageFrame {
                        image: Some(lumapaint_core::image_frame::FrameImage {
                            source_path,
                            fingerprint: Sha256::digest(&bytes)
                                .iter()
                                .map(|v| format!("{v:02x}"))
                                .collect(),
                            name: attr(image, "Name")
                                .unwrap_or("Image")
                                .chars()
                                .take(120)
                                .collect(),
                            width: w,
                            height: h,
                            encoded_width: w,
                            encoded_height: h,
                            orientation_transform: [1., 0., 0., 1., 0., 0.],
                            data_uri: format!(
                                "data:image/{};base64,{}",
                                if kind == "PNG" { "png" } else { "jpeg" },
                                base64::engine::general_purpose::STANDARD.encode(&bytes)
                            ),
                        }),
                        fitting: Default::default(),
                        content_transform: Some(fit),
                    });
                }
                if n.children()
                    .any(|v| matches!(v.tag_name().name(), "PDF" | "EPS" | "Group"))
                {
                    return Err(unsupported());
                }
                object.validate().map_err(|_| bad())?;
                if flag(n, "Locked", false) {
                    state.locked_objects.insert(id);
                }
                state.svg_layers[li].vector_objects.push(object);
            }
            for l in &mut state.svg_layers {
                l.source = lumapaint_core::document::vector_svg(
                    state.width,
                    state.height,
                    &l.vector_objects,
                );
            }
            let mut document = Document::from_document_state(state).map_err(|_| bad())?;
            if let Some(layout) = layout {
                document
                    .recompose_imported_text(layout)
                    .map_err(|_| bad())?;
            }
            states.push(document.document_state());
        }
    }
    if states.is_empty() || states.len() > 512 {
        return Err(E::LimitExceeded("idml.pages"));
    }
    let mut publication_threads = Vec::new();
    for (i, chain) in chains.into_iter().enumerate() {
        let locations: Vec<_> = chain
            .iter()
            .map(|id| {
                states
                    .iter()
                    .position(|s| {
                        s.svg_layers
                            .iter()
                            .flat_map(|l| &l.vector_objects)
                            .any(|o| &o.id == id)
                    })
                    .ok_or_else(bad)
            })
            .collect::<Result<_, _>>()?;
        if locations.iter().all(|p| *p == locations[0]) {
            states[locations[0]].vector_assets.threads.push(serde_json::from_value(serde_json::json!({"id":format!("thread-{i}"),"name":format!("Story {}",i+1),"frames":chain})).map_err(|_|bad())?);
        } else {
            publication_threads.push(PageTextThread {
                id: format!("publication-story-{i}"),
                name: format!("Story {}", i + 1),
                frames: chain
                    .into_iter()
                    .zip(locations)
                    .map(|(id, page)| PageTextFrame {
                        page_id: format!("page-{}", page + 1),
                        object_id: id,
                    })
                    .collect(),
            });
        }
    }
    if let Some(layout) = layout {
        for state in &mut states {
            let mut page = Document::from_document_state(state.clone()).map_err(|_| bad())?;
            page.recompose_imported_text(layout).map_err(|_| bad())?;
            *state = page.document_state();
        }
    }
    let preferences = docs
        .iter()
        .flat_map(|d| d.descendants())
        .find(|n| n.has_tag_name("DocumentPreference"));
    let facing = preferences.is_some_and(|n| flag(n, "FacingPages", false));
    let binding = if preferences.is_some_and(|n| attr(n, "PageBinding") == Some("RightToLeft")) {
        PageBinding::RightToLeft
    } else {
        PageBinding::LeftToRight
    };
    let mut first = states.remove(0);
    if !states.is_empty() {
        let mut pages = vec![PageState {
            id: "page-1".into(),
            content: None,
        }];
        for (i, s) in states.into_iter().enumerate() {
            pages.push(PageState {
                id: format!("page-{}", i + 2),
                content: Some(Box::new(s)),
            });
        }
        first.pages = Some(PageBookState {
            text_threads: publication_threads,
            facing,
            binding,
            active: 0,
            next_id: pages.len() as u64 + 1,
            pages,
        });
    }
    let mut document = Document::from_document_state(first).map_err(|_| bad())?;
    if let Some(layout) = layout {
        document
            .recompose_imported_publication_text(layout)
            .map_err(|_| bad())?;
    }
    let mut report = ConversionReport::default();
    issue(&mut report, "idml.import_recomposed", CompatibilityTier::B);
    if !resolved_links.is_empty() {
        issue(
            &mut report,
            "idml.external_images_resolved",
            CompatibilityTier::B,
        );
    }
    if (0..document.pages_snapshot().pages.len()).any(|i| {
        document.page_document(i).is_some_and(|p| {
            p.svg_layers().any(|l| {
                l.vector_objects
                    .iter()
                    .any(|o| o.image_frame.as_ref().is_some_and(|f| f.image.is_some()))
            })
        })
    }) {
        issue(
            &mut report,
            "idml.image_preview_embedded",
            CompatibilityTier::B,
        );
    }
    issue(
        &mut report,
        "idml.import_advanced_settings",
        CompatibilityTier::D,
    );
    if !options.allow_lossy {
        return Err(E::LossyConversionRequiresConsent(report));
    }
    Ok(ReadDocument {
        content: ReadContent::Vector(Box::new(document)),
        report,
    })
}
