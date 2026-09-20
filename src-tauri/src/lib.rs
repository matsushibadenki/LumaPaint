use std::sync::atomic::{AtomicBool, Ordering};
static CLOSE_APPROVED: AtomicBool = AtomicBool::new(false);
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
            canvas::sync_canvas,
            canvas::edit_document,
            canvas::toggle_layer,
            canvas::set_color_mode,
            canvas::set_bit_depth,
            canvas::set_color_profile,
            canvas::project_action,
            canvas::import_svg_layer,
            canvas::recovery_info,
            canvas::restore_recovery,
            canvas::retry_recovery
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !canvas::confirm_discard() {
                        api.prevent_close();
                        return;
                    }
                    CLOSE_APPROVED.store(true, Ordering::Relaxed);
                    canvas::discard_recovery();
                    canvas::shutdown();
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
                canvas::shutdown();
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !CLOSE_APPROVED.load(Ordering::Relaxed) && !canvas::confirm_discard() {
                    api.prevent_exit();
                } else {
                    canvas::discard_recovery();
                    canvas::shutdown();
                }
            }
        });
}
mod canvas;

#[cfg(any(target_os = "macos", test))]
mod project_file;

#[cfg(any(target_os = "macos", test))]
mod recovery;
