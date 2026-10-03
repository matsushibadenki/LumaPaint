mod image_frames;
mod measurement_units;
#[tauri::command]
fn file_format_capabilities() -> &'static [lumapaint_formats::io::FileFormat] {
    lumapaint_formats::io::FILE_FORMATS
}

use std::sync::atomic::{AtomicU8, Ordering};
use tauri::Manager;

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
            app.manage(modal_windows::Modals::default());
            app.manage(measurement_units::MeasurementUnits::load(app.handle()));
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
                if let Some(item) = menu.get(tauri::menu::WINDOW_SUBMENU_ID) {
                    if let Some(windows) = item.as_submenu() {
                        windows.prepend(&MenuItem::with_id(
                            app,
                            "new-editor-window",
                            "New Window",
                            true,
                            Some("CmdOrCtrl+Shift+N"),
                        )?)?;
                    }
                }
                app.set_menu(menu)?;
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "guarded-quit" {
                app.exit(0);
            } else if event.id().as_ref() == "new-editor-window" {
                if let Err(error) = editor_windows::create(app) {
                    eprintln!("LumaPaint window: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            measurement_units::measurement_unit,
            measurement_units::set_measurement_unit,
            runtime_info,
            canvas::edit_pages,
            canvas::edit_guides,
            canvas::edit_layer_groups,
            canvas::ruler_guide,
            canvas::ruler_origin,
            canvas::page_thumbnails,
            modal_windows::open_modal_window,
            modal_windows::modal_context,
            modal_windows::modal_ready,
            modal_windows::modal_busy,
            modal_windows::modal_change,
            modal_windows::close_modal_window,
            editor_windows::new_editor_window,
            tool_menu::icon_tool_menu,
            canvas::sync_canvas,
            canvas::reset_canvas_pan,
            canvas::finish_canvas_path,
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
            canvas::select_layer,
            canvas::upsert_vector_object,
            canvas::set_text_object,
            canvas::text_fonts,
            canvas::begin_text_edit,
            canvas::update_text_edit,
            canvas::set_text_edit_color,
            canvas::finish_text_edit,
            canvas::select_vector_objects,
            canvas::set_vector_stroke_width,
            canvas::set_vector_stroke_style,
            canvas::stroke_preview,
            canvas::direct_control_info,
            canvas::edit_direct_controls,
            canvas::set_vector_object_visibility,
            canvas::reorder_vector_objects,
            canvas::arrange_selected_vectors,
            canvas::select_arrange_layer,
            canvas::combine_selected_vectors,
            canvas::pathfinder_vectors,
            canvas::group_selected_vectors,
            canvas::clipping_path,
            canvas::compound_path,
            canvas::outline_text,
            canvas::saved_path_action,
            canvas::panel_thumbnails,
            canvas::text_writing_mode,
            canvas::outline_view,
            canvas::transform_objects,
            canvas::edit_transform_panel,
            canvas::set_vector_paint,
            canvas::apply_gradient,
            canvas::set_gradient_tool,
            gradient_presets::gradient_library,
            swatches::swatch_library,
            canvas::set_vector_appearance,
            canvas::ungroup_selected_vectors,
            canvas::edit_selected_paths,
            canvas::reorder_layers,
            canvas::set_color_mode,
            canvas::set_bit_depth,
            canvas::set_color_profile,
            canvas::set_document_settings,
            canvas::sample_tool_point,
            canvas::tool_preview,
            canvas::tool_options,
            canvas::set_tool_brush,
            canvas::set_tool_zoom,
            canvas::numeric_tool,
            canvas::project_action,
            file_format_capabilities,
            canvas::import_svg_layer,
            canvas::import_raster_layer,
            canvas::place_image,
            canvas::image_links,
            canvas::manage_image_links,
            canvas::image_frame_action,
            canvas::finish_raster_import,
            canvas::recovery_info,
            canvas::restore_recovery,
            canvas::delete_recovery,
            canvas::delete_all_recoveries,
            canvas::retry_recovery
        ])
        .on_window_event(|window, event| {
            if window.label().starts_with("modal-") {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !modal_windows::can_close(window.app_handle(), window.label()) {
                        api.prevent_close();
                        return;
                    }
                    modal_windows::detach(window.app_handle(), window.label());
                }
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    modal_windows::detach(window.app_handle(), window.label());
                }
                return;
            }
            if !editor_windows::is_editor(window.label()) {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if modal_windows::focus(window.app_handle(), Some(window.label())) {
                    api.prevent_close();
                    return;
                }
                if !canvas::confirm_window(window.label()) {
                    api.prevent_close();
                    return;
                }
                canvas::close_window(window.label());
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                modal_windows::parent_destroyed(window.app_handle(), window.label());
                canvas::close_window(window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build LumaPaint")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                EXIT.finish(canvas::shutdown);
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if modal_windows::focus(app, None) {
                    api.prevent_exit();
                    return;
                }
                if !EXIT.request(canvas::confirm_discard) {
                    api.prevent_exit();
                } else {
                    EXIT.finish(|| {
                        canvas::discard_recoveries();
                        canvas::shutdown();
                    });
                }
            }
        });
}
mod canvas;
mod editor_windows;
mod gradient_presets;
mod modal_windows;
#[cfg(any(target_os = "macos", test))]
mod render_queue;
mod swatches;
mod tool_menu;

#[cfg(any(target_os = "macos", test))]
pub mod project_file;

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
