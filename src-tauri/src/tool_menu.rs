//! Borderless icon pickers stay above Metal without AppKit menu chrome.

#[derive(serde::Serialize)]
pub struct ToolMenuResult {
    supported: bool,
    index: Option<usize>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMenuRequest {
    x: f64,
    y: f64,
    selected: usize,
    enabled: Vec<bool>,
    selection_shortcuts: bool,
    labels: Vec<String>,
    images: ToolMenuImages,
}

#[derive(serde::Deserialize)]
struct ToolMenuImages {
    buttons: Vec<[Vec<u8>; 2]>,
}

#[tauri::command]
pub async fn icon_tool_menu(
    window: tauri::WebviewWindow,
    request: ToolMenuRequest,
) -> Result<ToolMenuResult, String> {
    if !(2..=8).contains(&request.enabled.len())
        || request.selected >= request.enabled.len()
        || request.labels.len() != request.enabled.len()
        || request.images.buttons.len() != request.enabled.len()
        || !request.enabled.iter().any(|enabled| *enabled)
        || window.label() != "main"
        || !request.x.is_finite()
        || !request.y.is_finite()
        || request
            .images
            .buttons
            .iter()
            .flatten()
            .any(|bytes| bytes.is_empty() || bytes.len() > 128 * 1024)
    {
        return Err("Invalid tool menu request".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .with_webview(move |webview| {
                let _ = tx.send(macos::show(webview.inner(), request));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            request.selected,
            request.labels,
            request.selection_shortcuts,
        );
        Ok(ToolMenuResult {
            supported: false,
            index: None,
        })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{ToolMenuRequest, ToolMenuResult};
    use objc2::{
        define_class, msg_send, rc::Retained, sel, AnyThread, ClassType, DefinedClass,
        MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSButton, NSButtonType, NSColor, NSEvent, NSEventMask,
        NSEventModifierFlags, NSEventType, NSImage, NSView, NSWindow, NSWindowOrderingMode,
        NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSData, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString,
    };
    use std::{cell::Cell, ffi::c_void};

    define_class!(
        #[unsafe(super(NSWindow))]
        #[ivars = ()]
        struct PickerWindow;
        impl PickerWindow {
            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key(&self) -> bool { true }
            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main(&self) -> bool { false }
        }
    );

    pub struct PickerState {
        finished: Cell<bool>,
        buttons: Vec<Retained<ToolButton>>,
        focused: Cell<usize>,
        chosen: Cell<Option<usize>>,
        enabled: Vec<bool>,
        selection_shortcuts: bool,
    }

    pub struct ButtonImages {
        normal: Retained<NSImage>,
        selected: Retained<NSImage>,
    }

    // NSButton supplies hit testing and accessibility; the toolbar supplies all pixels.
    define_class!(
        #[unsafe(super(NSButton))]
        #[ivars = ButtonImages]
        struct ToolButton;
        impl ToolButton {
            #[unsafe(method(drawRect:))]
            fn draw_button(&self, _dirty: NSRect) {
                let image = if self.state() == 1 { &self.ivars().selected } else { &self.ivars().normal };
                image.drawInRect(self.bounds());
            }
        }
    );

    define_class!(
        #[unsafe(super(NSView))]
        #[ivars = PickerState]
        struct ToolPicker;
        impl ToolPicker {
            #[unsafe(method(acceptsFirstResponder))]
            fn accepts_first_responder(&self) -> bool { true }

            #[unsafe(method(viewDidMoveToWindow))]
            fn moved_to_window(&self) {
                if let Some(window) = self.window() { window.makeFirstResponder(Some(self)); }
            }

            #[unsafe(method(choose:))]
            fn choose(&self, button: &NSButton) {
                self.commit(button.tag() as usize);
            }

            #[unsafe(method(keyDown:))]
            fn key_down(&self, event: &NSEvent) {
                let focused = self.ivars().focused.get();
                match event.keyCode() {
                    53 => self.ivars().finished.set(true),
                    123 | 126 => self.move_focus(focused, -1),
                    124 | 125 => self.move_focus(focused, 1),
                    115 => self.focus(0),
                    119 => self.focus(self.ivars().buttons.len() - 1),
                    36 | 49 | 76 => self.commit(focused),
                    46 if self.ivars().selection_shortcuts && self.ivars().buttons.len() == 2 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) => {
                        self.commit(usize::from(event.modifierFlags().contains(NSEventModifierFlags::Shift)));
                    }
                    _ => unsafe { msg_send![super(self), keyDown: event] },
                }
            }
        }
    );

    impl ToolPicker {
        fn move_focus(&self, focused: usize, direction: isize) {
            let len = self.ivars().buttons.len();
            for step in 1..=len {
                let index = (focused as isize + direction * step as isize).rem_euclid(len as isize)
                    as usize;
                if self.ivars().enabled[index] {
                    self.focus(index);
                    break;
                }
            }
        }

        fn focus(&self, index: usize) {
            if !self.ivars().enabled[index] {
                return;
            }
            self.ivars().focused.set(index);
            for (i, button) in self.ivars().buttons.iter().enumerate() {
                button.setState(isize::from(index == i));
                NSView::setNeedsDisplay(button, true);
            }
        }

        fn commit(&self, index: usize) {
            if !self.ivars().enabled[index] {
                return;
            }
            self.ivars().chosen.set(Some(index));
            self.ivars().finished.set(true);
        }
    }

    fn decode(bytes: &[u8], size: NSSize) -> Result<Retained<NSImage>, String> {
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
            .ok_or("Cannot decode toolbar image")?;
        image.setSize(size);
        Ok(image)
    }

    pub fn show(parent: *mut c_void, request: ToolMenuRequest) -> Result<ToolMenuResult, String> {
        let mtm = MainThreadMarker::new().ok_or("Tool menu requires the main thread")?;
        // SAFETY: Tauri supplies a live WKWebView on the main thread for this callback.
        let parent = unsafe { &*parent.cast::<NSView>() };
        let owner = parent
            .window()
            .ok_or("Tool picker requires a document window")?;
        let previous_responder = owner.firstResponder();
        let make_button = |index: usize| -> Result<Retained<ToolButton>, String> {
            let images = &request.images.buttons[index];
            let normal = decode(&images[0], NSSize::new(36.0, 36.0))?;
            let selected = decode(&images[1], NSSize::new(36.0, 36.0))?;
            let label = NSString::from_str(&request.labels[index]);
            normal.setAccessibilityDescription(Some(&label));
            selected.setAccessibilityDescription(Some(&label));
            let allocated = ToolButton::alloc(mtm).set_ivars(ButtonImages {
                normal: normal.clone(),
                selected,
            });
            // SAFETY: initialize this allocated NSButton once on the main thread.
            let button: Retained<ToolButton> = unsafe {
                msg_send![super(allocated), initWithFrame: NSRect::new(NSPoint::new(index as f64 * 36.0, 0.0), NSSize::new(36.0, 36.0))]
            };
            button.setButtonType(NSButtonType::Toggle);
            button.setEnabled(request.enabled[index]);
            button.setBordered(false);
            button.setTitle(&label);
            button.setImage(Some(&normal));
            button.setToolTip(Some(&label));
            button.setTag(index as isize);
            Ok(button)
        };
        let buttons = (0..request.enabled.len())
            .map(make_button)
            .collect::<Result<Vec<_>, _>>()?;
        let menu_width = buttons.len() as f64 * 36.0;
        let allocated = ToolPicker::alloc(mtm).set_ivars(PickerState {
            finished: Cell::new(false),
            buttons,
            focused: Cell::new(0),
            chosen: Cell::new(None),
            enabled: request.enabled.clone(),
            selection_shortcuts: request.selection_shortcuts,
        });
        // SAFETY: initialize the allocated NSView once; targets stay alive through tracking.
        let picker: Retained<ToolPicker> = unsafe {
            msg_send![super(allocated), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(menu_width, 36.0))]
        };
        for button in &picker.ivars().buttons {
            picker.addSubview(button);
            unsafe {
                button.setTarget(Some(&picker));
                button.setAction(Some(sel!(choose:)));
            }
        }
        picker.focus(if request.enabled[request.selected] {
            request.selected
        } else {
            request
                .enabled
                .iter()
                .position(|enabled| *enabled)
                .unwrap_or(0)
        });
        // Anchor the row's center to the trigger's center, with no menu padding.
        let bounds = parent.bounds();
        let safe = parent.safeAreaInsets();
        let local = NSPoint::new(
            bounds.origin.x + safe.left + request.x,
            if parent.isFlipped() {
                bounds.origin.y + safe.top + request.y
            } else {
                bounds.origin.y + bounds.size.height - safe.top - request.y
            },
        );
        let base = parent.convertPoint_toView(local, None);
        let screen = owner.convertRectToScreen(NSRect::new(base, NSSize::new(0.0, 0.0)));
        let mut origin = NSPoint::new(screen.origin.x, screen.origin.y - 18.0);
        if let Some(display) = owner.screen() {
            let bounds = display.visibleFrame();
            origin.x = origin.x.clamp(
                bounds.origin.x,
                (bounds.origin.x + bounds.size.width - menu_width).max(bounds.origin.x),
            );
            origin.y = origin
                .y
                .clamp(bounds.origin.y, bounds.origin.y + bounds.size.height - 36.0);
        }
        let allocated = PickerWindow::alloc(mtm).set_ivars(());
        // SAFETY: initialize once on the main thread; Rust owns the window's lifetime.
        let window: Retained<PickerWindow> = unsafe {
            msg_send![super(allocated), initWithContentRect: NSRect::new(origin, NSSize::new(menu_width, 36.0)), styleMask: NSWindowStyleMask::Borderless, backing: NSBackingStoreType::Buffered, defer: false]
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setContentView(Some(&picker));
        // SAFETY: both windows are alive on this thread until the child is detached below.
        unsafe {
            owner.addChildWindow_ordered(&window, NSWindowOrderingMode::Above);
        }
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&picker));
        window.display();
        let app = NSApplication::sharedApplication(mtm);
        let original_frame = owner.frame();
        // Track like a menu: consume the outside click so it cannot paint on the canvas.
        // A bounded wait also dismisses promptly when the app deactivates or the owner closes.
        while !picker.ivars().finished.get()
            && owner.isVisible()
            && app.isActive()
            && owner.frame() == original_frame
        {
            app.updateWindows();
            window.displayIfNeeded();
            let deadline = NSDate::dateWithTimeIntervalSinceNow(0.1);
            let event = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&deadline),
                unsafe { NSDefaultRunLoopMode },
                true,
            );
            let Some(event) = event else {
                continue;
            };
            if event.window(mtm).as_deref() != Some(window.as_super())
                && matches!(
                    event.r#type(),
                    NSEventType::LeftMouseDown
                        | NSEventType::RightMouseDown
                        | NSEventType::OtherMouseDown
                        | NSEventType::ScrollWheel
                )
            {
                break;
            }
            app.sendEvent(&event);
        }
        let index = picker.ivars().chosen.get();
        owner.removeChildWindow(&window);
        window.orderOut(None);
        window.setContentView(None);
        if owner.isVisible() && app.isActive() {
            owner.makeKeyWindow();
            owner.makeFirstResponder(previous_responder.as_deref());
        }
        for button in &picker.ivars().buttons {
            unsafe {
                button.setTarget(None);
            }
        }
        Ok(ToolMenuResult {
            supported: true,
            index,
        })
    }
}
