use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{ipc::Channel, Manager};
use tauri_plugin_updater::UpdaterExt;

#[derive(Default)]
pub(crate) struct Updates(AtomicBool);

struct Lease<'a>(&'a AtomicBool);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn text<'a>(locale: &str, en: &'a str, ja: &'a str, zh: &'a str) -> &'a str {
    match locale {
        "ja" => ja,
        "zh-CN" => zh,
        _ => en,
    }
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    window: tauri::WebviewWindow,
    locale: String,
    progress: Channel<String>,
    state: tauri::State<'_, Updates>,
) -> Result<(), String> {
    if !crate::editor_windows::is_editor(window.label()) {
        return Err("Unknown editor window".into());
    }
    if state.0.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let _lease = Lease(&state.0);
    let result = run(window, &locale, progress).await;
    result.map_err(|error| {
        format!(
            "{}\n{error}",
            text(
                &locale,
                "Could not update LumaPaint. Please try again.",
                "LumaPaintを更新できませんでした。もう一度お試しください。",
                "无法更新LumaPaint。请重试。"
            )
        )
    })
}

async fn run(
    window: tauri::WebviewWindow,
    locale: &str,
    progress: Channel<String>,
) -> Result<(), String> {
    let app = window.app_handle().clone();
    let title = text(
        locale,
        "Software Update",
        "ソフトウェアアップデート",
        "软件更新",
    );
    let mut update = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update.as_mut() else {
        rfd::AsyncMessageDialog::new()
            .set_parent(&window)
            .set_title(title)
            .set_description(format!(
                "LumaPaint {}\n{}",
                app.package_info().version,
                text(
                    locale,
                    "You’re up to date.",
                    "最新バージョンです。",
                    "已是最新版本。"
                )
            ))
            .show()
            .await;
        return Ok(());
    };
    let notes: String = update
        .body
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(1200)
        .collect();
    let accepted = rfd::AsyncMessageDialog::new()
        .set_parent(&window)
        .set_title(title)
        .set_description(format!(
            "LumaPaint {} → {}\n\n{}\n\n{}",
            app.package_info().version,
            update.version,
            notes,
            text(
                locale,
                "Download this update?",
                "アップデートをダウンロードしますか？",
                "下载此更新？"
            )
        ))
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        .await
        == rfd::MessageDialogResult::Ok;
    if !accepted {
        return Ok(());
    }
    update.timeout = Some(Duration::from_secs(600));
    let label = text(
        locale,
        "Downloading update",
        "アップデートをダウンロード中",
        "正在下载更新",
    );
    let _ = progress.send(label.into());
    let mut received = 0u64;
    let mut last = 0u64;
    let bytes = update
        .download(
            |count, total| {
                received = received.saturating_add(count as u64);
                let percent = total
                    .filter(|n| *n > 0)
                    .map(|n| received.saturating_mul(100) / n)
                    .unwrap_or(0)
                    .min(100);
                if percent != last {
                    last = percent;
                    let _ = progress.send(format!("{label} · {percent}%"));
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let accepted = rfd::AsyncMessageDialog::new().set_parent(&window).set_title(title)
        .set_description(text(locale,
            "The signed update is ready. Install and restart now? Save all documents first; unsaved work will prevent installation.",
            "署名の検証が完了しました。インストールして再起動しますか？すべての作品を先に保存してください。未保存の変更がある場合は適用しません。",
            "签名已验证。立即安装并重启？请先保存所有文档；如有未保存的更改，将不会安装。"))
        .set_buttons(rfd::MessageButtons::OkCancel).show().await == rfd::MessageDialogResult::Ok;
    if !accepted {
        return Ok(());
    }
    let update = update.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = if crate::modal_windows::focus(&handle, None) || !crate::canvas::ready_for_update() {
            Err("Save all documents and close dialogs first. / すべての作品を保存し、ダイアログを閉じてください。 / 请保存所有文档并关闭对话框。".into())
        } else {
            update.install(bytes).map_err(|e| e.to_string()).map(|()| {
                crate::EXIT.state.store(crate::EXIT_APPROVED, Ordering::Release);
                crate::EXIT.finish(crate::canvas::shutdown);
                handle.request_restart();
            })
        };
        let _ = tx.send(result);
    }).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_gate_releases_after_error_or_cancel() {
        let gate = AtomicBool::new(false);
        assert!(!gate.swap(true, Ordering::AcqRel));
        {
            let _lease = Lease(&gate);
            assert!(gate.swap(true, Ordering::AcqRel));
        }
        assert!(!gate.swap(true, Ordering::AcqRel));
    }
}
