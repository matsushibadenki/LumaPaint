use std::sync::atomic::{AtomicU8, Ordering};

const EXIT_RUNNING: u8 = 0;
const EXIT_APPROVED: u8 = 1;
const EXIT_FINISHED: u8 = 2;

struct ExitCoordinator {
    state: AtomicU8,
}

impl ExitCoordinator {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EXIT_RUNNING),
        }
    }

    fn request(&self, confirm: impl FnOnce() -> bool) -> bool {
        if self.state.load(Ordering::Acquire) >= EXIT_APPROVED {
            return true;
        }
        if !confirm() {
            return false;
        }
        self.state.store(EXIT_APPROVED, Ordering::Release);
        true
    }

    fn finish(&self, cleanup: impl FnOnce()) {
        if self.state.swap(EXIT_FINISHED, Ordering::AcqRel) != EXIT_FINISHED {
            cleanup();
        }
    }
}

static EXIT: ExitCoordinator = ExitCoordinator::new();
#[tauri::command]
fn runtime_info() -> lumapaint_core::RuntimeInfo {
    lumapaint_core::runtime_info()
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            canvas::initialize(app.handle());
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem as Native, Submenu};
                let menu = Menu::default(app.handle())?;
                // The built-in macOS Quit uses NSApplication.terminate: and bypasses ExitRequested.
                // Replace the application submenu with a guarded Quit command.
                if let Some(old) = menu.items()?.first() {
                    menu.remove(old)?;
                }
                let application = Submenu::with_items(
                    app,
                    "LumaPaint",
                    true,
                    &[
                        &Native::about(app, None, None)?,
                        &Native::separator(app)?,
                        &Native::services(app, None)?,
                        &Native::separator(app)?,
                        &Native::hide(app, None)?,
                        &Native::hide_others(app, None)?,
                        &Native::separator(app)?,
                        &MenuItem::with_id(
                            app,
                            "guarded-quit",
                            "Quit LumaPaint",
                            true,
                            Some("CmdOrCtrl+Q"),
                        )?,
                    ],
                )?;
                menu.prepend(&application)?;
                app.set_menu(menu)?;
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "guarded-quit" {
                app.exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            runtime_info,
            tool_menu::icon_tool_menu,
            canvas::sync_canvas,
            canvas::reset_canvas_pan,
            canvas::document_workspace,
            canvas::new_document,
            canvas::switch_document,
            canvas::close_document,
            canvas::edit_document,
            canvas::toggle_layer,
            canvas::set_layer_settings,
            canvas::delete_layer,
            canvas::add_paint_layer,
            canvas::add_vector_layer,
            canvas::upsert_vector_object,
            canvas::set_text_object,
            canvas::text_fonts,
            canvas::begin_text_edit,
            canvas::update_text_edit,
            canvas::finish_text_edit,
            canvas::select_vector_objects,
            canvas::combine_selected_vectors,
            canvas::reorder_layers,
            canvas::set_color_mode,
            canvas::set_bit_depth,
            canvas::set_color_profile,
            canvas::set_document_settings,
            canvas::project_action,
            canvas::import_svg_layer,
            canvas::recovery_info,
            canvas::restore_recovery,
            canvas::delete_recovery,
            canvas::delete_all_recoveries,
            canvas::retry_recovery
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !EXIT.request(canvas::confirm_discard) {
                        api.prevent_close();
                        return;
                    }
                    EXIT.finish(|| {
                        canvas::discard_recovery();
                        canvas::shutdown();
                    });
                }
            }
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                canvas::destroy();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build LumaPaint")
        .run(|_, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                EXIT.finish(canvas::shutdown);
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !EXIT.request(canvas::confirm_discard) {
                    api.prevent_exit();
                } else {
                    EXIT.finish(|| {
                        canvas::discard_recovery();
                        canvas::shutdown();
                    });
                }
            }
        });
}
mod canvas;
mod tool_menu;

#[cfg(any(target_os = "macos", test))]
mod project_file;

#[cfg(any(target_os = "macos", test))]
mod recovery;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cancelled_exit_can_be_requested_again() {
        let coordinator = ExitCoordinator::new();
        assert!(!coordinator.request(|| false));
        assert!(coordinator.request(|| true));
        assert!(coordinator.request(|| panic!("approved exits do not prompt twice")));
    }

    #[test]
    fn cleanup_runs_once_across_overlapping_exit_paths() {
        let coordinator = ExitCoordinator::new();
        let cleanups = AtomicUsize::new(0);
        assert!(coordinator.request(|| true));
        coordinator.finish(|| {
            cleanups.fetch_add(1, Ordering::Relaxed);
        });
        coordinator.finish(|| {
            cleanups.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(cleanups.load(Ordering::Relaxed), 1);
    }
}
