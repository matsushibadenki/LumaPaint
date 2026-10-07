//! Native inline input: IME and selection stay in AppKit, one commit enters document history.
use super::*;
use lumapaint_core::vector::{
    KinsokuMode, MojikumiMode, ParagraphListStyle, TextAlignment, TextGlyphCluster, TextRun,
    TextStyle, TextStylePatch, VectorText,
};
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;
use objc2::{runtime::AnyObject, AnyThread, DefinedClass};
use objc2_app_kit::{
    NSBackgroundColorAttributeName, NSBaselineOffsetAttributeName, NSColor, NSCompositingOperation,
    NSFont, NSFontAttributeName, NSFontManager, NSFontTraitMask, NSForegroundColorAttributeName,
    NSKernAttributeName, NSLayoutManager, NSLigatureAttributeName, NSLineBreakMode,
    NSLineBreakStrategy, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSRectFillUsingOperation, NSSelectionAffinity, NSStrikethroughStyleAttributeName,
    NSTextAlignment, NSTextInputClient, NSTextLayoutOrientation, NSTextList,
    NSTextListMarkerDecimal, NSTextListMarkerDisc, NSTextListOptions, NSTextView,
    NSTrackingAttributeName, NSUnderlineStyleAttributeName,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSRange, NSString, NSUndoManager};

struct EditorState {
    label: String,
    undo: Retained<NSUndoManager>,
    selecting: std::cell::Cell<bool>,
}

define_class!(
    #[unsafe(super(NSTextView))]
    #[ivars = EditorState]
    struct InlineEditor;
    impl InlineEditor {
        #[unsafe(method_id(undoManager))]
        fn undo_manager(&self) -> Retained<NSUndoManager> { self.ivars().undo.clone() }
        #[unsafe(method(undo:))]
        fn undo_action(&self, _sender: Option<&AnyObject>) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; history(false); }
        #[unsafe(method(redo:))]
        fn redo_action(&self, _sender: Option<&AnyObject>) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; history(true); }

        #[unsafe(method(didChangeText))]
        fn did_change_text(&self) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            TEXT_FRAME_PENDING.with(|pending| pending.set(true));
            unsafe { msg_send![super(self), didChangeText] }
            SESSION.with(|slot| { if let Ok(slot) = slot.try_borrow() { if let Some(session) = slot.as_ref() { session.edited.set(true); } } });
            let typing_custom = decode_style(self.typingAttributes().objectForKey(&NSString::from_str(STYLE_KEY))).is_some_and(|style|style.kerning!=lumapaint_core::vector::KerningMode::Metrics || style.tate_chu_yoko || !style.rotate_latin || style.scale_x!=1. || style.scale_y!=1.);
            let settings = SESSION.with(|slot| slot.try_borrow().ok().and_then(|slot| slot.as_ref().filter(|session|typing_custom || session.settings.text.has_vertical_character_options() || session.settings.text.runs.iter().any(|r|r.style.scale_x!=1. || r.style.scale_y!=1.) || session.settings.text.kerning!=lumapaint_core::vector::KerningMode::Metrics || session.settings.text.runs.iter().any(|run|run.style.kerning!=lumapaint_core::vector::KerningMode::Metrics)).map(current_text)));
            if let Some(settings) = settings { apply_kerning_to_view(self, &settings); apply_character_advances_to_view(self, &settings); }
            hide_native_glyphs(self);
            invalidate_current_cache();
            TEXT_FRAME_PENDING.with(|pending| pending.set(true));
            if let Err(error) = request_redraw() { emit_error(error); }
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            let _keep_alive = unsafe { Retained::retain(self as *const Self as *mut Self) };
            if !self.hasMarkedText() && (event.keyCode() == 53 ||
                event.keyCode() == 36 && event.modifierFlags().contains(NSEventModifierFlags::Command)) {
                if let Err(error) = finish(event.keyCode() != 53) { emit_error(error); }
            } else { unsafe { msg_send![super(self), keyDown: event] } publish(); }
        }
        #[unsafe(method(setSelectedRange:affinity:stillSelecting:))]
        fn select_range(&self, range: NSRange, affinity: NSSelectionAffinity, selecting: bool) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            unsafe { msg_send![super(self), setSelectedRange: range, affinity: affinity, stillSelecting: selecting] }
            if !selecting && !self.ivars().selecting.get() { publish(); }
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            let _keep_alive = unsafe { Retained::retain(self as *const Self as *mut Self) };
            // AppKit tracks the entire drag in mouseDown. Publish only when the
            // tracking loop returns; serializing a long document blocks selection.
            self.ivars().selecting.set(true);
            unsafe { msg_send![super(self), mouseDown: event] }
            self.ivars().selecting.set(false);
            publish();
        }
        // Imported rich text may contain attachments or attributes outside our portable model.
        #[unsafe(method(paste:))]
        fn paste_plain(&self, sender: Option<&AnyObject>) {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            unsafe { self.pasteAsPlainText(sender) }
        }
        // Keep Cmd+A/C/V/Z in the text editor instead of the canvas responder.
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return false.into(); };
            unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }
    }
);

// NSTextView substitutes an opaque inactive-selection color when focus moves
// to the WebView. Glyphs live underneath this view in Metal, so that color
// conceals the preview. Keep native selection geometry, but paint it translucent.
define_class!(
    #[unsafe(super(NSLayoutManager))]
    #[ivars = ()]
    struct PreviewLayoutManager;
    impl PreviewLayoutManager {
        #[unsafe(method(drawGlyphsForGlyphRange:atPoint:))]
        unsafe fn draw_glyphs(&self, range: NSRange, origin: NSPoint) {
            // Metal owns glyph painting, including selected text. AppKit may choose
            // different vertical substitutions; never paint a second set of glyphs.
            let _ = (range, origin);
        }

        #[unsafe(method(fillBackgroundRectArray:count:forCharacterRange:color:))]
        unsafe fn fill_background(&self, rects: NonNull<NSRect>, count: usize, _range: NSRange, _color: &NSColor) {
            let highlight = NSColor::colorWithCalibratedRed_green_blue_alpha(0.25, 0.5, 0.9, 0.25);
            highlight.setFill();
            // Keep the same translucent highlight when focus moves to a numeric field.
            for rect in unsafe { std::slice::from_raw_parts(rects.as_ptr(), count) } {
                NSRectFillUsingOperation(*rect, NSCompositingOperation::SourceOver);
            }
        }
    }
);

// The inline NSTextView is a native subview, so WebView/CSS clipping cannot
// keep long text inside the document. Clip it in the same AppKit hierarchy.
define_class!(
    #[unsafe(super(NSView))]
    #[ivars = ()]
    struct PageClip;
    impl PageClip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
    }
);

/// One verified preview per editor. Pan/zoom/selection redraws reuse the same
/// document and journal; native layout and document ownership stay on main.
#[derive(Default)]
struct PreviewCache {
    value: Option<(u64, Document)>,
    new_object_id: Option<String>,
}
impl PreviewCache {
    fn update_existing(
        &mut self,
        generation: u64,
        base: &Document,
        settings: impl FnOnce() -> TextSettings,
    ) -> Result<&Document, String> {
        if self
            .value
            .as_ref()
            .is_none_or(|(saved, _)| *saved != generation)
        {
            let settings = settings();
            if let Some((saved, preview)) = &mut self.value {
                preview.update_text_preview(settings)?;
                *saved = generation;
            } else {
                let mut preview = base.clone();
                preview.update_text_preview(settings)?;
                self.value = Some((generation, preview));
            }
        }
        Ok(&self.value.as_ref().unwrap().1)
    }

    fn update_new(
        &mut self,
        generation: u64,
        base: &Document,
        settings: impl FnOnce() -> TextSettings,
    ) -> Result<&Document, String> {
        if self
            .value
            .as_ref()
            .is_some_and(|(saved, _)| *saved == generation)
        {
            return Ok(&self.value.as_ref().unwrap().1);
        }
        let mut settings = settings();
        if let Some(id) = &self.new_object_id {
            settings.id = Some(id.clone());
            return self.update_existing(generation, base, || settings);
        }
        if settings.text.content.trim().is_empty() && settings.text.box_height.is_none() {
            if let Some((saved, _)) = &mut self.value {
                *saved = generation;
            } else {
                self.value = Some((generation, base.clone()));
            }
        } else {
            // Allocate the temporary object once. Its ID stays private to this preview;
            // commit still creates the real object through the canonical document command.
            let preview = live_preview(base, settings)?;
            let id = preview
                .snapshot()
                .selected_vector_objects
                .into_iter()
                .next()
                .ok_or("New text preview object missing")?;
            self.new_object_id = Some(id);
            self.value = Some((generation, preview));
        }
        Ok(&self.value.as_ref().unwrap().1)
    }

    #[cfg(test)]
    fn get_or_update(
        &mut self,
        generation: u64,
        build: impl FnOnce() -> Result<Document, String>,
    ) -> Result<&Document, String> {
        if self
            .value
            .as_ref()
            .is_none_or(|(saved, _)| *saved != generation)
        {
            // Keep the last valid entry on failure, but never label it as current.
            self.value = Some((generation, build()?));
        }
        Ok(&self.value.as_ref().unwrap().1)
    }
}

struct Session {
    page_clip: Retained<PageClip>,
    view: Retained<InlineEditor>,
    settings: TextSettings,
    preview: Document,
    rendered_preview: RefCell<PreviewCache>,
    object_transform: [f32; 6],
    edited: std::cell::Cell<bool>,
    layout_generation: std::cell::Cell<u64>,
    current_cache: RefCell<Option<(u64, TextSettings)>>,
    layout_size: std::cell::Cell<Option<[f32; 2]>>,
}
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static TEXT_FRAME_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn font_families() -> Result<Vec<String>, String> {
    lumapaint_fonts::register_native_fonts();
    let mtm = MainThreadMarker::new().ok_or("Text editing requires the main thread")?;
    let mut names: Vec<_> = NSFontManager::sharedFontManager(mtm)
        .availableFontFamilies()
        .iter()
        .map(|name| name.to_string())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn active() -> bool {
    SESSION.with(|slot| slot.borrow().is_some())
}

pub fn render(canvas: &mut Canvas) -> Result<(), String> {
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow().as_ref() {
            let overlay = super::text_frame_overlay(&session.settings, false).map(|mut overlay| {
                let [a, b, c, d, _, _] = session.object_transform;
                let [x, y] = session.settings.position;
                for point in overlay
                    .corners
                    .iter_mut()
                    .chain(overlay.baseline.iter_mut().flatten())
                {
                    let local = [point[0] - x, point[1] - y];
                    *point = [
                        x + a * local[0] + c * local[1],
                        y + b * local[0] + d * local[1],
                    ];
                }
                overlay
            });
            canvas
                .renderer
                .set_selection_overlay_visible(!session.settings.text.point_text);
            canvas.renderer.set_frame_overlay(overlay);
            let mut cache = session.rendered_preview.borrow_mut();
            let current = || cached_current(session).unwrap_or_else(|| session.settings.clone());
            let preview = if session.settings.id.is_some() {
                cache.update_existing(session.layout_generation.get(), &session.preview, current)?
            } else {
                cache.update_new(session.layout_generation.get(), &session.preview, current)?
            };
            canvas.renderer.render(canvas.viewport, preview)
        } else {
            DOCUMENT.with(|doc| {
                canvas.renderer.render_vector_drag(
                    canvas.viewport,
                    &doc.borrow(),
                    VECTOR_MOVE.with(|offset| offset.get()),
                )
            })
        }
    })
}

const STYLE_KEY: &str = "LumaPaintCharacterStyle";

fn decode_style(value: Option<Retained<AnyObject>>) -> Option<TextStyle> {
    let value = value?.downcast::<NSString>().ok()?;
    serde_json::from_str(&value.to_string()).ok()
}
fn current_text(session: &Session) -> TextSettings {
    let mut settings = session.settings.clone();
    settings.text.content = session.view.string().to_string();
    settings.text.runs.clear();
    settings.text.clear_measured_layout();
    if let Some(storage) = unsafe { session.view.textStorage() } {
        let key = NSString::from_str(STYLE_KEY);
        let mut at = 0;
        while at < storage.length() {
            let mut range = NSRange::new(at, 1);
            let value = unsafe { storage.attribute_atIndex_effectiveRange(&key, at, &mut range) };
            let style =
                decode_style(value).unwrap_or_else(|| settings.text.base_style(settings.color));
            let end = (range.location + range.length)
                .min(storage.length())
                .max(at + 1);
            if let Some(last) = settings
                .text
                .runs
                .last_mut()
                .filter(|run| run.style == style)
            {
                last.end = end;
            } else {
                settings.text.runs.push(TextRun {
                    start: at,
                    end,
                    style,
                });
            }
            at = end;
        }
    }
    settings
}
fn current(session: &Session) -> TextSettings {
    let mut settings = current_text(session);
    let (breaks, baselines, widths, origins, segments, characters, clusters, bounds) =
        measured_layout(&session.view, &settings.text, settings.color);
    settings.text.soft_breaks = breaks;
    settings.text.line_baselines = baselines;
    settings.text.line_widths = widths;
    settings.text.line_origins = origins;
    settings.text.style_segment_origins = segments;
    settings.text.character_origins = characters;
    settings.text.glyph_clusters = clusters;
    settings.text.layout_bounds = bounds;
    settings
        .text
        .normalize_vertical_group_positions(settings.color);
    settings
}

type MeasuredTextLayout = (
    Vec<usize>,
    Vec<f32>,
    Vec<f32>,
    Vec<f32>,
    Vec<Vec<f32>>,
    Vec<Vec<f32>>,
    Vec<Vec<TextGlyphCluster>>,
    Option<[f32; 4]>,
);

fn measured_layout(view: &NSTextView, text: &VectorText, color: [u8; 3]) -> MeasuredTextLayout {
    let mut breaks = Vec::new();
    let mut baselines = Vec::new();
    let mut widths = Vec::new();
    let mut origins = Vec::new();
    let mut segments = Vec::new();
    let mut characters = Vec::new();
    let mut clusters = Vec::new();
    let mut bounds = None;
    if let (Some(manager), Some(container)) = (unsafe { view.layoutManager() }, unsafe {
        view.textContainer()
    }) {
        manager.ensureLayoutForTextContainer(&container);
        let glyph_range = manager.glyphRangeForTextContainer(&container);
        let origin = view.textContainerOrigin();
        let mut y_correction = 0.0;
        if glyph_range.length > 0 {
            let rect = manager.boundingRectForGlyphRange_inTextContainer(glyph_range, &container);
            let line = unsafe {
                manager.lineFragmentRectForGlyphAtIndex_effectiveRange(0, std::ptr::null_mut())
            };
            let natural_baseline = line.origin.y + manager.locationForGlyphAtIndex(0).y + origin.y;
            if text.writing_mode != lumapaint_core::vector::WritingMode::Vertical {
                y_correction = f64::from(text.style_at(0, color).font_size + text.space_before)
                    - natural_baseline;
            }
            let candidate = [
                (rect.origin.x + origin.x) as f32,
                (rect.origin.y + origin.y + y_correction) as f32,
                rect.size.width as f32,
                rect.size.height as f32,
            ];
            if candidate
                .iter()
                .all(|value| value.is_finite() && value.abs() < 100_000.0)
                && candidate[2] > 0.0
                && candidate[3] > 0.0
                && (candidate[0] + candidate[2]).abs() < 100_000.0
                && (candidate[1] + candidate[3]).abs() < 100_000.0
            {
                bounds = Some(
                    if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
                        [
                            text.box_width - candidate[1] - candidate[3],
                            candidate[0],
                            candidate[3],
                            candidate[2],
                        ]
                    } else {
                        candidate
                    },
                );
            }
        }
        let utf16: Vec<_> = text.content.encode_utf16().collect();
        let mut glyph = 0;
        while glyph < manager.numberOfGlyphs() {
            let mut range = NSRange::new(0, 0);
            let line = unsafe {
                manager.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, &mut range)
            };
            let used = unsafe {
                manager
                    .lineFragmentUsedRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut())
            };
            let baseline =
                line.origin.y + manager.locationForGlyphAtIndex(glyph).y + origin.y + y_correction;
            let column_baseline =
                if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
                    // A vertical fragment spans the complete column, including
                    // its largest run. Its center is independent of the first font.
                    line.origin.y + line.size.height * 0.5 + origin.y
                } else {
                    baseline
                };
            baselines.push(column_baseline as f32);
            widths.push(used.size.width as f32);
            origins
                .push((line.origin.x + manager.locationForGlyphAtIndex(glyph).x + origin.x) as f32);
            if range.location > 0 {
                let offset = manager.characterIndexForGlyphAtIndex(range.location);
                if offset > 0
                    && offset < utf16.len()
                    && utf16[offset - 1] != b'\n' as u16
                    && utf16[offset] != b'\n' as u16
                    && !(0xD800..=0xDBFF).contains(&utf16[offset - 1])
                    && breaks.last().is_none_or(|last| *last < offset)
                {
                    breaks.push(offset);
                }
            }
            glyph = (range.location + range.length).max(glyph + 1);
        }
    }
    // A terminal newline creates an extra paragraph without a glyph. Include
    // it rather than discarding every preceding measured baseline and width.
    if text.content.ends_with('\n') {
        if let Some(manager) = unsafe { view.layoutManager() } {
            let extra = manager.extraLineFragmentRect();
            let used = manager.extraLineFragmentUsedRect();
            let origin = view.textContainerOrigin();
            let baseline = if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
                extra.origin.y + extra.size.height * 0.5 + origin.y
            } else {
                // The empty final paragraph draws no glyph. Preserve its position
                // after the last populated line without affecting their metrics.
                baselines.last().map_or(text.font_size, |last| {
                    *last
                        + extra
                            .size
                            .height
                            .max(f64::from(text.font_size * text.line_height))
                            as f32
                }) as f64
            };
            baselines.push(baseline as f32);
            widths.push(used.size.width as f32);
            origins.push((used.origin.x + origin.x) as f32);
        }
    }
    let mut measured_text = text.clone();
    measured_text.soft_breaks = breaks.clone();
    if baselines.len() != measured_text.visual_lines().len()
        || baselines
            .iter()
            .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
        || baselines.windows(2).any(|pair| pair[0] >= pair[1])
    {
        baselines.clear();
    }
    if widths.len() != measured_text.visual_lines().len()
        || widths
            .iter()
            .any(|value| !value.is_finite() || !(0.0..100_000.0).contains(value))
    {
        widths.clear();
    }
    if origins.len() != measured_text.visual_lines().len()
        || origins
            .iter()
            .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
    {
        origins.clear();
    }
    if let Some(manager) = unsafe { view.layoutManager() } {
        for (start, line, _) in measured_text.visual_lines() {
            let mut line_segments = Vec::new();
            for offset in measured_text.style_segment_starts(start, line, color) {
                let glyph = manager.glyphIndexForCharacterAtIndex(offset);
                if glyph >= manager.numberOfGlyphs() {
                    line_segments.clear();
                    break;
                }
                let fragment = unsafe {
                    manager
                        .lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut())
                };
                let x = fragment.origin.x
                    + manager.locationForGlyphAtIndex(glyph).x
                    + view.textContainerOrigin().x;
                line_segments.push(x as f32);
            }
            segments.push(line_segments);
        }
    }
    if segments.len() != measured_text.visual_lines().len()
        || segments
            .iter()
            .zip(measured_text.visual_lines())
            .any(|(positions, (start, line, _))| {
                positions.len() != measured_text.style_segment_starts(start, line, color).len()
                    || positions
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
            })
    {
        segments.clear();
    }
    if let Some(manager) = unsafe { view.layoutManager() } {
        for (start, line, _) in measured_text.visual_lines() {
            let mut line_characters = Vec::with_capacity(line.chars().count());
            let mut offset = start;
            for character in line.chars() {
                let glyph = manager.glyphIndexForCharacterAtIndex(offset);
                if glyph >= manager.numberOfGlyphs() {
                    line_characters.clear();
                    break;
                }
                let fragment = unsafe {
                    manager
                        .lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut())
                };
                let x = fragment.origin.x
                    + manager.locationForGlyphAtIndex(glyph).x
                    + view.textContainerOrigin().x;
                line_characters.push(x as f32);
                offset += character.len_utf16();
            }
            characters.push(line_characters);
        }
    }
    if characters.len() != measured_text.visual_lines().len()
        || characters
            .iter()
            .zip(measured_text.visual_lines())
            .any(|(positions, (_, line, _))| {
                positions.len() != line.chars().count()
                    || positions
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
                    || positions.windows(2).any(|pair| pair[0] > pair[1])
            })
    {
        characters.clear();
    }
    if let Some(manager) = unsafe { view.layoutManager() } {
        for (start, line, _) in measured_text.visual_lines() {
            let line_len = line.encode_utf16().count();
            let line_end = start + line_len;
            let mut at = start;
            let mut line_clusters = Vec::new();
            while at < line_end {
                let glyph = manager.glyphIndexForCharacterAtIndex(at);
                if glyph >= manager.numberOfGlyphs() {
                    line_clusters.clear();
                    break;
                }
                let character_range = unsafe {
                    manager.characterRangeForGlyphRange_actualGlyphRange(
                        NSRange::new(glyph, 1),
                        std::ptr::null_mut(),
                    )
                };
                let cluster_start = character_range.location.max(start);
                let cluster_end = (character_range.location + character_range.length).min(line_end);
                if cluster_start != at || cluster_end <= cluster_start {
                    line_clusters.clear();
                    break;
                }
                let fragment = unsafe {
                    manager
                        .lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut())
                };
                let x = fragment.origin.x
                    + manager.locationForGlyphAtIndex(glyph).x
                    + view.textContainerOrigin().x;
                line_clusters.push(TextGlyphCluster {
                    start: cluster_start - start,
                    end: cluster_end - start,
                    x: x as f32,
                });
                at = cluster_end;
            }
            clusters.push(line_clusters);
        }
    }
    if clusters.len() != measured_text.visual_lines().len()
        || clusters
            .iter()
            .zip(measured_text.visual_lines())
            .any(|(line_clusters, (_, line, _))| {
                let line_len = line.encode_utf16().count();
                let mut end = 0;
                for cluster in line_clusters {
                    if cluster.start != end
                        || cluster.end <= cluster.start
                        || cluster.end > line_len
                        || !cluster.x.is_finite()
                        || cluster.x.abs() >= 100_000.0
                    {
                        return true;
                    }
                    end = cluster.end;
                }
                end != line_len || (line_len > 0 && line_clusters.is_empty())
            })
    {
        clusters.clear();
    }
    (
        breaks, baselines, widths, origins, segments, characters, clusters, bounds,
    )
}

/// Reflow a committed panel edit with the same AppKit font and paragraph attributes
/// used by inline editing, without displaying or focusing another editor.
pub fn reflow(settings: &mut TextSettings) -> Result<(), String> {
    settings.text.clear_measured_layout();
    settings.text.validate()?;
    if settings.id.is_none() {
        DOCUMENT.with(|doc| {
            let document = doc.borrow();
            if document.has_active_saved_path() {
                Err(
                    "Paths accept geometry only / パスには文字を追加できません / 路径不支持文字"
                        .to_string(),
                )
            } else {
                document.selected_vector_target()
            }
        })?;
    }
    let mtm = MainThreadMarker::new().ok_or("Text layout requires the main thread")?;
    let view = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(settings.text.box_width.into(), 100_000.0),
        ),
    );
    view.setTextContainerInset(NSSize::new(0.0, 0.0));
    view.setString(&NSString::from_str(&settings.text.content));
    view.setHorizontallyResizable(false);
    view.setVerticallyResizable(false);
    if let Some(container) = unsafe { view.textContainer() } {
        container.setLineFragmentPadding(0.0);
        container.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        container.setWidthTracksTextView(!settings.text.point_text);
        if settings.text.point_text {
            container.setContainerSize(NSSize::new(100_000.0, 100_000.0));
        }
    }
    if settings.text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
        let width = settings.text.box_width;
        let height = if settings.text.point_text {
            100_000.0
        } else {
            settings.text.box_height.unwrap_or(width)
        };
        view.setLayoutOrientation(NSTextLayoutOrientation::Vertical);
        view.setFrame(NSRect::new(
            NSPoint::new(0., 0.),
            NSSize::new(width.into(), height.into()),
        ));
        view.setBoundsSize(NSSize::new(width.into(), height.into()));
        if let Some(container) = unsafe { view.textContainer() } {
            container.setWidthTracksTextView(false);
            container.setHeightTracksTextView(false);
            container.setContainerSize(vertical_container_size(width, height));
        }
    }
    if let Some(manager) = unsafe { view.layoutManager() } {
        manager.setUsesFontLeading(false);
    }
    apply_attributes_to_view(&view, settings)?;
    let (breaks, baselines, widths, origins, segments, characters, clusters, bounds) =
        measured_layout(&view, &settings.text, settings.color);
    settings.text.soft_breaks = breaks;
    settings.text.line_baselines = baselines;
    settings.text.line_widths = widths;
    settings.text.line_origins = origins;
    settings.text.style_segment_origins = segments;
    settings.text.character_origins = characters;
    settings.text.glyph_clusters = clusters;
    settings.text.layout_bounds = bounds;
    settings
        .text
        .normalize_vertical_group_positions(settings.color);
    settings.text.validate()
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionStyle {
    start: usize,
    length: usize,
    characters: usize,
    style: TextStyle,
    mixed: Vec<String>,
}
#[derive(Clone, serde::Serialize)]
struct SessionEvent {
    #[serde(flatten)]
    settings: TextSettings,
    selection: SelectionStyle,
}
fn selection_style(session: &Session, settings: &TextSettings) -> SelectionStyle {
    let range = session.view.selectedRange();
    let style = if range.length == 0 {
        decode_style(
            session
                .view
                .typingAttributes()
                .objectForKey(&NSString::from_str(STYLE_KEY)),
        )
        .unwrap_or_else(|| {
            settings
                .text
                .style_at(range.location.saturating_sub(1), settings.color)
        })
    } else {
        settings.text.style_at(range.location, settings.color)
    };
    let first = serde_json::to_value(&style).unwrap_or_default();
    let mut mixed = Vec::new();
    for run in &settings.text.runs {
        if range.length > 0 && run.start < range.location + range.length && run.end > range.location
        {
            if let Some(map) = serde_json::to_value(&run.style)
                .ok()
                .and_then(|v| v.as_object().cloned())
            {
                for (key, value) in map {
                    if first.get(&key) != Some(&value) && !mixed.contains(&key) {
                        mixed.push(key);
                    }
                }
            }
        }
    }
    let selected: Vec<u16> = settings
        .text
        .content
        .encode_utf16()
        .skip(range.location)
        .take(range.length)
        .collect();
    SelectionStyle {
        start: range.location,
        length: range.length,
        characters: String::from_utf16_lossy(&selected).chars().count(),
        style,
        mixed,
    }
}
pub(super) fn prepare_frame() {
    if TEXT_FRAME_PENDING.with(|pending| pending.replace(false)) {
        publish();
    }
}

fn publish() {
    if TEXT_FRAME_PENDING.with(|pending| pending.get()) {
        let _ = request_redraw();
        return;
    }
    // AppKit can notify selection changes while layout or session teardown holds the slot.
    let settings = SESSION.with(|slot| {
        let Ok(slot) = slot.try_borrow() else {
            return None;
        };
        let Some(session) = slot.as_ref() else {
            return Some(None);
        };
        let settings = cached_current(session)?;
        let selection = selection_style(session, &settings);
        Some(Some(SessionEvent {
            settings,
            selection,
        }))
    });
    if let (Some(app), Some(settings)) = (APP.get(), settings) {
        let _ = app.emit_to(current_label(), "canvas-text-session", settings);
    }
}

fn invalidate_current_cache() {
    SESSION.with(|slot| {
        if let Ok(slot) = slot.try_borrow() {
            if let Some(session) = slot.as_ref() {
                session
                    .layout_generation
                    .set(session.layout_generation.get().wrapping_add(1));
            }
        }
    });
}

fn cached_current(session: &Session) -> Option<TextSettings> {
    if !session.edited.get() {
        return Some(session.settings.clone());
    }
    let generation = session.layout_generation.get();
    let mut cache = session.current_cache.try_borrow_mut().ok()?;
    if let Some((saved_generation, settings)) = cache.as_ref() {
        if *saved_generation == generation {
            return Some(settings.clone());
        }
    }
    let settings = current(session);
    if session.layout_generation.get() == generation {
        *cache = Some((generation, settings.clone()));
    }
    Some(settings)
}

fn point_text(vertical: bool) -> VectorText {
    let mut text = VectorText {
        content: "Text".into(),
        point_text: true,
        ..Default::default()
    };
    if vertical {
        text.writing_mode = lumapaint_core::vector::WritingMode::Vertical;
        text.box_width = (text.font_size * text.line_height).max(16.0);
    }
    text
}

pub fn begin_at(point: [f32; 2], create: bool) -> Result<(), String> {
    finish(true)?;
    let id = DOCUMENT.with(|doc| doc.borrow().text_at(point));
    let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
    let settings = if let Some(object) = snapshot
        .text_objects
        .iter()
        .find(|text| Some(&text.id) == id.as_ref())
    {
        if !object.editable {
            return Err("Text layer is locked or hidden".into());
        }
        TextSettings {
            id: Some(object.id.clone()),
            text: object.text.clone(),
            position: object.position,
            color: object.color,
        }
    } else if create {
        TextSettings {
            id: None,
            text: {
                let mut text = point_text(TOOL.with(|tool| tool.get()) == CanvasTool::TextVertical);
                text.no_color = BRUSH.with(|brush| brush.borrow().no_color);
                text
            },
            position: point,
            color: BRUSH.with(|brush| brush.borrow().color),
        }
    } else {
        return Ok(());
    };
    begin(settings)
}

pub fn begin(mut settings: TextSettings) -> Result<(), String> {
    lumapaint_fonts::register_native_fonts();
    finish(true)?;
    if settings.text.point_text && settings.text.layout_bounds.is_none() {
        reflow(&mut settings)?;
    }
    settings.text.validate()?;
    // Reject an incompatible destination before creating an inline editor. Otherwise
    // the error is deferred until a tool switch commits the text during canvas sync.
    if settings.id.is_none() {
        DOCUMENT.with(|doc| {
            let document = doc.borrow();
            if document.has_active_saved_path() {
                Err(
                    "Paths accept geometry only / パスには文字を追加できません / 路径不支持文字"
                        .to_string(),
                )
            } else {
                document.selected_vector_target()
            }
        })?;
    }
    if !DOCUMENT_OPEN.with(|open| open.get()) {
        return Err("No document is open".into());
    }
    if settings
        .position
        .iter()
        .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
    {
        return Err("Invalid text position".into());
    }
    let object_transform = DOCUMENT.with(|doc| {
        doc.borrow()
            .svg_layers()
            .flat_map(|layer| layer.vector_objects.iter())
            .find(|object| Some(&object.id) == settings.id.as_ref())
            .map_or([1., 0., 0., 1., 0., 0.], |object| object.transform)
    });
    // Validate editability, then retain the original visible object for GPU rendering.
    DOCUMENT.with(|doc| doc.borrow().text_edit_preview(settings.id.as_deref()))?;
    let preview = DOCUMENT.with(|doc| doc.borrow().clone());
    let mtm = MainThreadMarker::new().ok_or("Text editing requires the main thread")?;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 200.0));
    let allocated = InlineEditor::alloc(mtm).set_ivars(EditorState {
        label: current_label(),
        undo: NSUndoManager::new(mtm),
        selecting: std::cell::Cell::new(false),
    });
    let view: Retained<InlineEditor> = unsafe { msg_send![super(allocated), initWithFrame: frame] };
    let clip_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
    let allocated = PageClip::alloc(mtm).set_ivars(());
    let page_clip: Retained<PageClip> =
        unsafe { msg_send![super(allocated), initWithFrame: clip_frame] };
    page_clip.setClipsToBounds(true);
    // Native selection painting must also be clipped in Core Animation, including rotated text.
    page_clip.setWantsLayer(true);
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![&*page_clip, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
    view.setRichText(true);
    view.setImportsGraphics(false);
    if let Some(container) = unsafe { view.textContainer() } {
        let manager: Retained<PreviewLayoutManager> =
            unsafe { msg_send![super(PreviewLayoutManager::alloc().set_ivars(())), init] };
        container.replaceLayoutManager(&manager);
    }
    if let Some(manager) = unsafe { view.layoutManager() } {
        manager.setUsesFontLeading(false);
    }
    view.setAllowsUndo(true);
    view.setDrawsBackground(false);
    view.setTextContainerInset(NSSize::new(0.0, 0.0));
    view.setString(&NSString::from_str(&settings.text.content));
    view.setHorizontallyResizable(false);
    view.setVerticallyResizable(false);
    if let Some(container) = unsafe { view.textContainer() } {
        container.setLineFragmentPadding(0.0);
        container.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        container.setWidthTracksTextView(true);
    }
    CANVAS.with(|slot| -> Result<(), String> {
        let slot = slot.borrow();
        let canvas = slot.as_ref().ok_or("Canvas is not ready")?;
        canvas.view.addSubview(&page_clip);
        page_clip.addSubview(&view);
        Ok(())
    })?;
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            page_clip,
            view: view.clone(),
            settings,
            preview,
            rendered_preview: RefCell::new(PreviewCache::default()),
            object_transform,
            edited: std::cell::Cell::new(false),
            layout_generation: std::cell::Cell::new(0),
            current_cache: RefCell::new(None),
            layout_size: std::cell::Cell::new(None),
        })
    });
    apply_all_attributes()?;
    layout()?;
    redraw()?;
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(&view));
    }
    view.setSelectedRange(NSRange::new(0, view.string().length()));
    publish();
    Ok(())
}

/// Color controls send only a color and object identity. The live AppKit selection
/// and text stay in Rust, including drafts that have not entered the document yet.
pub fn set_color(id: Option<String>, color: [u8; 3]) -> Result<(), String> {
    let settings = SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|session| session.settings.id == id)
            .map(|session| session.settings.clone())
    });
    if let Some(settings) = settings {
        update(
            settings,
            Some(TextStylePatch {
                color: Some(color),
                ..Default::default()
            }),
        )?;
    }
    Ok(())
}

pub fn update(settings: TextSettings, patch: Option<TextStylePatch>) -> Result<(), String> {
    let prepared = SESSION.with(|slot| -> Result<_, String> {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return Ok(None);
        };
        if session.settings.id != settings.id {
            return Err("A different text object is being edited".into());
        }
        let current = cached_current(session).unwrap_or_else(|| current(session));
        let selection = selection_style(session, &current);
        let mut next = settings;
        next.text.content = current.text.content;
        next.text.runs = current.text.runs;
        let mut typing = selection.style;
        if let Some(patch) = &patch {
            next.text.apply_style(
                selection.start,
                selection.start + selection.length,
                patch,
                next.color,
            )?;
            typing.apply(patch);
            typing.validate()?;
        }
        let mut validation = next.text.clone();
        if validation.content.trim().is_empty() {
            validation.content = "Text".into();
            validation.runs.clear();
        }
        validation.validate()?;
        if next
            .position
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
        {
            return Err("Invalid text position".into());
        }
        Ok(Some((
            session.view.clone(),
            next,
            typing,
            NSRange::new(selection.start, selection.length),
        )))
    })?;
    let Some((view, next, typing, range)) = prepared else {
        return Ok(());
    };
    if patch.is_some()
        && range.length > 0
        && !view.shouldChangeTextInRange_replacementString(range, None)
    {
        return Ok(());
    }
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            session.edited.set(true);
            session.settings = next;
            session
                .layout_generation
                .set(session.layout_generation.get().wrapping_add(1));
        }
    });
    TEXT_FRAME_PENDING.with(|pending| pending.set(true));
    apply_all_attributes()?;
    // Attribute replacement must not turn the selected range into an insertion
    // point when focus is in a WebView color field or the system color picker.
    view.setSelectedRange(range);
    let text = SESSION.with(|slot| slot.borrow().as_ref().unwrap().settings.text.clone());
    let attributes = attributes(&text, &typing)?;
    unsafe {
        view.setTypingAttributes(&attributes);
    }
    hide_native_glyphs(&view);
    if patch.is_some() && range.length > 0 {
        view.didChangeText();
    }
    layout()?;
    // A confirmed panel value must finish layout and GPU presentation before
    // returning to the WebView. AppKit also retains the inactive selection's
    // backing image until explicitly invalidated after the font metrics change.
    invalidate_current_cache();
    redraw()?;
    view.setNeedsDisplay(true);
    view.displayIfNeeded();
    Ok(())
}

pub fn finish(commit: bool) -> Result<(), String> {
    TEXT_FRAME_PENDING.with(|pending| pending.set(false));
    if !active() {
        return Ok(());
    }
    let session = SESSION.with(|slot| -> Result<Option<Session>, String> {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_ref() else {
            return Ok(None);
        };
        let mut settings = cached_current(session).unwrap_or_else(|| current(session));
        let base = settings.text.base_style(settings.color);
        settings.text.runs.retain(|run| run.style != base);
        if commit
            && (session.edited.get() || session.settings.id.is_none())
            && (!settings.text.content.trim().is_empty() || settings.text.box_height.is_some())
        {
            DOCUMENT.with(|doc| doc.borrow_mut().set_text_object(settings))?;
        }
        Ok(slot.take())
    })?;
    // Resigning an IME responder may call back into didChangeText. Release the slot borrow first.
    if let Some(session) = session {
        if let Some(window) = session.view.window() {
            let canvas =
                CANVAS.with(|slot| slot.borrow().as_ref().map(|canvas| canvas.view.clone()));
            window.makeFirstResponder(canvas.as_deref().map(|view| &***view));
        }
        session.view.ivars().undo.removeAllActions();
        // Keep the native glyphs visible until the committed GPU image is ready.
        // A deferred first frame would otherwise show the text-edit preview,
        // which intentionally excludes this object.
        let rendered = CANVAS.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(canvas) = slot.as_mut() else {
                return Ok(());
            };
            canvas.renderer.set_frame_overlay(None);
            DOCUMENT.with(|document| canvas.renderer.render(canvas.viewport, &document.borrow()))
        });
        session.page_clip.removeFromSuperview();
        rendered?;
    }
    publish();
    redraw()?;
    emit_document();
    Ok(())
}

// Native text layout remains in original font units. Only the view is scaled,
// so reading edited attributes back cannot bake the object transform in twice.
fn editor_display_transform(text: &VectorText, matrix: [f32; 6]) -> (f32, f32, f32) {
    let [a, b, c, d, _, _] = matrix;
    let (sin, cos) = text.rotation.to_radians().sin_cos();
    let x = [a * cos + c * sin, b * cos + d * sin];
    let y = [-a * sin + c * cos, -b * sin + d * cos];
    (
        text.scale_x * x[0].hypot(x[1]),
        text.scale_y * y[0].hypot(y[1]),
        x[1].atan2(x[0]).to_degrees(),
    )
}

/// The artboard is a page inside the workspace; native selection stays within the canvas.
fn workspace_editor_rect(page: NSRect, viewport: NSSize) -> (NSRect, NSPoint) {
    (NSRect::new(NSPoint::new(0.0, 0.0), viewport), page.origin)
}

fn vertical_container_size(_width: f32, height: f32) -> NSSize {
    // Lay out overflow as well: unlaid glyphs have zero fragments and would
    // invalidate all measured column positions. The physical view/frame clips it.
    NSSize::new(height.into(), 100_000.0)
}

pub fn layout() -> Result<(), String> {
    let viewport = CANVAS.with(|slot| slot.borrow().as_ref().map(|canvas| canvas.viewport));
    let Some(viewport) = viewport else {
        return Ok(());
    };
    SESSION.with(|slot| -> Result<(), String> {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return Ok(());
        };
        let view = &session.view;
        let text = &session.settings.text;
        let vertical = text.writing_mode == lumapaint_core::vector::WritingMode::Vertical;
        view.setLayoutOrientation(if vertical {
            NSTextLayoutOrientation::Vertical
        } else {
            NSTextLayoutOrientation::Horizontal
        });
        let width = viewport.width as f32 / viewport.scale;
        let height = viewport.height as f32 / viewport.scale;
        let fit = ((width - 48.0) / viewport.document_width)
            .min((height - 48.0) / viewport.document_height)
            .max(0.01)
            * viewport.zoom;
        let page_x = width * 0.5 + viewport.pan_x - viewport.document_width * fit * 0.5;
        let page_y = height * 0.5 + viewport.pan_y - viewport.document_height * fit * 0.5;
        let page = NSRect::new(
            NSPoint::new(page_x.into(), page_y.into()),
            NSSize::new(
                (viewport.document_width * fit).into(),
                (viewport.document_height * fit).into(),
            ),
        );
        let (clip, offset) = workspace_editor_rect(page, NSSize::new(width.into(), height.into()));
        session.page_clip.setFrame(clip);
        session
            .page_clip
            .setHidden(clip.size.width == 0.0 || clip.size.height == 0.0);
        let (scale_x, scale_y, rotation) = editor_display_transform(text, session.object_transform);
        let document_y = session.settings.position[1] * fit;
        // Shift the editor relative to the canvas viewport so zoom/pan never moves the actual text.
        let x = session.settings.position[0] * fit + offset.x as f32;
        let y = document_y + offset.y as f32;
        let logical_height = if text.point_text && vertical {
            100_000.0
        } else {
            text.box_height
                .or_else(|| vertical.then_some(text.box_width))
                .unwrap_or_else(|| {
                    ((height - page_y - document_y) / (fit * scale_y)).max(text.font_size * 2.0)
                })
        };
        let logical_width = if text.point_text && !vertical {
            100_000.0
        } else {
            text.box_width
        };
        let layout_size = [
            logical_width,
            if vertical { logical_height } else { 100_000.0 },
        ];
        if session.layout_size.replace(Some(layout_size)) != Some(layout_size) {
            session
                .layout_generation
                .set(session.layout_generation.get().wrapping_add(1));
        }
        view.setFrameRotation(0.0);
        view.setFrame(NSRect::new(
            NSPoint::new(x.into(), y.into()),
            NSSize::new(
                (logical_width * fit * scale_x).into(),
                (logical_height * fit * scale_y).into(),
            ),
        ));
        view.setBoundsSize(NSSize::new(logical_width.into(), logical_height.into()));
        if let Some(container) = unsafe { view.textContainer() } {
            container.setWidthTracksTextView(!vertical && !text.point_text);
            // NSTextView rotates its internal coordinate system for vertical text.
            // The container's width is therefore the physical column height.
            // Automatic height tracking would overwrite the cross-column extent
            // with the physical view height during zooming or resizing.
            container.setHeightTracksTextView(false);
            if vertical {
                container.setContainerSize(vertical_container_size(text.box_width, logical_height));
            } else {
                container.setContainerSize(NSSize::new(logical_width.into(), 100_000.0));
            }
        }
        view.setFrameRotation(f64::from(rotation));
        // AppKit centers a font's baseline within the requested line height. SVG uses the
        // explicit baseline font_size + space_before. Align the editor's first baseline
        // without changing glyph attributes, selection, or document coordinates.
        unsafe {
            if let (Some(manager), Some(container)) = (view.layoutManager(), view.textContainer()) {
                manager.ensureLayoutForTextContainer(&container);
                if manager.numberOfGlyphs() > 0 && !vertical {
                    let fragment = manager
                        .lineFragmentRectForGlyphAtIndex_effectiveRange(0, std::ptr::null_mut());
                    let natural = fragment.origin.y
                        + manager.locationForGlyphAtIndex(0).y
                        + view.textContainerOrigin().y;
                    let expected = f64::from(
                        text.style_at(0, session.settings.color).font_size + text.space_before,
                    );
                    let correction = (natural - expected) * f64::from(fit * scale_y);
                    let angle = f64::from(rotation).to_radians();
                    view.setFrameOrigin(NSPoint::new(
                        f64::from(x) + correction * angle.sin(),
                        f64::from(y) - correction * angle.cos(),
                    ));
                }
            }
        }
        Ok(())
    })
}

pub fn history(redo: bool) {
    let view = SESSION.with(|slot| slot.borrow().as_ref().map(|session| session.view.clone()));
    if let Some(view) = view {
        if redo {
            view.ivars().undo.redo();
        } else {
            view.ivars().undo.undo();
        }
        if let Err(error) = layout() {
            emit_error(error);
        }
        publish();
    }
}

pub fn select_all() {
    let view = SESSION.with(|slot| slot.borrow().as_ref().map(|session| session.view.clone()));
    if let Some(view) = view {
        view.setSelectedRange(NSRange::new(0, view.string().length()));
        if let Some(window) = view.window() {
            window.makeFirstResponder(Some(&view));
        }
        publish();
    }
}

// This editor explicitly uses NSLayoutManager/TextKit 1, which supports vertical glyph forms.
#[allow(deprecated)]
fn attributes(
    text: &VectorText,
    style: &TextStyle,
) -> Result<Retained<NSDictionary<NSString, AnyObject>>, String> {
    let mtm = MainThreadMarker::new().ok_or("Text editing requires the main thread")?;
    let family = match style.font_family.as_str() {
        "sans-serif" => "Hiragino Sans",
        "serif" => "Hiragino Mincho ProN",
        "monospace" => "Menlo",
        family => family,
    };
    let mut traits = NSFontTraitMask::empty();
    if style.bold {
        traits |= NSFontTraitMask::BoldFontMask;
    }
    if style.italic {
        traits |= NSFontTraitMask::ItalicFontMask;
    }
    let font = lumapaint_renderer::vector::text_font_postscript_name(style)
        .and_then(|name| {
            NSFont::fontWithName_size(&NSString::from_str(&name), style.font_size.into())
        })
        .or_else(|| {
            NSFontManager::sharedFontManager(mtm).fontWithFamily_traits_weight_size(
                &NSString::from_str(family),
                traits,
                if style.bold { 9 } else { 5 },
                style.font_size.into(),
            )
        })
        .or_else(|| {
            NSFontManager::sharedFontManager(mtm).fontWithFamily_traits_weight_size(
                &NSString::from_str("Arial"),
                traits,
                if style.bold { 9 } else { 5 },
                style.font_size.into(),
            )
        })
        .unwrap_or_else(|| NSFont::systemFontOfSize(style.font_size.into()));
    // TextKit cannot safely lay out nonuniform NSFont matrices (vertical
    // advances can jump to the frame width). Use a uniformly sized font for
    // cross-axis metrics; GPU glyph transforms own the nonuniform shape.
    let cross_scale = if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
        style.scale_x
    } else {
        style.scale_y
    };
    let font = if cross_scale != 1. {
        NSFont::fontWithName_size(&font.fontName(), (style.font_size * cross_scale).into())
            .unwrap_or(font)
    } else {
        font
    };
    let [r, g, b] = style.color;
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        if style.no_color { 0.0 } else { 1.0 },
    );
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(match text.alignment {
        TextAlignment::Left => NSTextAlignment::Left,
        TextAlignment::Center => NSTextAlignment::Center,
        TextAlignment::Right => NSTextAlignment::Right,
        TextAlignment::Justify => NSTextAlignment::Justified,
    });
    paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    // Character size is stored in runs when changed from the Character panel.
    // The object's original size must not keep enlarged line/column spacing
    // after a whole selection is reduced.
    paragraph.setMinimumLineHeight((style.font_size * cross_scale * text.line_height).into());
    // Keep the requested spacing as a minimum, but let larger runs expand
    // their line/column. A fixed maximum causes overlapping mixed-size text.
    paragraph.setMaximumLineHeight(0.0);
    paragraph.setHeadIndent(text.indent_left.into());
    paragraph.setFirstLineHeadIndent((text.indent_left + text.indent_first).into());
    paragraph.setTailIndent((-text.indent_right).into());
    paragraph.setParagraphSpacingBefore(text.space_before.into());
    paragraph.setParagraphSpacing(text.space_after.into());
    paragraph.setHyphenationFactor(if text.hyphenation { 1.0 } else { 0.0 });
    paragraph.setUsesDefaultHyphenation(text.hyphenation);
    paragraph.setLineBreakStrategy(match text.kinsoku {
        KinsokuMode::None => NSLineBreakStrategy::None,
        KinsokuMode::Standard => NSLineBreakStrategy::PushOut,
        KinsokuMode::Strict => NSLineBreakStrategy::Standard,
    });
    paragraph
        .setAllowsDefaultTighteningForTruncation(matches!(text.mojikumi, MojikumiMode::Japanese));
    if !matches!(text.list_style, ParagraphListStyle::None) {
        let marker = match text.list_style {
            ParagraphListStyle::Bullets => unsafe { NSTextListMarkerDisc },
            ParagraphListStyle::Numbers => unsafe { NSTextListMarkerDecimal },
            ParagraphListStyle::None => unreachable!(),
        };
        let list = NSTextList::initWithMarkerFormat_options_startingItemNumber(
            NSTextList::alloc(),
            marker,
            NSTextListOptions::empty(),
            1,
        );
        paragraph.setTextLists(&NSArray::from_slice(&[&*list]));
    }
    let vertical_form = NSNumber::new_i32(1);
    let kern = NSNumber::new_f64((style.tracking * style.font_size / 1000.0).into());
    // A baseline shift is a glyph-only displacement. Let Metal/SVG apply it
    // from STYLE_KEY; giving it to TextKit also expands line/column metrics
    // and moves subsequent, unselected text (and can apply the shift twice).
    let baseline = NSNumber::new_f64(0.0);
    let underline = NSNumber::new_i32(i32::from(style.underline));
    let strike = NSNumber::new_i32(i32::from(style.strikethrough));
    let metadata =
        NSString::from_str(&serde_json::to_string(style).map_err(|error| error.to_string())?);
    let metadata_key = NSString::from_str(STYLE_KEY);
    // SAFETY: AppKit attribute names map to their documented font, color, style and number values.
    unsafe {
        let values: [&AnyObject; 9] = [
            &font,
            &color,
            &paragraph,
            &kern,
            &baseline,
            &underline,
            &strike,
            &metadata,
            &vertical_form,
        ];
        let attributes = NSDictionary::from_slices(
            &[
                NSFontAttributeName,
                NSForegroundColorAttributeName,
                NSParagraphStyleAttributeName,
                NSTrackingAttributeName,
                NSBaselineOffsetAttributeName,
                NSUnderlineStyleAttributeName,
                NSStrikethroughStyleAttributeName,
                &metadata_key,
                objc2_app_kit::NSVerticalGlyphFormAttributeName,
            ],
            &values,
        );
        Ok(attributes)
    }
}

// AppKit owns IME, caret and selection; the GPU draws the actual glyphs in both
// editing and preview modes, avoiding font rasterizer/antialiasing differences.
fn hide_native_glyphs(view: &NSTextView) {
    let clear = NSColor::clearColor();
    view.setTextColor(Some(&clear));
    let background = NSColor::colorWithCalibratedRed_green_blue_alpha(0.25, 0.5, 0.9, 0.25);
    let selected = NSColor::clearColor();
    unsafe {
        let values: [&AnyObject; 2] = [&selected, &background];
        let attributes = NSDictionary::from_slices(
            &[
                NSForegroundColorAttributeName,
                NSBackgroundColorAttributeName,
            ],
            &values,
        );
        view.setSelectedTextAttributes(&attributes);
    }
}

fn live_preview(document: &Document, settings: TextSettings) -> Result<Document, String> {
    if settings.text.content.trim().is_empty() && settings.text.box_height.is_none() {
        return document.text_edit_preview(settings.id.as_deref());
    }
    let mut preview = document.clone();
    preview.set_text_object(settings)?;
    Ok(preview)
}

fn apply_all_attributes() -> Result<(), String> {
    let state = SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| (session.view.clone(), session.settings.clone()))
    });
    let Some((view, settings)) = state else {
        return Ok(());
    };
    apply_attributes_to_view(&view, &settings)?;
    hide_native_glyphs(&view);
    Ok(())
}

fn apply_attributes_to_view(view: &NSTextView, settings: &TextSettings) -> Result<(), String> {
    if let Some(storage) = unsafe { view.textStorage() } {
        let base = attributes(&settings.text, &settings.text.base_style(settings.color))?;
        let runs = settings
            .text
            .runs
            .iter()
            .map(|run| attributes(&settings.text, &run.style).map(|style| (run, style)))
            .collect::<Result<Vec<_>, _>>()?;
        storage.beginEditing();
        unsafe {
            storage.setAttributes_range(Some(&base), NSRange::new(0, storage.length()));
        }
        for (run, style) in runs {
            unsafe {
                storage.setAttributes_range(
                    Some(&style),
                    NSRange::new(run.start, run.end - run.start),
                );
            }
        }
        storage.endEditing();
        apply_kerning_to_view(view, settings);
        apply_character_advances_to_view(view, settings);
        unsafe {
            view.setTypingAttributes(&base);
        }
    }
    Ok(())
}

// Match native caret/selection advances to GPU character transforms, without
// asking TextKit to interpret an anisotropic font matrix.
fn apply_character_advances_to_view(view: &NSTextView, settings: &TextSettings) {
    use lumapaint_core::vector::{KerningMode, WritingMode};
    use unicode_segmentation::UnicodeSegmentation;
    let text = &settings.text;
    let vertical = text.writing_mode == WritingMode::Vertical;
    let scaled = text
        .runs
        .iter()
        .any(|r| r.style.scale_x != 1. || r.style.scale_y != 1.);
    if !(vertical && text.has_vertical_character_options() || scaled) {
        return;
    }
    let (Some(storage), Some(manager), Some(container)) = (
        unsafe { view.textStorage() },
        unsafe { view.layoutManager() },
        unsafe { view.textContainer() },
    ) else {
        return;
    };
    let units = text.vertical_units(settings.color);
    storage.beginEditing();
    unsafe {
        storage.removeAttribute_range(NSKernAttributeName, NSRange::new(0, storage.length()));
    }
    let zero = NSNumber::new_i32(0);
    for (start, content, style) in &units {
        let transformed = style.scale_x != 1. || style.scale_y != 1.;
        let group = vertical && style.tate_chu_yoko;
        let upright = vertical && style.upright_latin(content);
        if !(transformed || group || upright) || content.contains(['\n', '\r']) {
            continue;
        }
        let range = NSRange::new(*start, content.encode_utf16().count());
        if range.location + range.length <= storage.length() {
            let tracking = NSNumber::new_f64((style.tracking * style.font_size / 1000.).into());
            unsafe {
                storage.addAttribute_value_range(NSTrackingAttributeName, &tracking, range);
                storage.addAttribute_value_range(NSLigatureAttributeName, &zero, range);
            }
        }
    }
    storage.endEditing();
    apply_kerning_to_view(view, settings);
    manager.ensureLayoutForTextContainer(&container);
    let mut changes = Vec::new();
    for (start, content, style) in units {
        let transformed = style.scale_x != 1. || style.scale_y != 1.;
        let group = vertical && style.tate_chu_yoko;
        let upright = vertical && style.upright_latin(content);
        if !(transformed || group || upright) || content.contains(['\n', '\r']) {
            continue;
        }
        let count = content.graphemes(true).count().max(1);
        let (inline, cross) = if vertical {
            (style.scale_y, style.scale_x)
        } else {
            (style.scale_x, style.scale_y)
        };
        let tracking = f64::from(style.tracking * style.font_size / 1000.);
        let mut at = start;
        for grapheme in content.graphemes(true) {
            let next_at = at + grapheme.encode_utf16().count();
            let glyph = manager.glyphIndexForCharacterAtIndex(at);
            if glyph >= manager.numberOfGlyphs() {
                at = next_at;
                continue;
            }
            let pen = manager.locationForGlyphAtIndex(glyph).x;
            let mut line_range = NSRange::new(0, 0);
            let used = unsafe {
                manager.lineFragmentUsedRectForGlyphAtIndex_effectiveRange(glyph, &mut line_range)
            };
            let next_glyph = if next_at < storage.length() {
                manager.glyphIndexForCharacterAtIndex(next_at)
            } else {
                manager.numberOfGlyphs()
            };
            let advance = if next_glyph < line_range.location + line_range.length {
                manager.locationForGlyphAtIndex(next_glyph).x - pen
            } else {
                used.size.width - pen
            };
            let kern = unsafe {
                storage.attribute_atIndex_effectiveRange(
                    NSKernAttributeName,
                    at,
                    std::ptr::null_mut(),
                )
            }
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map_or(0., |n| n.doubleValue());
            let desired = if group {
                f64::from(style.font_size * style.scale_y) / count as f64
            } else if upright {
                f64::from(style.font_size * style.scale_y * (1. + style.tracking / 1000.).max(1.))
            } else {
                ((advance - kern - tracking) * f64::from(inline / cross) + kern + tracking).max(0.1)
            };
            let use_kern = group || upright || style.kerning != KerningMode::Metrics;
            let value = if use_kern {
                kern + desired - advance
            } else {
                tracking + desired - advance
            };
            changes.push((
                NSRange::new(at, grapheme.encode_utf16().count()),
                value,
                use_kern,
            ));
            at = next_at;
        }
    }
    storage.beginEditing();
    for (range, value, use_kern) in changes {
        if range.location + range.length <= storage.length() {
            let value = NSNumber::new_f64(value);
            unsafe {
                storage.addAttribute_value_range(
                    if use_kern {
                        NSKernAttributeName
                    } else {
                        NSTrackingAttributeName
                    },
                    &value,
                    range,
                );
            }
        }
    }
    storage.endEditing();
}

fn apply_kerning_to_view(view: &NSTextView, settings: &TextSettings) {
    use lumapaint_core::vector::KerningMode;
    let Some(storage) = (unsafe { view.textStorage() }) else {
        return;
    };
    let text = &settings.text;
    let has_custom = text.kerning != KerningMode::Metrics
        || text
            .runs
            .iter()
            .any(|r| r.style.kerning != KerningMode::Metrics);
    if !has_custom {
        return;
    }
    let adjustments = lumapaint_renderer::vector::kerning::adjustments(text, settings.color);
    storage.beginEditing();
    unsafe {
        storage.removeAttribute_range(NSKernAttributeName, NSRange::new(0, storage.length()));
        storage.removeAttribute_range(NSLigatureAttributeName, NSRange::new(0, storage.length()));
    }
    let zero = NSNumber::new_i32(0);
    let mut offset = 0;
    for c in text.content.chars() {
        let range = NSRange::new(offset, c.len_utf16());
        if text.style_at(offset, settings.color).kerning != KerningMode::Metrics
            && range.location + range.length <= storage.length()
        {
            unsafe {
                storage.addAttribute_value_range(NSLigatureAttributeName, &zero, range);
            }
        }
        offset += c.len_utf16();
    }
    for adjustment in adjustments {
        let range = NSRange::new(adjustment.start, adjustment.length);
        if range.location + range.length <= storage.length()
            && !(adjustment.native_delta.abs() < 0.000001 && adjustment.amount.abs() > 0.000001)
        {
            let value = NSNumber::new_f64(adjustment.native_delta.into());
            unsafe {
                storage.addAttribute_value_range(NSKernAttributeName, &value, range);
            }
        }
    }
    storage.endEditing();
}

pub fn clipboard(action: super::DocumentAction) {
    let view = SESSION.with(|slot| slot.borrow().as_ref().map(|session| session.view.clone()));
    if let Some(view) = view {
        unsafe {
            match action {
                super::DocumentAction::Copy => {
                    let _: () = msg_send![&*view, copy: std::ptr::null::<AnyObject>()];
                }
                super::DocumentAction::Cut => {
                    let _: () = msg_send![&*view, cut: std::ptr::null::<AnyObject>()];
                }
                super::DocumentAction::Paste => view.pasteAsPlainText(None),
                _ => {}
            }
        }
        publish();
    }
}

pub fn reflow_text(text: &mut VectorText, color: [u8; 3]) -> Result<(), String> {
    let mut settings = TextSettings {
        id: Some(String::new()),
        text: text.clone(),
        position: [0., 0.],
        color,
    };
    reflow(&mut settings)?;
    *text = settings.text;
    Ok(())
}

#[cfg(test)]
mod transform_tests {
    use super::*;
    #[test]
    fn point_text_has_no_area_frame() {
        let horizontal = point_text(false);
        let vertical = point_text(true);
        assert!(horizontal.point_text && vertical.point_text);
        assert_eq!(horizontal.box_height, None);
        assert_eq!(vertical.box_height, None);
        assert_eq!(vertical.font_size, horizontal.font_size);
        vertical.validate().unwrap();
    }
    #[test]
    fn selected_character_width_and_height_reflow_without_scaling_the_frame() {
        let mut settings = TextSettings {
            id: None,
            text: VectorText {
                content: "AAAAAA".into(),
                font_size: 24.0,
                box_width: 500.0,
                ..Default::default()
            },
            position: [10.0, 20.0],
            color: [0, 0, 0],
        };
        lumapaint_renderer::vector::reflow_text_with_system_fonts(
            &mut settings.text,
            settings.color,
        )
        .unwrap();
        let before = settings.text.line_widths[0];
        let base = settings.text.base_style(settings.color);
        settings
            .text
            .apply_style(
                2,
                4,
                &TextStylePatch {
                    scale_x: Some(2.0),
                    scale_y: Some(1.5),
                    ..Default::default()
                },
                settings.color,
            )
            .unwrap();
        lumapaint_renderer::vector::reflow_text_with_system_fonts(
            &mut settings.text,
            settings.color,
        )
        .unwrap();
        assert!(settings.text.line_widths[0] > before + 10.0);
        assert_eq!(settings.text.style_at(0, settings.color), base);
        assert_eq!(settings.text.style_at(4, settings.color), base);
        assert_eq!(
            (
                settings.text.scale_x,
                settings.text.scale_y,
                settings.text.box_width
            ),
            (1.0, 1.0, 500.0)
        );
        let mut document = Document::default();
        document.set_text_object(settings).unwrap();
        let layer = document.svg_layers().next().unwrap();
        assert!(layer.source.contains("scale(2 1.5)"));
        let raster = lumapaint_renderer::vector::rasterize_svg(&layer.source, 960, 640).unwrap();
        assert!(raster
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }
    #[test]
    fn retained_preview_skips_unchanged_work_and_retries_failed_generation() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "日本語 English 简体中文".into(),
                box_height: Some(180.),
                ..Default::default()
            },
            position: [40., 50.],
            color: [12, 34, 56],
        })
        .unwrap();
        let before = doc.encode().unwrap();
        let text = doc.snapshot().text_objects.remove(0);
        let mut settings = TextSettings {
            id: Some(text.id),
            text: text.text,
            position: text.position,
            color: text.color,
        };
        let mut cache = PreviewCache::default();
        let first = cache
            .get_or_update(0, || live_preview(&doc, settings.clone()))
            .unwrap()
            .scene_journal()
            .instance_id();
        for _ in 0..100 {
            let reused = cache
                .get_or_update(0, || panic!("pan/selection must reuse preview"))
                .unwrap();
            assert_eq!(reused.scene_journal().instance_id(), first);
        }
        settings.text.content.push('文');
        assert!(cache
            .get_or_update(1, || Err("fixture failure".into()))
            .is_err());
        let updated = cache
            .get_or_update(1, || live_preview(&doc, settings.clone()))
            .unwrap();
        let mut committed = doc.clone();
        committed.set_text_object(settings.clone()).unwrap();
        assert_eq!(
            updated.svg_layers().next().unwrap().source,
            committed.svg_layers().next().unwrap().source
        );
        assert_eq!(doc.encode().unwrap(), before);
        settings.text.content.clear();
        settings.text.box_height = None;
        settings.text.clear_measured_layout();
        let empty = cache
            .get_or_update(2, || live_preview(&doc, settings.clone()))
            .unwrap();
        assert_eq!(
            empty.clone().encode().unwrap(),
            doc.text_edit_preview(settings.id.as_deref())
                .unwrap()
                .encode()
                .unwrap()
        );
    }

    #[test]
    fn new_text_preview_reuses_identity_and_recovers_after_empty_or_invalid_input() {
        let doc = Document::default();
        let original = doc.clone().encode().unwrap();
        let mut settings = TextSettings {
            id: None,
            text: VectorText {
                content: "".into(),
                box_height: None,
                ..Default::default()
            },
            position: [40., 80.],
            color: [10, 20, 30],
        };
        let mut cache = PreviewCache::default();
        cache.update_new(0, &doc, || settings.clone()).unwrap();
        assert!(cache.new_object_id.is_none());
        settings.text.content = "新規 English 简体中文".into();
        cache.update_new(1, &doc, || settings.clone()).unwrap();
        let id = cache.new_object_id.clone().unwrap();
        let instance = cache
            .value
            .as_ref()
            .unwrap()
            .1
            .scene_journal()
            .instance_id();
        for generation in 2..8 {
            settings.text.content.push('文');
            let preview = cache
                .update_new(generation, &doc, || settings.clone())
                .unwrap();
            assert_eq!(preview.scene_journal().instance_id(), instance);
            let fresh = live_preview(&doc, settings.clone()).unwrap();
            assert_eq!(
                preview.svg_layers().next().unwrap().source,
                fresh.svg_layers().next().unwrap().source
            );
        }
        let mut invalid = settings.clone();
        invalid.position[0] = f32::NAN;
        assert!(cache.update_new(8, &doc, || invalid).is_err());
        cache
            .update_new(7, &doc, || panic!("last valid generation"))
            .unwrap();
        settings.text.content.clear();
        let hidden = cache.update_new(8, &doc, || settings.clone()).unwrap();
        assert!(!hidden.svg_layers().next().unwrap().vector_objects[0].visible);
        settings.text.content = "復帰".into();
        let restored = cache.update_new(9, &doc, || settings.clone()).unwrap();
        assert!(restored.svg_layers().next().unwrap().vector_objects[0].visible);
        assert_eq!(restored.scene_journal().instance_id(), instance);
        assert_eq!(cache.new_object_id.as_deref(), Some(id.as_str()));
        assert_eq!(doc.clone().encode().unwrap(), original);
    }

    #[test]
    fn retained_editor_updates_keep_document_identity_across_content_changes() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "初期".into(),
                ..Default::default()
            },
            position: [10., 20.],
            color: [0, 0, 0],
        })
        .unwrap();
        let text = doc.snapshot().text_objects.remove(0);
        let mut settings = TextSettings {
            id: Some(text.id),
            text: text.text,
            position: text.position,
            color: text.color,
        };
        let mut cache = PreviewCache::default();
        let id = cache
            .update_existing(0, &doc, || settings.clone())
            .unwrap()
            .scene_journal()
            .instance_id();
        for generation in 1..20 {
            settings.text.content = format!("日本語 English 简体中文 {generation}");
            let preview = cache
                .update_existing(generation, &doc, || settings.clone())
                .unwrap();
            assert_eq!(preview.scene_journal().instance_id(), id);
            cache
                .update_existing(generation, &doc, || panic!("unchanged generation"))
                .unwrap();
        }
        let mut invalid = settings.clone();
        invalid.position[0] = f32::NAN;
        assert!(cache.update_existing(20, &doc, || invalid).is_err());
        assert_eq!(
            cache
                .update_existing(19, &doc, || panic!("last valid cache"))
                .unwrap()
                .scene_journal()
                .instance_id(),
            id
        );
        cache.update_existing(20, &doc, || settings).unwrap();
    }

    #[test]
    fn gpu_inline_preview_matches_commit_without_changing_original() {
        for mode in [
            lumapaint_core::vector::WritingMode::Horizontal,
            lumapaint_core::vector::WritingMode::Vertical,
        ] {
            let mut doc = Document::default();
            doc.set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "文字 ABC、っ。".into(),
                    writing_mode: mode,
                    box_height: Some(180.),
                    ..Default::default()
                },
                position: [40., 50.],
                color: [12, 34, 56],
            })
            .unwrap();
            doc.affine_selected_vectors([1.3, 0., 0., 1.8, 5., 9.])
                .unwrap();
            let before = doc.encode().unwrap();
            let snapshot = doc.snapshot().text_objects[0].clone();
            let mut settings = TextSettings {
                id: Some(snapshot.id),
                text: snapshot.text,
                position: snapshot.position,
                color: snapshot.color,
            };
            let preview = live_preview(&doc, settings.clone()).unwrap();
            assert_eq!(
                preview.svg_layers().next().unwrap().source,
                doc.svg_layers().next().unwrap().source
            );
            settings.text.content.push('文');
            let preview = live_preview(&doc, settings.clone()).unwrap();
            assert_eq!(doc.encode().unwrap(), before);
            doc.set_text_object(settings.clone()).unwrap();
            assert_eq!(
                preview.svg_layers().next().unwrap().source,
                doc.svg_layers().next().unwrap().source
            );
            settings.text.content.clear();
            settings.text.box_height = None;
            settings.text.clear_measured_layout();
            assert!(live_preview(&doc, settings).is_ok());
        }
    }

    #[test]
    fn editor_workspace_clip_stays_inside_canvas_and_preserves_page_coordinates() {
        let viewport = NSSize::new(800.0, 600.0);
        for origin in [
            NSPoint::new(100.0, 50.0),
            NSPoint::new(-200.0, -100.0),
            NSPoint::new(900.0, 700.0),
        ] {
            let page = NSRect::new(origin, NSSize::new(1600.0, 1200.0));
            let (clip, offset) = workspace_editor_rect(page, viewport);
            assert_eq!(clip, NSRect::new(NSPoint::new(0.0, 0.0), viewport));
            assert_eq!(clip.origin.x + offset.x + 123.0, page.origin.x + 123.0);
            assert_eq!(clip.origin.y + offset.y + 234.0, page.origin.y + 234.0);
        }
    }

    #[test]
    fn vertical_container_uses_physical_height_for_column_length() {
        assert_eq!(
            vertical_container_size(260., 430.),
            NSSize::new(430., 100_000.)
        );
        assert_eq!(
            vertical_container_size(430., 260.),
            NSSize::new(260., 100_000.)
        );
        assert_eq!(
            vertical_container_size(300., 300.),
            NSSize::new(300., 100_000.)
        );
    }

    #[test]
    fn editing_resized_text_keeps_font_units_and_object_transform() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Original".into(),
                font_size: 15.,
                box_width: 180.,
                box_height: Some(80.),
                ..Default::default()
            },
            position: [40., 50.],
            color: [0, 0, 0],
        })
        .unwrap();
        doc.affine_selected_vectors([2., 0., 0., 3., -40., -100.])
            .unwrap();
        let original = doc.svg_layers().next().unwrap().vector_objects[0].clone();
        let snapshot = doc.snapshot().text_objects[0].clone();
        let mut settings = TextSettings {
            id: Some(snapshot.id),
            text: snapshot.text,
            position: snapshot.position,
            color: snapshot.color,
        };
        settings.text.content = "Edited".into();
        doc.set_text_object(settings).unwrap();
        let edited = &doc.svg_layers().next().unwrap().vector_objects[0];
        assert_eq!(edited.transform, original.transform);
        assert_eq!(edited.text.as_ref().unwrap().font_size, 15.);
        assert_eq!(
            editor_display_transform(edited.text.as_ref().unwrap(), edited.transform),
            (2., 3., 0.)
        );
        doc.undo();
        assert_eq!(doc.svg_layers().next().unwrap().vector_objects[0], original);
    }

    #[test]
    fn inline_editor_preserves_bounding_box_scale_and_rotation() {
        let text = VectorText {
            scale_x: 1.2,
            scale_y: 0.8,
            ..Default::default()
        };
        let (x, y, angle) = editor_display_transform(&text, [0., 2., -3., 0., 40., 50.]);
        assert!((x - 2.4).abs() < 1e-5);
        assert!((y - 2.4).abs() < 1e-5);
        assert!((angle - 90.).abs() < 1e-5);
        let rotated = VectorText {
            rotation: 30.,
            ..text.clone()
        };
        let (x, y, angle) = editor_display_transform(&rotated, [2., 0., 0., 2., 0., 0.]);
        assert!((x - 2.4).abs() < 1e-5);
        assert!((y - 1.6).abs() < 1e-5);
        assert!((angle - 30.).abs() < 1e-5);
        assert_eq!(text.scale_x, 1.2);
    }
}

#[derive(Default)]
pub(super) struct WindowContext {
    session: Option<Session>,
    text_frame_pending: bool,
}
impl WindowContext {
    pub(super) fn active(&self) -> bool {
        self.session.is_some()
    }
    pub(super) fn exchange(&mut self) {
        SESSION.with(|slot| std::mem::swap(&mut self.session, &mut *slot.borrow_mut()));
        TEXT_FRAME_PENDING
            .with(|slot| self.text_frame_pending = slot.replace(self.text_frame_pending));
    }
}
