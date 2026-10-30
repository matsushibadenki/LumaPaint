//! Story text is independent of frame geometry; soft wraps are recomposed by InDesign.
use super::*;
use lumapaint_core::vector::{KerningMode, TextAlignment};

pub(super) fn write(
    id: &str,
    frames: &[&Frame<'_>],
    scale: f32,
    resources: &mut Resources,
) -> Result<String, ExportError> {
    let first = frames.first().ok_or_else(unsupported)?;
    let mut content = String::new();
    let mut parts = Vec::new();
    let mut runs = Vec::new();
    let mut offset = 0;
    for frame in frames {
        let t = frame.text;
        let length = t.content.encode_utf16().count();
        parts.push((offset, offset + length, *frame));
        let mut bounds = vec![0, length];
        for run in &t.runs {
            bounds.extend([run.start, run.end]);
        }
        bounds.sort_unstable();
        bounds.dedup();
        for pair in bounds.windows(2) {
            runs.push((
                offset + pair[0],
                offset + pair[1],
                t.style_at(pair[0], frame.color),
            ));
        }
        content.push_str(&t.content);
        offset += length;
    }
    if content.chars().count() > lumapaint_core::vector::MAX_TEXT_CHARACTERS {
        return Err(limit());
    }
    let mut bytes = vec![usize::MAX; offset + 1];
    let mut at = 0;
    for (byte, c) in content.char_indices() {
        bytes[at] = byte;
        at += c.len_utf16();
    }
    bytes[offset] = content.len();
    let orientation = if first.text.writing_mode == WritingMode::Vertical {
        "Vertical"
    } else {
        "Horizontal"
    };
    let mut body = format!("<Story Self=\"{id}\" UserText=\"true\" AppliedTOCStyle=\"n\" TrackChanges=\"false\" StoryTitle=\"{}\"><StoryPreference FrameType=\"TextFrameType\" StoryOrientation=\"{orientation}\" StoryDirection=\"LeftToRightDirection\"/>",xml(&first.object.name));
    let mut start = 0;
    let paragraphs: Vec<_> = content.split('\n').collect();
    for (paragraph_index, line) in paragraphs.iter().enumerate() {
        let end = start + line.encode_utf16().count();
        let paragraph_frame = parts
            .iter()
            .find(|(a, b, _)| *a <= start && start < *b)
            .map_or(*first, |(_, _, frame)| *frame);
        let t = paragraph_frame.text;
        let alignment = match t.alignment {
            TextAlignment::Left => "LeftAlign",
            TextAlignment::Center => "CenterAlign",
            TextAlignment::Right => "RightAlign",
            TextAlignment::Justify => "LeftJustified",
        };
        let _=write!(body,"<ParagraphStyleRange AppliedParagraphStyle=\"{PARAGRAPH}\" Justification=\"{alignment}\" LeftIndent=\"{}\" RightIndent=\"{}\" FirstLineIndent=\"{}\" SpaceBefore=\"{}\" SpaceAfter=\"{}\" Hyphenation=\"{}\">",t.indent_left*scale,t.indent_right*scale,t.indent_first*scale,t.space_before*scale,t.space_after*scale,t.hyphenation);
        let mut segments = Vec::new();
        for (a, b, style) in &runs {
            let (a, b) = ((*a).max(start), (*b).min(end));
            if a < b {
                segments.push((a, b, style.clone()));
            }
        }
        if segments.is_empty() {
            segments.push((start, end, t.style_at(0, paragraph_frame.color)));
        }
        let last = segments.len() - 1;
        for (i, (a, b, style)) in segments.into_iter().enumerate() {
            let source = content.get(bytes[a]..bytes[b]).ok_or_else(unsupported)?;
            let (family, variant) = resources.font(&style);
            let paint = if style.no_color {
                "Swatch/None".into()
            } else {
                resources.color(style.color)
            };
            let kerning = match style.kerning {
                KerningMode::Metrics => "$ID/Metrics",
                KerningMode::Optical => "$ID/Optical",
                KerningMode::JapaneseMonospaced => "$ID/None",
            };
            let _=write!(body,"<CharacterStyleRange AppliedCharacterStyle=\"{CHARACTER}\" FontStyle=\"{}\" PointSize=\"{}\" Tracking=\"{}\" HorizontalScale=\"{}\" VerticalScale=\"{}\" BaselineShift=\"{}\" CharacterRotation=\"{}\" Underline=\"{}\" StrikeThru=\"{}\" FillColor=\"{paint}\" StrokeColor=\"Swatch/None\" KerningMethod=\"{kerning}\" RotateSingleByteCharacters=\"{}\" Tatechuyoko=\"{}\"><Properties><AppliedFont type=\"string\">{}</AppliedFont><Leading type=\"unit\">{}</Leading></Properties><Content>{}</Content>{}</CharacterStyleRange>",xml(&variant),style.font_size*scale,style.tracking,style.scale_x*100.,style.scale_y*100.,style.baseline_shift*scale,style.rotation,style.underline,style.strikethrough,style.rotate_latin,style.tate_chu_yoko,xml(&family),style.font_size*t.line_height*scale,xml(source),if i==last && paragraph_index+1<paragraphs.len() {"<Br/>"} else {""});
        }
        body.push_str("</ParagraphStyleRange>");
        start = end + 1;
        if body.len() > LIMIT {
            return Err(limit());
        }
    }
    body.push_str("</Story>");
    Ok(body)
}
