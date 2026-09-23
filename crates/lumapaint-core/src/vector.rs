//! Portable path interchange. Engine-owned objects never enter saved documents.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// SVG path data (the `d` attribute), in document coordinates, without paint or transforms.
/// This is the engine and editable-document interchange boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorPath {
    pub data: String,
    pub fill_rule: FillRule,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorPaint {
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VectorObjectKind {
    #[default]
    Path,
    Compound,
    Rectangle,
    Ellipse,
    Text,
}

/// Character formatting, independent of paragraph and text-frame layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub tracking: f32,
    pub baseline_shift: f32,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: [u8; 3],
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextStylePatch {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub tracking: Option<f32>,
    pub baseline_shift: Option<f32>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub color: Option<[u8; 3]>,
}
impl TextStyle {
    pub fn apply(&mut self, patch: &TextStylePatch) {
        if let Some(value) = &patch.font_family {
            self.font_family = value.clone();
        }
        if let Some(value) = patch.font_size {
            self.font_size = value;
        }
        if let Some(value) = patch.bold {
            self.bold = value;
        }
        if let Some(value) = patch.italic {
            self.italic = value;
        }
        if let Some(value) = patch.tracking {
            self.tracking = value;
        }
        if let Some(value) = patch.baseline_shift {
            self.baseline_shift = value;
        }
        if let Some(value) = patch.underline {
            self.underline = value;
        }
        if let Some(value) = patch.strikethrough {
            self.strikethrough = value;
        }
        if let Some(value) = patch.color {
            self.color = value;
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.font_family.trim().is_empty()
            || self.font_family.chars().count() > 200
            || self.font_family.chars().any(char::is_control)
            || !(1.0..=512.0).contains(&self.font_size)
            || !(-100.0..=1000.0).contains(&self.tracking)
            || !(-512.0..=512.0).contains(&self.baseline_shift)
        {
            return Err("Invalid character style".into());
        }
        Ok(())
    }
}
/// Half-open UTF-16 offsets, matching native text selection and JavaScript strings.
/// Boundaries must not split surrogate pairs. Gaps inherit the object's base style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextGlyphCluster {
    /// UTF-16 offsets relative to the visual line.
    pub start: usize,
    pub end: usize,
    pub x: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(default)]
pub struct VectorText {
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextRun>,
    /// UTF-16 offsets at the start of visual lines inserted by the native text layout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub soft_breaks: Vec<usize>,
    /// Native layout baselines for visual lines in unscaled text-container coordinates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_baselines: Vec<f32>,
    /// Native layout width for each visual line, before text-object transforms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_widths: Vec<f32>,
    /// Native layout pen position at the start of each visual line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_origins: Vec<f32>,
    /// Native pen positions for the style segments on each visual line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_segment_origins: Vec<Vec<f32>>,
    /// Native pen positions for each Unicode scalar on every visual line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub character_origins: Vec<Vec<f32>>,
    /// Native shaping clusters for ligatures and combining sequences on each line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glyph_clusters: Vec<Vec<TextGlyphCluster>>,
    /// Glyph enclosure in unscaled, unrotated text-container coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_bounds: Option<[f32; 4]>,
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub bold: bool,
    pub italic: bool,
    pub tracking: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub baseline_shift: f32,
    pub rotation: f32,
    pub underline: bool,
    pub strikethrough: bool,
    pub alignment: TextAlignment,
    pub box_width: f32,
    pub indent_left: f32,
    pub indent_right: f32,
    pub indent_first: f32,
    pub space_before: f32,
    pub space_after: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

impl Default for VectorText {
    fn default() -> Self {
        Self {
            content: String::new(),
            runs: Vec::new(),
            soft_breaks: Vec::new(),
            line_baselines: Vec::new(),
            line_widths: Vec::new(),
            line_origins: Vec::new(),
            style_segment_origins: Vec::new(),
            character_origins: Vec::new(),
            glyph_clusters: Vec::new(),
            layout_bounds: None,
            font_family: "sans-serif".into(),
            font_size: 48.0,
            line_height: 1.4,
            bold: false,
            italic: false,
            tracking: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            baseline_shift: 0.0,
            rotation: 0.0,
            underline: false,
            strikethrough: false,
            alignment: TextAlignment::Left,
            box_width: 480.0,
            indent_left: 0.0,
            indent_right: 0.0,
            indent_first: 0.0,
            space_before: 0.0,
            space_after: 0.0,
        }
    }
}

fn utf16_boundary_in(value: &str, offset: usize) -> bool {
    let mut at = 0;
    for character in value.chars() {
        if at == offset {
            return true;
        }
        at += character.len_utf16();
    }
    at == offset
}

impl VectorText {
    /// Discard measurements tied to a particular font/layout engine.
    /// A host should call this before measuring the current content again.
    pub fn clear_measured_layout(&mut self) {
        self.soft_breaks.clear();
        self.line_baselines.clear();
        self.line_widths.clear();
        self.line_origins.clear();
        self.style_segment_origins.clear();
        self.character_origins.clear();
        self.glyph_clusters.clear();
        self.layout_bounds = None;
    }

    /// Reflow without a platform text view. The caller supplies a width measured
    /// with its font engine for a UTF-16-offset substring of the source text.
    /// Breaks always land on extended grapheme boundaries; source text and runs
    /// are never rewritten. Widths should grow as text is appended, allowing a
    /// logarithmic search for each line. Native hosts can replace the result.
    pub fn reflow_soft_breaks_with(
        &mut self,
        mut measure: impl FnMut(&str, usize) -> Result<f32, String>,
    ) -> Result<(), String> {
        use unicode_segmentation::UnicodeSegmentation;

        let mut candidate = self.clone();
        candidate.clear_measured_layout();
        candidate.validate()?;
        let mut breaks = Vec::new();
        let mut paragraph_utf16 = 0;
        for paragraph in self.content.split('\n') {
            let mut boundaries = vec![(0, 0)];
            let mut utf16 = 0;
            for (byte, grapheme) in paragraph.grapheme_indices(true) {
                utf16 += grapheme.encode_utf16().count();
                boundaries.push((byte + grapheme.len(), utf16));
            }
            let preferred: std::collections::HashSet<_> = paragraph
                .split_word_bound_indices()
                .map(|(byte, word)| byte + word.len())
                .collect();
            let mut line_start = 0;
            let last = boundaries.len() - 1;
            while line_start < last {
                let mut fitting = line_start + 1;
                let first_line = line_start == 0;
                let available = self.box_width
                    - self.indent_left
                    - self.indent_right
                    - if first_line { self.indent_first } else { 0.0 };
                if !available.is_finite() || available <= 0.0 {
                    return Err("Invalid text line width".into());
                }
                let first_source = &paragraph[boundaries[line_start].0..boundaries[fitting].0];
                let first_width =
                    measure(first_source, paragraph_utf16 + boundaries[line_start].1)?;
                if !first_width.is_finite() || !(0.0..100_000.0).contains(&first_width) {
                    return Err("Invalid measured text width".into());
                }
                let mut upper = last + 1;
                while fitting < upper {
                    let end = fitting + (upper - fitting) / 2;
                    if end == fitting {
                        break;
                    }
                    let source = &paragraph[boundaries[line_start].0..boundaries[end].0];
                    let width = measure(source, paragraph_utf16 + boundaries[line_start].1)?;
                    if !width.is_finite() || !(0.0..100_000.0).contains(&width) {
                        return Err("Invalid measured text width".into());
                    }
                    if width <= available {
                        fitting = end;
                    } else {
                        upper = end;
                    }
                }
                if fitting == last {
                    break;
                }
                let next = (line_start + 1..=fitting)
                    .rev()
                    .find(|index| preferred.contains(&boundaries[*index].0))
                    .unwrap_or(fitting);
                breaks.push(paragraph_utf16 + boundaries[next].1);
                line_start = next;
            }
            paragraph_utf16 += utf16 + 1;
        }
        candidate.soft_breaks = breaks;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Retain source text and character styles while splitting at hard and soft line breaks.
    /// The bool distinguishes a paragraph break from a wrapped line.
    pub fn visual_lines(&self) -> Vec<(usize, &str, bool)> {
        let mut lines = Vec::new();
        let mut start_byte = 0;
        let mut start_utf16 = 0;
        let mut offset = 0;
        let mut hard_break_before = false;
        for (byte, c) in self.content.char_indices() {
            if c == '\n' {
                lines.push((
                    start_utf16,
                    &self.content[start_byte..byte],
                    hard_break_before,
                ));
                offset += 1;
                start_byte = byte + 1;
                start_utf16 = offset;
                hard_break_before = true;
                continue;
            }
            if self.soft_breaks.binary_search(&offset).is_ok() && byte > start_byte {
                lines.push((
                    start_utf16,
                    &self.content[start_byte..byte],
                    hard_break_before,
                ));
                start_byte = byte;
                start_utf16 = offset;
                hard_break_before = false;
            }
            offset += c.len_utf16();
        }
        lines.push((start_utf16, &self.content[start_byte..], hard_break_before));
        lines
    }
    pub fn base_style(&self, color: [u8; 3]) -> TextStyle {
        TextStyle {
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            tracking: self.tracking,
            baseline_shift: self.baseline_shift,
            underline: self.underline,
            strikethrough: self.strikethrough,
            color,
        }
    }
    pub fn style_segment_starts(&self, start: usize, line: &str, color: [u8; 3]) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut previous = None;
        let mut offset = start;
        for c in line.chars() {
            let style = self.style_at(offset, color);
            if previous.as_ref() != Some(&style) {
                starts.push(offset);
                previous = Some(style);
            }
            offset += c.len_utf16();
        }
        starts
    }
    pub fn style_at(&self, offset: usize, color: [u8; 3]) -> TextStyle {
        self.runs
            .iter()
            .find(|run| run.start <= offset && offset < run.end)
            .map(|run| run.style.clone())
            .unwrap_or_else(|| self.base_style(color))
    }
    pub fn utf16_boundary(&self, offset: usize) -> bool {
        let mut at = 0;
        for c in self.content.chars() {
            if at == offset {
                return true;
            }
            at += c.len_utf16();
        }
        at == offset
    }
    /// Patch only specified properties, preserving other formatting in the selected range.
    pub fn apply_style(
        &mut self,
        start: usize,
        end: usize,
        patch: &TextStylePatch,
        color: [u8; 3],
    ) -> Result<(), String> {
        if start > end || !self.utf16_boundary(start) || !self.utf16_boundary(end) {
            return Err("Invalid character range".into());
        }
        let mut runs: Vec<TextRun> = Vec::new();
        let mut at = 0;
        for c in self.content.chars() {
            let mut style = self.style_at(at, color);
            if at >= start && at < end {
                style.apply(patch);
            }
            style.validate()?;
            let next = at + c.len_utf16();
            if let Some(last) = runs.last_mut().filter(|run| run.style == style) {
                last.end = next;
            } else {
                runs.push(TextRun {
                    start: at,
                    end: next,
                    style,
                });
            }
            at = next;
        }
        let base = self.base_style(color);
        runs.retain(|run| run.style != base);
        let changed = self.runs != runs;
        self.runs = runs;
        if changed
            && (patch.font_family.is_some()
                || patch.font_size.is_some()
                || patch.bold.is_some()
                || patch.italic.is_some()
                || patch.tracking.is_some()
                || patch.baseline_shift.is_some()
                || patch.underline.is_some()
                || patch.strikethrough.is_some())
        {
            self.clear_measured_layout();
        } else if changed {
            // Color can split or merge style segments without changing glyph geometry.
            self.style_segment_origins.clear();
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.content.trim().is_empty()
            || self.content.chars().count() > 4096
            || self.content.split('\n').count() > 64
            || self
                .content
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            || self.font_family.trim().is_empty()
            || self.font_family.chars().count() > 200
            || self.font_family.chars().any(char::is_control)
            || !(-100.0..=1000.0).contains(&self.tracking)
            || !(0.1..=4.0).contains(&self.scale_x)
            || !(0.1..=4.0).contains(&self.scale_y)
            || !(-512.0..=512.0).contains(&self.baseline_shift)
            || !(-180.0..=180.0).contains(&self.rotation)
            || !(16.0..=8192.0).contains(&self.box_width)
            || !(0.0..=4096.0).contains(&self.indent_left)
            || !(0.0..=4096.0).contains(&self.indent_right)
            || !(-4096.0..=4096.0).contains(&self.indent_first)
            || self.indent_left + self.indent_right >= self.box_width
            || !(0.0..=512.0).contains(&self.space_before)
            || !(0.0..=512.0).contains(&self.space_after)
            || !self.font_size.is_finite()
            || !(1.0..=512.0).contains(&self.font_size)
            || !self.line_height.is_finite()
            || !(0.1..=4096.0).contains(&(self.font_size * self.line_height))
        {
            return Err("Invalid text settings".into());
        }
        if self.runs.len() > 4096 {
            return Err("Too many character runs".into());
        }
        let utf16: Vec<_> = self.content.encode_utf16().collect();
        if self.soft_breaks.len() > 4096
            || self.soft_breaks.windows(2).any(|pair| pair[0] >= pair[1])
            || self.soft_breaks.iter().any(|&offset| {
                offset == 0
                    || offset >= utf16.len()
                    || !self.utf16_boundary(offset)
                    || utf16[offset - 1] == b'\n' as u16
                    || utf16[offset] == b'\n' as u16
            })
        {
            return Err("Invalid text wrap positions".into());
        }
        if !self.line_baselines.is_empty()
            && (self.line_baselines.len() != self.visual_lines().len()
                || self
                    .line_baselines
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
                || self
                    .line_baselines
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1]))
        {
            return Err("Invalid text line baselines".into());
        }
        if !self.line_widths.is_empty()
            && (self.line_widths.len() != self.visual_lines().len()
                || self
                    .line_widths
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..100_000.0).contains(value)))
        {
            return Err("Invalid text line widths".into());
        }
        if !self.line_origins.is_empty()
            && (self.line_origins.len() != self.visual_lines().len()
                || self
                    .line_origins
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() >= 100_000.0))
        {
            return Err("Invalid text line origins".into());
        }
        if !self.style_segment_origins.is_empty()
            && (self.style_segment_origins.len() != self.visual_lines().len()
                || self
                    .style_segment_origins
                    .iter()
                    .zip(self.visual_lines())
                    .any(|(origins, (_, line, _))| {
                        origins.len() > line.chars().count()
                            || (origins.is_empty() != line.is_empty())
                            || origins
                                .iter()
                                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
                    }))
        {
            return Err("Invalid text style segment origins".into());
        }
        if !self.character_origins.is_empty()
            && (self.character_origins.len() != self.visual_lines().len()
                || self.character_origins.iter().zip(self.visual_lines()).any(
                    |(origins, (_, line, _))| {
                        origins.len() != line.chars().count()
                            || origins
                                .iter()
                                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
                            || origins.windows(2).any(|pair| pair[0] > pair[1])
                    },
                ))
        {
            return Err("Invalid text character origins".into());
        }
        if !self.glyph_clusters.is_empty()
            && (self.glyph_clusters.len() != self.visual_lines().len()
                || self.glyph_clusters.iter().zip(self.visual_lines()).any(
                    |(clusters, (_, line, _))| {
                        let line_len = line.encode_utf16().count();
                        let mut end = 0;
                        for cluster in clusters {
                            if cluster.start != end
                                || cluster.end <= cluster.start
                                || cluster.end > line_len
                                || !cluster.x.is_finite()
                                || cluster.x.abs() >= 100_000.0
                                || !utf16_boundary_in(line, cluster.start)
                                || !utf16_boundary_in(line, cluster.end)
                            {
                                return true;
                            }
                            end = cluster.end;
                        }
                        end != line_len || (line_len > 0 && clusters.is_empty())
                    },
                ))
        {
            return Err("Invalid text glyph clusters".into());
        }
        if self.layout_bounds.is_some_and(|[x, y, width, height]| {
            [x, y, width, height]
                .iter()
                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
                || width <= 0.0
                || height <= 0.0
                || (x + width).abs() >= 100_000.0
                || (y + height).abs() >= 100_000.0
        }) {
            return Err("Invalid text layout bounds".into());
        }
        let mut previous_end = 0;
        for run in &self.runs {
            if run.start < previous_end
                || run.start >= run.end
                || !self.utf16_boundary(run.start)
                || !self.utf16_boundary(run.end)
            {
                return Err("Invalid character runs".into());
            }
            run.style.validate()?;
            previous_end = run.end;
        }
        Ok(())
    }

    pub fn control_points(&self) -> Vec<[f32; 2]> {
        // Older documents fall back to a conservative text-frame rectangle.
        let max_size = self
            .runs
            .iter()
            .map(|run| run.style.font_size)
            .fold(self.font_size, f32::max);
        let height = self.visual_lines().len() as f32
            * (self.font_size * self.line_height + self.space_before + self.space_after);
        let highest_shift = self
            .runs
            .iter()
            .map(|run| run.style.baseline_shift)
            .fold(self.baseline_shift, f32::max);
        let lowest_shift = self
            .runs
            .iter()
            .map(|run| run.style.baseline_shift)
            .fold(self.baseline_shift, f32::min);
        let top = -highest_shift - (max_size - self.font_size).max(0.0);
        let bottom = height - lowest_shift + (max_size - self.font_size).max(0.0);
        let angle = self.rotation.to_radians();
        let [left, top, right, bottom] = self.layout_bounds.map_or(
            [0.0, top, self.box_width, bottom],
            |[x, y, width, height]| [x, y, x + width, y + height],
        );
        [[left, top], [right, top], [right, bottom], [left, bottom]]
            .into_iter()
            .map(|[x, y]| {
                let x = x * self.scale_x;
                let y = y * self.scale_y;
                [
                    x * angle.cos() - y * angle.sin(),
                    x * angle.sin() + y * angle.cos(),
                ]
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorObject {
    pub id: String,
    pub name: String,
    pub path: VectorPath,
    /// SVG-compatible affine matrix: [a, b, c, d, e, f].
    pub transform: [f32; 6],
    pub fill: Option<VectorPaint>,
    pub stroke: Option<VectorPaint>,
    pub stroke_width: f32,
    pub visible: bool,
    #[serde(default)]
    pub kind: VectorObjectKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<VectorText>,
}

impl VectorObject {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 64
            || self.name.trim().is_empty()
            || self.name.chars().count() > 120
            || self.path.data.is_empty()
            || self.path.data.len() > 1024 * 1024
            || self.transform.iter().any(|value| !value.is_finite())
            || !self.stroke_width.is_finite()
            || !(0.0..=4096.0).contains(&self.stroke_width)
            || (self.fill.is_none() && self.stroke.is_none())
            || self.control_points.len() > 65_536
            || self
                .control_points
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
        {
            return Err("Invalid vector object".into());
        }
        match (&self.kind, &self.text) {
            (VectorObjectKind::Text, Some(text)) => text.validate()?,
            (VectorObjectKind::Text, None) | (_, Some(_)) => {
                return Err("Invalid text object".into())
            }
            _ => {}
        }
        Ok(())
    }

    pub fn hit_test(&self, point: [f32; 2], tolerance: f32) -> bool {
        if !self.visible
            || !tolerance.is_finite()
            || tolerance < 0.0
            || self.control_points.is_empty()
        {
            return false;
        }
        let points = self
            .control_points
            .iter()
            .map(|[x, y]| {
                let [a, b, c, d, e, f] = self.transform;
                [a * x + c * y + e, b * x + d * y + f]
            })
            .collect::<Vec<_>>();
        let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        match self.kind {
            VectorObjectKind::Text => point_in_convex_quad(&points, point, tolerance),
            VectorObjectKind::Rectangle | VectorObjectKind::Compound => {
                point[0] >= min_x - tolerance
                    && point[0] <= max_x + tolerance
                    && point[1] >= min_y - tolerance
                    && point[1] <= max_y + tolerance
            }
            VectorObjectKind::Ellipse => {
                let rx = (max_x - min_x) * 0.5 + tolerance;
                let ry = (max_y - min_y) * 0.5 + tolerance;
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                rx > 0.0
                    && ry > 0.0
                    && ((point[0] - cx) / rx).powi(2) + ((point[1] - cy) / ry).powi(2) <= 1.0
            }
            VectorObjectKind::Path => points.windows(2).any(|segment| {
                distance_to_segment(point, segment[0], segment[1])
                    <= tolerance.max(self.stroke_width * 0.5)
            }),
        }
    }
}

fn point_in_convex_quad(points: &[[f32; 2]], point: [f32; 2], tolerance: f32) -> bool {
    if points.len() != 4 {
        return false;
    }
    let mut positive = false;
    let mut negative = false;
    for index in 0..4 {
        let a = points[index];
        let b = points[(index + 1) % 4];
        let edge = [b[0] - a[0], b[1] - a[1]];
        let length = edge[0].hypot(edge[1]);
        if length <= f32::EPSILON {
            return false;
        }
        let distance = (edge[0] * (point[1] - a[1]) - edge[1] * (point[0] - a[0])) / length;
        positive |= distance > tolerance;
        negative |= distance < -tolerance;
        if positive && negative {
            return false;
        }
    }
    true
}

fn distance_to_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    if length_squared <= f32::EPSILON {
        return (point[0] - start[0]).hypot(point[1] - start[1]);
    }
    let t = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1])
        / length_squared)
        .clamp(0.0, 1.0);
    (point[0] - start[0] - t * delta[0]).hypot(point[1] - start[1] - t * delta[1])
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathOperation {
    Union,
    Difference,
    Intersection,
    Xor,
}

/// Boolean operations act on filled areas. Difference means left minus right.
/// Backends must reject malformed or non-finite geometry, never change either input.
pub trait VectorPathEngine {
    fn combine(
        &self,
        left: &VectorPath,
        right: &VectorPath,
        operation: PathOperation,
    ) -> Result<VectorPath, String>;
    fn contains(&self, path: &VectorPath, point: [f32; 2]) -> Result<bool, String>;
}

#[cfg(test)]
mod text_style_tests {
    use super::*;
    #[test]
    fn portable_reflow_preserves_graphemes_runs_and_hard_breaks() {
        let mut text = VectorText {
            content: "A\n😀😀😀".into(),
            box_width: 21.0,
            ..Default::default()
        };
        text.apply_style(
            2,
            4,
            &TextStylePatch {
                color: Some([200, 0, 0]),
                ..Default::default()
            },
            [0, 0, 0],
        )
        .unwrap();
        let runs = text.runs.clone();
        text.reflow_soft_breaks_with(|source, _| {
            use unicode_segmentation::UnicodeSegmentation;
            Ok((source.graphemes(true).count() * 10) as f32)
        })
        .unwrap();
        assert_eq!(text.soft_breaks, vec![6]);
        assert_eq!(
            text.visual_lines(),
            vec![(0, "A", false), (2, "😀😀", true), (6, "😀", false)]
        );
        assert_eq!(text.runs, runs);
        text.validate().unwrap();
    }

    #[test]
    fn portable_reflow_obeys_first_line_indent_and_is_atomic_on_error() {
        let mut text = VectorText {
            content: "abcdef".into(),
            box_width: 50.0,
            indent_first: 20.0,
            ..Default::default()
        };
        text.reflow_soft_breaks_with(|source, _| Ok(source.len() as f32 * 10.0))
            .unwrap();
        assert_eq!(text.soft_breaks, vec![3]);
        let before = text.clone();
        assert!(text.reflow_soft_breaks_with(|_, _| Ok(f32::NAN)).is_err());
        assert_eq!(text, before);
    }

    #[test]
    fn portable_reflow_avoids_measuring_every_prefix_of_a_long_line() {
        let mut text = VectorText {
            content: "a".repeat(4096),
            box_width: 8192.0,
            ..Default::default()
        };
        let calls = std::cell::Cell::new(0);
        text.reflow_soft_breaks_with(|source, _| {
            calls.set(calls.get() + 1);
            Ok(source.len() as f32)
        })
        .unwrap();
        assert!(text.soft_breaks.is_empty());
        assert!(calls.get() < 32, "measured {} candidates", calls.get());
    }
    #[test]
    fn style_changes_invalidate_only_the_layout_they_affect() {
        let mut text = VectorText {
            content: "ABCD".into(),
            soft_breaks: vec![2],
            line_baselines: vec![48.0, 110.0],
            line_widths: vec![80.0, 82.0],
            line_origins: vec![0.0, 0.0],
            style_segment_origins: vec![vec![0.0], vec![0.0]],
            character_origins: vec![vec![0.0, 40.0], vec![0.0, 41.0]],
            glyph_clusters: vec![
                vec![
                    TextGlyphCluster {
                        start: 0,
                        end: 1,
                        x: 0.0,
                    },
                    TextGlyphCluster {
                        start: 1,
                        end: 2,
                        x: 40.0,
                    },
                ],
                vec![
                    TextGlyphCluster {
                        start: 0,
                        end: 1,
                        x: 0.0,
                    },
                    TextGlyphCluster {
                        start: 1,
                        end: 2,
                        x: 41.0,
                    },
                ],
            ],
            layout_bounds: Some([0.0, 0.0, 100.0, 120.0]),
            ..Default::default()
        };
        let color = [0, 0, 0];
        text.validate().unwrap();
        let original = text.clone();
        text.apply_style(0, 2, &TextStylePatch::default(), color)
            .unwrap();
        assert_eq!(text, original);
        text.apply_style(
            0,
            2,
            &TextStylePatch {
                color: Some([255, 0, 0]),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert_eq!(text.soft_breaks, vec![2]);
        assert_eq!(text.line_baselines, vec![48.0, 110.0]);
        assert_eq!(text.line_widths, vec![80.0, 82.0]);
        assert_eq!(text.line_origins, vec![0.0, 0.0]);
        assert_eq!(text.character_origins, original.character_origins);
        assert_eq!(text.glyph_clusters, original.glyph_clusters);
        assert_eq!(text.layout_bounds, original.layout_bounds);
        assert!(text.style_segment_origins.is_empty());
        text.apply_style(
            0,
            2,
            &TextStylePatch {
                font_size: Some(72.0),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert!(text.soft_breaks.is_empty());
        assert!(text.line_baselines.is_empty());
        assert!(text.line_widths.is_empty());
        assert!(text.line_origins.is_empty());
        assert!(text.style_segment_origins.is_empty());
        assert_eq!(text.layout_bounds, None);
    }
    #[test]
    fn selected_character_patches_preserve_other_properties_and_unicode_boundaries() {
        let mut text = VectorText {
            content: "A日😀BC".into(),
            ..Default::default()
        };
        let color = [20, 30, 40];
        text.apply_style(
            1,
            4,
            &TextStylePatch {
                bold: Some(true),
                color: Some([240, 10, 20]),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        text.apply_style(
            2,
            5,
            &TextStylePatch {
                font_size: Some(72.0),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert!(!text.style_at(0, color).bold);
        assert_eq!(text.style_at(1, color).font_size, 48.0);
        assert!(text.style_at(2, color).bold);
        assert_eq!(text.style_at(2, color).color, [240, 10, 20]);
        assert_eq!(text.style_at(2, color).font_size, 72.0);
        assert!(!text.style_at(4, color).bold);
        assert_eq!(text.style_at(4, color).font_size, 72.0);
        assert_eq!(text.style_at(5, color).font_size, 48.0);
        text.validate().unwrap();
        let before = text.clone();
        assert!(text
            .apply_style(3, 4, &TextStylePatch::default(), color)
            .is_err());
        assert_eq!(text, before, "Do not split an emoji's surrogate pair");
        assert!(text
            .apply_style(0, usize::MAX, &TextStylePatch::default(), color)
            .is_err());
        assert_eq!(text, before);
        text.apply_style(
            0,
            6,
            &TextStylePatch {
                underline: Some(true),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert!(text.runs.iter().all(|run| run.style.underline));
        assert!(text.style_at(2, color).bold);
        assert_eq!(text.style_at(4, color).font_size, 72.0);
    }
    #[test]
    fn rejects_overlapping_out_of_bounds_and_invalid_character_styles() {
        let mut text = VectorText {
            content: "a😀b".into(),
            ..Default::default()
        };
        let style = text.base_style([0, 0, 0]);
        for (start, end) in [(2, 3), (0, 5), (1, 1), (4, 3)] {
            text.runs = vec![TextRun {
                start,
                end,
                style: style.clone(),
            }];
            assert!(text.validate().is_err());
        }
        text.runs = vec![
            TextRun {
                start: 0,
                end: 3,
                style: style.clone(),
            },
            TextRun {
                start: 1,
                end: 4,
                style: style.clone(),
            },
        ];
        assert!(text.validate().is_err());
        text.runs = vec![TextRun {
            start: 0,
            end: 1,
            style: TextStyle {
                font_size: f32::NAN,
                ..style
            },
        }];
        assert!(text.validate().is_err());
    }
    #[test]
    fn equal_adjacent_styles_merge_and_legacy_text_defaults_to_no_runs() {
        let mut text: VectorText = serde_json::from_str(r#"{"content":"abc"}"#).unwrap();
        assert!(text.runs.is_empty());
        for offset in 0..3 {
            text.apply_style(
                offset,
                offset + 1,
                &TextStylePatch {
                    italic: Some(true),
                    ..Default::default()
                },
                [0, 0, 0],
            )
            .unwrap();
        }
        assert_eq!(text.runs.len(), 1);
        assert_eq!((text.runs[0].start, text.runs[0].end), (0, 3));
        assert_eq!(
            serde_json::from_str::<VectorText>(&serde_json::to_string(&text).unwrap()).unwrap(),
            text
        );
    }

    #[test]
    fn leading_accepts_direct_values_smaller_and_larger_than_the_font_size() {
        let mut text = VectorText {
            content: "Two\nlines".into(),
            font_size: 48.0,
            line_height: 30.0 / 48.0,
            ..Default::default()
        };
        text.validate().unwrap();
        text.line_height = 240.0 / 48.0;
        text.validate().unwrap();
        text.line_height = 0.0;
        assert!(text.validate().is_err());
        text.line_height = 5000.0 / 48.0;
        assert!(text.validate().is_err());
    }
}
