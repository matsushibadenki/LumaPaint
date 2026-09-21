//! Native inline input: IME and selection stay in AppKit, one commit enters document history.
use super::*;
use lumapaint_core::vector::{TextAlignment, VectorText};
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBaselineOffsetAttributeName, NSColor, NSFont, NSFontAttributeName, NSFontManager,
    NSFontTraitMask, NSForegroundColorAttributeName, NSKernAttributeName, NSLineBreakMode,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStrikethroughStyleAttributeName,
    NSTextAlignment, NSTextInputClient, NSTextView, NSUnderlineStyleAttributeName,
};
use objc2_foundation::{NSDictionary, NSNumber, NSRange, NSString};

define_class!(
    #[unsafe(super(NSTextView))]
    #[ivars = ()]
    struct InlineEditor;
    impl InlineEditor {
        #[unsafe(method(didChangeText))]
        fn did_change_text(&self) {
            unsafe { msg_send![super(self), didChangeText] }
            publish();
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let _keep_alive = unsafe { Retained::retain(self as *const Self as *mut Self) };
            if !self.hasMarkedText() && (event.keyCode() == 53 ||
                event.keyCode() == 36 && event.modifierFlags().contains(NSEventModifierFlags::Command)) {
                if let Err(error) = finish(event.keyCode() != 53) { emit_error(error); }
            } else { unsafe { msg_send![super(self), keyDown: event] } }
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
            DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow()))
        }
    })
}

fn current(session: &Session) -> TextSettings {
    let mut settings = session.settings.clone();
    settings.text.content = session.view.string().to_string();
    settings
}

fn publish() {
    let settings = SESSION.with(|slot| slot.borrow().as_ref().map(current));
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "canvas-text-session", settings);
    }
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
    let allocated = InlineEditor::alloc(mtm).set_ivars(());
    let view: Retained<InlineEditor> = unsafe { msg_send![super(allocated), initWithFrame: frame] };
    view.setRichText(false);
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
        container.setLineBreakMode(NSLineBreakMode::ByClipping);
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
        })
    });
    layout()?;
    redraw()?;
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(&view));
    }
    view.setSelectedRange(NSRange::new(0, view.string().length()));
    publish();
    Ok(())
}

pub fn update(settings: TextSettings) -> Result<(), String> {
    let updated = SESSION.with(|slot| -> Result<bool, String> {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return Ok(false);
        };
        if session.settings.id != settings.id {
            return Err("A different text object is being edited".into());
        }
        let mut next = settings;
        next.text.content = session.view.string().to_string();
        // Empty text is allowed as an input draft; finish cancels an empty draft.
        let mut validation = next.text.clone();
        if validation.content.trim().is_empty() {
            validation.content = "Text".into();
        }
        validation.validate()?;
        if next
            .position
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
        {
            return Err("Invalid text position".into());
        }
        session.settings = next;
        Ok(true)
    })?;
    if updated {
        layout()?;
        publish();
    }
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
        let settings = current(session);
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
        session.view.removeFromSuperview();
    }
    publish();
    redraw()?;
    emit_document();
    Ok(())
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
        let mtm = MainThreadMarker::new().ok_or("Text editing requires the main thread")?;
        let family = match text.font_family.as_str() {
            "sans-serif" => "Hiragino Sans",
            "serif" => "Hiragino Mincho ProN",
            "monospace" => "Menlo",
            family => family,
        };
        let mut traits = NSFontTraitMask::empty();
        if text.bold {
            traits |= NSFontTraitMask::BoldFontMask;
        }
        if text.italic {
            traits |= NSFontTraitMask::ItalicFontMask;
        }
        let font = NSFontManager::sharedFontManager(mtm)
            .fontWithFamily_traits_weight_size(
                &NSString::from_str(family),
                traits,
                if text.bold { 9 } else { 5 },
                text.font_size.into(),
            )
            .unwrap_or_else(|| NSFont::systemFontOfSize(text.font_size.into()));
        let [r, g, b] = session.settings.color;
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
        paragraph.setMinimumLineHeight((text.font_size * text.line_height).into());
        paragraph.setMaximumLineHeight((text.font_size * text.line_height).into());
        paragraph.setHeadIndent(text.indent_left.into());
        paragraph.setFirstLineHeadIndent((text.indent_left + text.indent_first).into());
        paragraph.setTailIndent((-text.indent_right).into());
        paragraph.setParagraphSpacingBefore(text.space_before.into());
        paragraph.setParagraphSpacing(text.space_after.into());
        paragraph.setLineBreakMode(NSLineBreakMode::ByClipping);
        let kern = NSNumber::new_f64((text.tracking * text.font_size / 1000.0).into());
        let baseline = NSNumber::new_f64(text.baseline_shift.into());
        let underline = NSNumber::new_i32(i32::from(text.underline));
        let strike = NSNumber::new_i32(i32::from(text.strikethrough));
        // SAFETY: AppKit attribute names map to their documented font, color, style and number values.
        unsafe {
            let values: [&AnyObject; 7] = [
                &font, &color, &paragraph, &kern, &baseline, &underline, &strike,
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
                ],
                &values,
            );
            if let Some(storage) = view.textStorage() {
                storage.setAttributes_range(
                    Some(&attributes),
                    NSRange::new(0, view.string().length()),
                );
            }
            view.setTypingAttributes(&attributes);
        }
        view.setDefaultParagraphStyle(Some(&paragraph));
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
                    let expected =
                        f64::from(text.font_size - text.baseline_shift + text.space_before);
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
