//! Bounded editable text/path/image publication -> IDML package. No UI state or rasterization.
//! IDML is an exchange package, not an INDD writer; unsupported artwork is rejected.
use crate::export::{
    DocumentExporter, ExportError, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};
use crate::{CompatibilityTier, ConversionIssue, ConversionReport};
use lumapaint_core::{
    document::{Document, DocumentState, PageBinding},
    vector::{TextStyle, VectorObject, VectorText, WritingMode},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    io::{Cursor, Write as IoWrite},
};

mod artwork;
pub mod read;
mod story;
const NS: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const MIME: &str = "application/vnd.adobe.indesign-idml-package";
const LIMIT: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 4096;
const PARAGRAPH: &str = "ParagraphStyle/$ID/NormalParagraphStyle";
const CHARACTER: &str = "CharacterStyle/$ID/[No character style]";
fn unsupported() -> ExportError {
    ExportError::UnsupportedFeature("idml.native_text_only")
}
fn limit() -> ExportError {
    ExportError::UnsupportedFeature("idml.publication_limit")
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn package(kind: &str, body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><idPkg:{kind} xmlns:idPkg=\"{NS}\" DOMVersion=\"7.0\">{body}</idPkg:{kind}>")
}
fn issue(report: &mut ConversionReport, code: &'static str, tier: CompatibilityTier) {
    if !report.issues.iter().any(|i| i.code == code) {
        report.issues.push(ConversionIssue { code, tier });
    }
}
struct Archive {
    zip: zip::ZipWriter<Cursor<Vec<u8>>>,
    bytes: usize,
    count: usize,
}
impl Archive {
    fn new() -> Result<Self, ExportError> {
        let mut archive = Self {
            zip: zip::ZipWriter::new(Cursor::new(Vec::new())),
            bytes: 0,
            count: 0,
        };
        archive.add("mimetype", MIME)?;
        archive.add("META-INF/container.xml", "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/></rootfiles></container>")?;
        Ok(archive)
    }
    fn add(&mut self, path: &str, data: &str) -> Result<(), ExportError> {
        self.bytes = self.bytes.checked_add(data.len()).ok_or_else(limit)?;
        self.count += 1;
        if self.bytes > LIMIT || self.count > 8192 || path.len() > 128 {
            return Err(limit());
        }
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        self.zip
            .start_file(path, options)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        self.zip
            .write_all(data.as_bytes())
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))
    }
    fn finish(self) -> Result<Vec<u8>, ExportError> {
        let bytes = self
            .zip
            .finish()
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?
            .into_inner();
        if bytes.len() > LIMIT {
            return Err(limit());
        }
        Ok(bytes)
    }
}
#[derive(Clone)]
struct Frame<'a> {
    id: String,
    layer: String,
    object: &'a VectorObject,
    text: &'a VectorText,
    color: [u8; 3],
    locked: bool,
}
struct Resources {
    fonts: BTreeMap<(String, String), String>,
    colors: BTreeSet<[u8; 3]>,
    report: ConversionReport,
}
impl Resources {
    fn new() -> Self {
        Self {
            fonts: BTreeMap::new(),
            colors: BTreeSet::new(),
            report: ConversionReport::default(),
        }
    }
    fn font(&mut self, style: &TextStyle) -> (String, String) {
        let key = (
            style.font_family.clone(),
            format!("{}{}", style.bold, style.italic),
        );
        if let Some(value) = self.fonts.get(&key) {
            let (family, variant) = value.split_once('\t').unwrap();
            return (family.into(), variant.into());
        }
        let db = crate::svg::font_database();
        use usvg::fontdb::{Family, Query, Style, Weight};
        let family = match style.font_family.as_str() {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            name => Family::Name(name),
        };
        let exact = db
            .faces()
            .find(|f| f.post_script_name == style.font_family)
            .map(|f| f.id);
        let id = exact.or_else(|| {
            db.query(&Query {
                families: &[family],
                weight: if style.bold {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                },
                style: if style.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Default::default()
            })
        });
        let value = id
            .and_then(|id| {
                let info = db.face(id)?;
                let family = info.families.first()?.0.clone();
                let variant = db
                    .with_face_data(id, |data, index| {
                        let face = ttf_parser::Face::parse(data, index).ok()?;
                        [
                            ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY,
                            ttf_parser::name_id::SUBFAMILY,
                        ]
                        .iter()
                        .find_map(|id| {
                            face.names()
                                .into_iter()
                                .find_map(|n| (n.name_id == *id).then(|| n.to_string()).flatten())
                        })
                    })
                    .flatten()?;
                Some((family, variant))
            })
            .unwrap_or_else(|| {
                issue(
                    &mut self.report,
                    "idml.font_unavailable",
                    CompatibilityTier::D,
                );
                (
                    style.font_family.clone(),
                    match (style.bold, style.italic) {
                        (true, true) => "Bold Italic",
                        (true, false) => "Bold",
                        (false, true) => "Italic",
                        _ => "Regular",
                    }
                    .into(),
                )
            });
        self.fonts.insert(key, format!("{}\t{}", value.0, value.1));
        value
    }
    fn color(&mut self, color: [u8; 3]) -> String {
        self.colors.insert(color);
        format!("Color/LP{:02x}{:02x}{:02x}", color[0], color[1], color[2])
    }
    fn write(self, archive: &mut Archive) -> Result<ConversionReport, ExportError> {
        let mut fonts = String::new();
        let mut families: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for value in self.fonts.values() {
            let (family, variant) = value.split_once('\t').unwrap();
            families.entry(family).or_default().insert(variant);
        }
        for (i, (family, variants)) in families.into_iter().enumerate() {
            let family = xml(family);
            let _ = write!(
                fonts,
                "<FontFamily Self=\"lp_fontfamily_{i}\" Name=\"{family}\">"
            );
            for (j, variant) in variants.into_iter().enumerate() {
                let variant = xml(variant);
                let _ = write!(fonts, "<Font Self=\"lp_font_{i}_{j}\" FontFamily=\"{family}\" Name=\"{family} {variant}\" FontStyleName=\"{variant}\"/>");
            }
            fonts.push_str("</FontFamily>");
        }
        archive.add("Resources/Fonts.xml", &package("Fonts", &fonts))?;
        let mut colors = String::from("<Swatch Self=\"Swatch/None\" Name=\"$ID/None\"/><Color Self=\"Color/Black\" Name=\"Black\" Model=\"Process\" Space=\"CMYK\" ColorValue=\"0 0 0 100\"/>");
        for [r, g, b] in self.colors {
            let _ = write!(colors, "<Color Self=\"Color/LP{r:02x}{g:02x}{b:02x}\" Name=\"LP{r:02x}{g:02x}{b:02x}\" Model=\"Process\" Space=\"RGB\" ColorValue=\"{r} {g} {b}\"/>");
        }
        archive.add("Resources/Graphic.xml", &package("Graphic", &colors))?;
        archive.add("Resources/Styles.xml", &package("Styles", &format!("<RootCharacterStyleGroup Self=\"lp_csg\"><CharacterStyle Self=\"{CHARACTER}\" Name=\"$ID/[No character style]\"/></RootCharacterStyleGroup><RootParagraphStyleGroup Self=\"lp_psg\"><ParagraphStyle Self=\"{PARAGRAPH}\" Name=\"$ID/NormalParagraphStyle\"/></RootParagraphStyleGroup>")))?;
        Ok(self.report)
    }
}
fn preflight(state: &DocumentState) -> Result<(), ExportError> {
    if state.paint_source.is_some()
        || !state.animation.tracks.is_empty()
        || state.color_mode != lumapaint_core::document::ColorMode::Rgb
        || !state.strokes.is_empty()
        || state.paper.enabled
        || state.clipping_path_id.is_some()
        || !state.layer_groups.groups.is_empty()
        || state.layer_effects.values().any(|e| e.active())
        || state.pixel_aspect_ratio.is_some_and(|v| v != 1.)
    {
        return Err(unsupported());
    }
    for layer in &state.svg_layers {
        if !layer.vector_layer || layer.effective_opacity() != 1. {
            return Err(unsupported());
        }
        for o in &layer.vector_objects {
            let t = o.text.as_ref();
            let mut stroke = o.stroke_style.clone();
            stroke.cap = Default::default();
            stroke.join = Default::default();
            stroke.miter_limit = 4.;
            if o.clipping_group.is_some()
                || !o.group_path.is_empty()
                || o.fill_gradient.is_some()
                || o.stroke_gradient.is_some()
                || (t.is_some() && o.stroke.is_some())
                || o.blend_mode != "normal"
                || [o.fill, o.stroke]
                    .into_iter()
                    .flatten()
                    .any(|p| p.color[3] != 255 || p.registration)
                || (t.is_none() && stroke != Default::default())
                || o.path.fill_rule != lumapaint_core::vector::FillRule::NonZero
                || o.live_corners.is_some()
                || o.rectangle_radii
                    .is_some_and(|r| r.iter().any(|v| *v != 0.))
                || t.is_some_and(|t| {
                    !t.font_axes.is_empty()
                        || t.runs.iter().any(|r| !r.style.font_axes.is_empty())
                        || t.list_style != lumapaint_core::vector::ParagraphListStyle::None
                })
            {
                return Err(unsupported());
            }
        }
    }
    Ok(())
}
fn frame_xml(frame: &Frame<'_>, scale: f32, story: &str, previous: &str, next: &str) -> String {
    let text = frame.text;
    let w = text.box_width * scale;
    let h = text.box_height.unwrap_or_else(|| {
        (text.visual_lines().len() as f32 * text.font_size * text.line_height + text.font_size)
            .clamp(16., 8192.)
    }) * scale;
    let angle = text.rotation.to_radians();
    let rotated = [
        angle.cos() * text.scale_x,
        angle.sin() * text.scale_x,
        -angle.sin() * text.scale_y,
        angle.cos() * text.scale_y,
        0.,
        0.,
    ];
    let [a, b, c, d, e, f] = lumapaint_core::image_frame::multiply(frame.object.transform, rotated);
    let (e, f) = (e * scale, f * scale);
    let mut geometry = String::new();
    for (x, y) in [(0., 0.), (w, 0.), (w, h), (0., h)] {
        let _=write!(geometry,"<PathPointType Anchor=\"{x} {y}\" LeftDirection=\"{x} {y}\" RightDirection=\"{x} {y}\"/>");
    }
    format!("<TextFrame Self=\"{}\" Name=\"{}\" ParentStory=\"{story}\" PreviousTextFrame=\"{previous}\" NextTextFrame=\"{next}\" ContentType=\"TextType\" ItemLayer=\"{}\" Visible=\"{}\" Locked=\"{}\" ItemTransform=\"{a} {b} {c} {d} {e} {f}\" FillColor=\"Swatch/None\" StrokeColor=\"Swatch/None\" StrokeWeight=\"0\"><Properties><PathGeometry><GeometryPathType PathOpen=\"false\"><PathPointArray>{geometry}</PathPointArray></GeometryPathType></PathGeometry></Properties><TextFramePreference TextColumnCount=\"1\" TextColumnFixedWidth=\"{w}\" FirstBaselineOffset=\"FixedHeight\" MinimumFirstBaselineOffset=\"{}\" VerticalJustification=\"TopAlign\"><Properties><InsetSpacing type=\"list\"><ListItem type=\"unit\">0</ListItem><ListItem type=\"unit\">0</ListItem><ListItem type=\"unit\">0</ListItem><ListItem type=\"unit\">0</ListItem></InsetSpacing></Properties></TextFramePreference><TransparencySetting><BlendingSetting Opacity=\"{}\" BlendMode=\"Normal\"/></TransparencySetting></TextFrame>",frame.id,xml(&frame.object.name),frame.layer,frame.object.visible,frame.locked,text.font_size*scale,frame.object.opacity*100.)
}

pub struct IdmlExporter;
impl DocumentExporter for IdmlExporter {
    fn format(&self) -> FormatId {
        FormatId::Idml
    }
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        options: ExportOptions,
    ) -> Result<ExportedDocument, ExportError> {
        let document = Document::from_document_state(snapshot.state().clone())
            .map_err(ExportError::InvalidDocument)?;
        let book = document.pages_snapshot();
        let mut archive = Archive::new()?;
        let mut resources = Resources::new();
        issue(
            &mut resources.report,
            "idml.text_recomposed",
            CompatibilityTier::B,
        );
        if book.facing {
            issue(
                &mut resources.report,
                "idml.facing_pages_separated",
                CompatibilityTier::B,
            );
        }
        let states: Vec<_> = (0..book.pages.len())
            .map(|i| {
                document
                    .detached_page(i)
                    .map(|d| d.document_state())
                    .ok_or_else(limit)
            })
            .collect::<Result<_, _>>()?;
        let mut layers = String::new();
        let mut story_references = String::new();
        let mut spread_references = String::new();
        let mut stories = Vec::new();
        let mut all_frames = Vec::new();
        let mut frame_indices = BTreeMap::new();
        let mut layer_ids = Vec::new();
        let mut object_count = 0;
        let mut layer_count = 0;
        for (pi, state) in states.iter().enumerate() {
            preflight(state)?;
            let mut ids = Vec::new();
            for layer in &state.svg_layers {
                layer_count += 1;
                if layer_count > 8192 {
                    return Err(limit());
                }
                let layer_id = format!("u{:x}", 0x10000000 + layer_count);
                ids.push(layer_id.clone());
                let _=write!(layers,"<Layer Self=\"{layer_id}\" Name=\"{}\" Visible=\"{}\" Locked=\"{}\" Printable=\"true\"/>",xml(&layer.name),layer.visible,layer.locked);
                for o in &layer.vector_objects {
                    object_count += 1;
                    if object_count > MAX_FRAMES {
                        return Err(limit());
                    }
                    if let Some(text) = &o.text {
                        frame_indices.insert((pi, o.id.as_str()), all_frames.len());
                        all_frames.push(Frame {
                            id: format!("u{:x}", 0x20000000 + object_count),
                            layer: layer_id.clone(),
                            object: o,
                            text,
                            color: o
                                .fill
                                .map_or([0; 3], |p| [p.color[0], p.color[1], p.color[2]]),
                            locked: state.locked_objects.contains(&o.id)
                                || state.locked_artwork_layers.contains(&layer.id),
                        });
                    }
                }
            }
            layer_ids.push(ids);
        }
        let mut groups = Vec::new();
        let mut linked = BTreeSet::new();
        if let Some(publication) = &snapshot.state().pages {
            for thread in &publication.text_threads {
                let indices = thread
                    .frames
                    .iter()
                    .map(|f| {
                        let pi = book
                            .pages
                            .iter()
                            .position(|p| p.id == f.page_id)
                            .ok_or_else(unsupported)?;
                        frame_indices
                            .get(&(pi, f.object_id.as_str()))
                            .copied()
                            .ok_or_else(unsupported)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                linked.extend(indices.iter().copied());
                groups.push(indices);
            }
        }
        for (pi, state) in states.iter().enumerate() {
            for thread in &state.vector_assets.threads {
                let indices = thread
                    .frames
                    .iter()
                    .map(|id| {
                        frame_indices
                            .get(&(pi, id.as_str()))
                            .copied()
                            .ok_or_else(unsupported)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                linked.extend(indices.iter().copied());
                groups.push(indices);
            }
        }
        groups.extend(
            (0..all_frames.len())
                .filter(|i| !linked.contains(i))
                .map(|i| vec![i]),
        );
        let mut links = BTreeMap::new();
        for group in groups {
            let story_id = format!("u{:x}", 0x30000000 + stories.len() + 1);
            let first_page = frame_indices
                .iter()
                .find(|(_, i)| **i == group[0])
                .map(|((pi, _), _)| *pi)
                .ok_or_else(unsupported)?;
            let scale = 72. / states[first_page].resolution.unwrap_or(72) as f32;
            let frames: Vec<_> = group.iter().map(|i| &all_frames[*i]).collect();
            let body = story::write(&story_id, &frames, scale, &mut resources)?;
            let path = format!("Stories/lp_story_{}.xml", stories.len() + 1);
            archive.add(&path, &package("Story", &body))?;
            let _ = write!(story_references, "<idPkg:Story src=\"{path}\"/>");
            stories.push(story_id.clone());
            for (at, index) in group.iter().enumerate() {
                links.insert(
                    *index,
                    (
                        story_id.clone(),
                        at.checked_sub(1)
                            .map_or("n", |i| all_frames[group[i]].id.as_str())
                            .to_owned(),
                        group
                            .get(at + 1)
                            .map_or("n", |i| all_frames[*i].id.as_str())
                            .to_owned(),
                    ),
                );
            }
        }
        let mut artwork_id = 0;
        for (pi, state) in states.iter().enumerate() {
            let scale = 72. / state.resolution.unwrap_or(72) as f32;
            let (w, h) = (state.width as f32 * scale, state.height as f32 * scale);
            if w > 8640. || h > 8640. {
                return Err(limit());
            }
            let mut spread=format!("<Spread Self=\"u{:x}\" PageCount=\"1\" BindingLocation=\"1\" AllowPageShuffle=\"false\"><Page Self=\"u{:x}\" Name=\"{}\" GeometricBounds=\"0 0 {h} {w}\" ItemTransform=\"1 0 0 1 0 0\" AppliedMaster=\"n\"><MarginPreference Top=\"0\" Bottom=\"0\" Left=\"0\" Right=\"0\" ColumnCount=\"1\" ColumnGutter=\"0\"/></Page>",0x40000000+pi,0x50000000+pi,pi+1);
            for (li, layer) in state.svg_layers.iter().enumerate() {
                for o in &layer.vector_objects {
                    if let Some(index) = frame_indices.get(&(pi, o.id.as_str())) {
                        let (story, previous, next) = &links[index];
                        spread.push_str(&frame_xml(
                            &all_frames[*index],
                            scale,
                            story,
                            previous,
                            next,
                        ));
                    } else {
                        spread.push_str(&artwork::write(
                            o,
                            &layer_ids[pi][li],
                            artwork_id,
                            scale,
                            &mut resources,
                            state.locked_objects.contains(&o.id)
                                || state.locked_artwork_layers.contains(&layer.id),
                        )?);
                        artwork_id += 1;
                    }
                }
            }
            spread.push_str("</Spread>");
            let path = format!("Spreads/lp_spread_{pi}.xml");
            archive.add(&path, &package("Spread", &spread))?;
            let _ = write!(spread_references, "<idPkg:Spread src=\"{path}\"/>");
        }
        if layers.is_empty() {
            layers.push_str(
                "<Layer Self=\"u10000001\" Name=\"Layer 1\" Visible=\"true\" Locked=\"false\"/>",
            );
        }
        let first = &book.pages[0];
        let scale = 72.
            / document
                .detached_page(0)
                .ok_or_else(limit)?
                .document_state()
                .resolution
                .unwrap_or(72) as f32;
        // Zero avoids an extra default page; explicit Spread/Page elements own pagination.
        archive.add("Resources/Preferences.xml", &package("Preferences", &format!("<DocumentPreference PageWidth=\"{}\" PageHeight=\"{}\" PagesPerDocument=\"0\" FacingPages=\"false\" PageBinding=\"{}\"/> <TextFramePreference TextColumnCount=\"1\"/>",first.width as f32*scale,first.height as f32*scale,if book.binding==PageBinding::RightToLeft {"RightToLeft"} else {"LeftToRight"})))?;
        let report = resources.write(&mut archive)?;
        let design = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><?aid style=\"50\" type=\"document\" readerVersion=\"6.0\" featureSet=\"257\" product=\"7.0(330)\"?><Document xmlns:idPkg=\"{NS}\" DOMVersion=\"7.0\" Self=\"d\" Name=\"LumaPaint\" StoryList=\"{}\" ZeroPoint=\"0 0\"><idPkg:Graphic src=\"Resources/Graphic.xml\"/><idPkg:Fonts src=\"Resources/Fonts.xml\"/><idPkg:Styles src=\"Resources/Styles.xml\"/><idPkg:Preferences src=\"Resources/Preferences.xml\"/>{layers}{spread_references}{story_references}</Document>",stories.join(" "));
        archive.add("designmap.xml", &design)?;
        if !options.allow_lossy && !report.issues.is_empty() {
            return Err(ExportError::LossyConversionRequiresConsent(report));
        }
        Ok(ExportedDocument {
            format: FormatId::Idml,
            media_type: MIME,
            bytes: archive.finish()?,
            report,
        })
    }
}
#[cfg(test)]
mod tests;
