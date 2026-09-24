//! Native inline input: IME and selection stay in AppKit, one commit enters document history.
use super::*;
use lumapaint_core::vector::{
    TextAlignment, TextGlyphCluster, TextRun, TextStyle, TextStylePatch, VectorText,
};
use objc2::{runtime::AnyObject, DefinedClass};
use objc2_app_kit::{
    NSBaselineOffsetAttributeName, NSColor, NSFont, NSFontAttributeName, NSFontManager,
    NSFontTraitMask, NSForegroundColorAttributeName, NSKernAttributeName, NSLineBreakMode,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSSelectionAffinity,
    NSStrikethroughStyleAttributeName, NSTextAlignment, NSTextInputClient, NSTextView,
    NSUnderlineStyleAttributeName,
};
use objc2_foundation::{NSDictionary, NSNumber, NSRange, NSString, NSUndoManager};

struct EditorState {
    undo: Retained<NSUndoManager>,
}

define_class!(
    #[unsafe(super(NSTextView))]
    #[ivars = EditorState]
    struct InlineEditor;
    impl InlineEditor {
        #[unsafe(method_id(undoManager))]
        fn undo_manager(&self) -> Retained<NSUndoManager> { self.ivars().undo.clone() }
        #[unsafe(method(undo:))]
        fn undo_action(&self, _sender: Option<&AnyObject>) { history(false); }
        #[unsafe(method(redo:))]
        fn redo_action(&self, _sender: Option<&AnyObject>) { history(true); }

        #[unsafe(method(didChangeText))]
        fn did_change_text(&self) {
            unsafe { msg_send![super(self), didChangeText] }
            invalidate_current_cache();
            publish();
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let _keep_alive = unsafe { Retained::retain(self as *const Self as *mut Self) };
            if !self.hasMarkedText() && (event.keyCode() == 53 ||
                event.keyCode() == 36 && event.modifierFlags().contains(NSEventModifierFlags::Command)) {
                if let Err(error) = finish(event.keyCode() != 53) { emit_error(error); }
            } else { unsafe { msg_send![super(self), keyDown: event] } publish(); }
        }
        #[unsafe(method(setSelectedRange:affinity:stillSelecting:))]
        fn select_range(&self, range: NSRange, affinity: NSSelectionAffinity, selecting: bool) {
            unsafe { msg_send![super(self), setSelectedRange: range, affinity: affinity, stillSelecting: selecting] }
            if !selecting { publish(); }
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            unsafe { msg_send![super(self), mouseUp: event] }
            publish();
        }
        // Imported rich text may contain attachments or attributes outside our portable model.
        #[unsafe(method(paste:))]
        fn paste_plain(&self, sender: Option<&AnyObject>) {
            unsafe { self.pasteAsPlainText(sender) }
        }
        // Keep Cmd+A/C/V/Z in the text editor instead of the canvas responder.
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }
    }
);

struct Session {
    view: Retained<InlineEditor>,
    settings: TextSettings,
    preview: Document,
    layout_generation: std::cell::Cell<u64>,
    current_cache: RefCell<Option<(u64, TextSettings)>>,
}
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

pub fn font_families() -> Result<Vec<String>, String> {
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
            canvas.renderer.render(canvas.viewport, &session.preview)
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
fn current(session: &Session) -> TextSettings {
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
            y_correction = f64::from(text.font_size + text.space_before) - natural_baseline;
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
                bounds = Some(candidate);
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
            baselines.push(baseline as f32);
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
    // An empty trailing paragraph has no glyph fragment to measure. Keep the
    // legacy line-spacing fallback rather than saving an incomplete layout.
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
        DOCUMENT.with(|doc| doc.borrow().selected_vector_target())?;
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
        container.setWidthTracksTextView(true);
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
fn publish() {
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
        let _ = app.emit_to("main", "canvas-text-session", settings);
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
            text: VectorText {
                content: "Text".into(),
                ..Default::default()
            },
            position: point,
            color: BRUSH.with(|brush| brush.borrow().color),
        }
    } else {
        return Ok(());
    };
    begin(settings)
}

pub fn begin(settings: TextSettings) -> Result<(), String> {
    finish(true)?;
    settings.text.validate()?;
    // Reject an incompatible destination before creating an inline editor. Otherwise
    // the error is deferred until a tool switch commits the text during canvas sync.
    if settings.id.is_none() {
        DOCUMENT.with(|doc| doc.borrow().selected_vector_target())?;
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
    let preview = DOCUMENT.with(|doc| doc.borrow().text_edit_preview(settings.id.as_deref()))?;
    let mtm = MainThreadMarker::new().ok_or("Text editing requires the main thread")?;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 200.0));
    let allocated = InlineEditor::alloc(mtm).set_ivars(EditorState {
        undo: NSUndoManager::new(mtm),
    });
    let view: Retained<InlineEditor> = unsafe { msg_send![super(allocated), initWithFrame: frame] };
    view.setRichText(true);
    view.setImportsGraphics(false);
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
        canvas.view.addSubview(&view);
        Ok(())
    })?;
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            view: view.clone(),
            settings,
            preview,
            layout_generation: std::cell::Cell::new(0),
            current_cache: RefCell::new(None),
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

pub fn update(settings: TextSettings, patch: Option<TextStylePatch>) -> Result<(), String> {
    let prepared = SESSION.with(|slot| -> Result<_, String> {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return Ok(None);
        };
        if session.settings.id != settings.id {
            return Err("A different text object is being edited".into());
        }
        let current = current(session);
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
            session.settings = next;
            session
                .layout_generation
                .set(session.layout_generation.get().wrapping_add(1));
        }
    });
    apply_all_attributes()?;
    let text = SESSION.with(|slot| slot.borrow().as_ref().unwrap().settings.text.clone());
    let attributes = attributes(&text, &typing)?;
    unsafe {
        view.setTypingAttributes(&attributes);
    }
    if patch.is_some() && range.length > 0 {
        view.didChangeText();
    }
    layout()?;
    publish();
    Ok(())
}

pub fn finish(commit: bool) -> Result<(), String> {
    if !active() {
        return Ok(());
    }
    let session = SESSION.with(|slot| -> Result<Option<Session>, String> {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_ref() else {
            return Ok(None);
        };
        let mut settings = current(session);
        let base = settings.text.base_style(settings.color);
        settings.text.runs.retain(|run| run.style != base);
        if commit && !settings.text.content.trim().is_empty() {
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
        session.view.removeFromSuperview();
    }
    publish();
    redraw()?;
    emit_document();
    Ok(())
}

pub fn layout() -> Result<(), String> {
    invalidate_current_cache();
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
        let width = viewport.width as f32 / viewport.scale;
        let height = viewport.height as f32 / viewport.scale;
        let fit = ((width - 48.0) / viewport.document_width)
            .min((height - 48.0) / viewport.document_height)
            .max(0.01)
            * viewport.zoom;
        let x = width * 0.5
            + viewport.pan_x
            + (session.settings.position[0] - viewport.document_width * 0.5) * fit;
        let y = height * 0.5
            + viewport.pan_y
            + (session.settings.position[1] - viewport.document_height * 0.5) * fit;
        let logical_height = ((height - y) / (fit * text.scale_y)).max(text.font_size * 2.0);
        view.setFrameRotation(0.0);
        view.setFrame(NSRect::new(
            NSPoint::new(x.into(), y.into()),
            NSSize::new(
                (text.box_width * fit * text.scale_x).into(),
                (logical_height * fit * text.scale_y).into(),
            ),
        ));
        view.setBoundsSize(NSSize::new(text.box_width.into(), logical_height.into()));
        view.setFrameRotation(f64::from(text.rotation));
        // AppKit centers a font's baseline within the requested line height. SVG uses the
        // explicit baseline font_size + space_before. Align the editor's first baseline
        // without changing glyph attributes, selection, or document coordinates.
        unsafe {
            if let (Some(manager), Some(container)) = (view.layoutManager(), view.textContainer()) {
                manager.ensureLayoutForTextContainer(&container);
                if manager.numberOfGlyphs() > 0 {
                    let fragment = manager
                        .lineFragmentRectForGlyphAtIndex_effectiveRange(0, std::ptr::null_mut());
                    let natural = fragment.origin.y
                        + manager.locationForGlyphAtIndex(0).y
                        + view.textContainerOrigin().y;
                    let expected = f64::from(text.font_size + text.space_before);
                    let correction = (natural - expected) * f64::from(fit * text.scale_y);
                    let angle = f64::from(text.rotation).to_radians();
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
    let font = NSFontManager::sharedFontManager(mtm)
        .fontWithFamily_traits_weight_size(
            &NSString::from_str(family),
            traits,
            if style.bold { 9 } else { 5 },
            style.font_size.into(),
        )
        .or_else(|| {
            NSFontManager::sharedFontManager(mtm).fontWithFamily_traits_weight_size(
                &NSString::from_str("Arial"),
                traits,
                if style.bold { 9 } else { 5 },
                style.font_size.into(),
            )
        })
        .unwrap_or_else(|| NSFont::systemFontOfSize(style.font_size.into()));
    let [r, g, b] = style.color;
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        1.0,
    );
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(match text.alignment {
        TextAlignment::Left => NSTextAlignment::Left,
        TextAlignment::Center => NSTextAlignment::Center,
        TextAlignment::Right => NSTextAlignment::Right,
    });
    paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    paragraph.setMinimumLineHeight((text.font_size * text.line_height).into());
    paragraph.setMaximumLineHeight((text.font_size * text.line_height).into());
    paragraph.setHeadIndent(text.indent_left.into());
    paragraph.setFirstLineHeadIndent((text.indent_left + text.indent_first).into());
    paragraph.setTailIndent((-text.indent_right).into());
    paragraph.setParagraphSpacingBefore(text.space_before.into());
    paragraph.setParagraphSpacing(text.space_after.into());
    let kern = NSNumber::new_f64((style.tracking * style.font_size / 1000.0).into());
    let baseline = NSNumber::new_f64(style.baseline_shift.into());
    let underline = NSNumber::new_i32(i32::from(style.underline));
    let strike = NSNumber::new_i32(i32::from(style.strikethrough));
    let metadata =
        NSString::from_str(&serde_json::to_string(style).map_err(|error| error.to_string())?);
    let metadata_key = NSString::from_str(STYLE_KEY);
    // SAFETY: AppKit attribute names map to their documented font, color, style and number values.
    unsafe {
        let values: [&AnyObject; 8] = [
            &font, &color, &paragraph, &kern, &baseline, &underline, &strike, &metadata,
        ];
        let attributes = NSDictionary::from_slices(
            &[
                NSFontAttributeName,
                NSForegroundColorAttributeName,
                NSParagraphStyleAttributeName,
                NSKernAttributeName,
                NSBaselineOffsetAttributeName,
                NSUnderlineStyleAttributeName,
                NSStrikethroughStyleAttributeName,
                &metadata_key,
            ],
            &values,
        );
        Ok(attributes)
    }
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
    apply_attributes_to_view(&view, &settings)
}

fn apply_attributes_to_view(view: &NSTextView, settings: &TextSettings) -> Result<(), String> {
    if let Some(storage) = unsafe { view.textStorage() } {
        let base = attributes(&settings.text, &settings.text.base_style(settings.color))?;
        unsafe {
            storage.setAttributes_range(Some(&base), NSRange::new(0, storage.length()));
        }
        for run in &settings.text.runs {
            let style = attributes(&settings.text, &run.style)?;
            unsafe {
                storage.setAttributes_range(
                    Some(&style),
                    NSRange::new(run.start, run.end - run.start),
                );
            }
        }
        unsafe {
            view.setTypingAttributes(&base);
        }
    }
    Ok(())
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
