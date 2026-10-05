//! PSD decoding stays on Rust workers; only confirmed native tiles enter the editor.
use lumapaint_core::tiles::TiledRasterDocument;
use lumapaint_formats::{ConversionReport, ImportError};
use std::{
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
#[derive(Default)]
pub(crate) struct Gate(Arc<AtomicBool>);
struct Permit(Arc<AtomicBool>);
impl Gate {
    fn claim(&self) -> Result<Permit, String> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "PSD import is already running / PSDの読み込みを実行中です / PSD导入正在运行"
                    .to_string()
            })?;
        Ok(Permit(self.0.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
pub(crate) struct Prepared {
    pub document: TiledRasterDocument,
    pub report: ConversionReport,
    pub name: String,
}
fn error(error: ImportError) -> String {
    let message = match error {
        ImportError::Unsupported(_) => "This PSD format is unsupported. Currently supported: PSD v1, RGB8 composite images, uncompressed or PackBits. / このPSD形式は未対応です。現在はPSD v1・RGB8統合画像・非圧縮またはPackBitsに対応しています。 / 此PSD格式不受支持。目前支持PSD v1、RGB8合成图像、无压缩或PackBits。",
        ImportError::LimitExceeded(_) => "PSD exceeds the input or canvas limit / PSDが入力容量または用紙寸法の上限を超えています / PSD超出输入或画布限制",
        _ => "PSD is damaged or incomplete / PSDが破損しているか不完全です / PSD已损坏或不完整",
    };
    format!("{message}\n{error}")
}
fn decode(path: &Path) -> Result<Prepared, String> {
    let limit = lumapaint_formats::psd::MAX_INPUT_BYTES;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err(error(ImportError::LimitExceeded("PSD input")));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let decoded = lumapaint_formats::io::read_document(
        lumapaint_formats::export::FormatId::Psd,
        name.clone(),
        &bytes,
        lumapaint_formats::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
    )
    .map_err(error)?;
    drop(bytes);
    let lumapaint_formats::io::ReadContent::Raster(mut state) = decoded.content else {
        return Err("Expected native PSD tiles".into());
    };
    if let Some(layer) = state.layers.first_mut().filter(|l| l.id == "psd-composite") {
        layer.name = name.clone();
    }
    Ok(Prepared {
        document: TiledRasterDocument::from_state(state)?,
        report: decoded.report,
        name,
    })
}
fn report_text(report: &ConversionReport) -> String {
    report.issues.iter().map(|issue| match issue.code {
        "psd.extraChannelsNotPreserved" => "Saved alpha or spot channels are not retained. / 保存されたアルファ・スポットチャンネルは保持されません。 / 保存的Alpha或专色通道不会保留。",
        "psd.maskDensityBaked" => "Mask density is applied to mask pixels; its original numeric setting is not retained. / マスク密度を画素に反映します。元の密度数値は保持されません。 / 蒙版密度将应用到像素，原始密度数值不会保留。",
        "psd.layersFlattened" => "Layers are flattened to the merged image. / レイヤーは統合画像になります。 / 图层将合并为一张图像。",
        "psd.resolutionNotPreserved" => "Original resolution metadata is not retained. / 元の解像度情報は保持されません。 / 原始分辨率元数据不会保留。",
        "psd.iccProfileNotPreserved" => "ICC profiles are not retained; colors may differ. / ICCプロファイルは保持されず、色が異なる場合があります。 / ICC配置文件不会保留，颜色可能不同。",
        _ => "Additional PSD metadata is not retained. / PSDの追加情報は保持されません。 / 其他PSD元数据不会保留。",
    }).collect::<Vec<_>>().join("\n\n")
}
#[cfg(target_os = "macos")]
pub(crate) fn open(
    app: tauri::AppHandle,
    owner: String,
    path: std::path::PathBuf,
) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    let permit = app.state::<Gate>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = decode(&path);
        let callback_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _permit = permit;
            if callback_app.get_webview_window(&owner).is_none() {
                return;
            }
            let Err(error) = (|| -> Result<(), String> {
                let prepared = result?;
                if !prepared.report.issues.is_empty()
                    && rfd::MessageDialog::new()
                        .set_title("互換性 / Compatibility / 兼容性")
                        .set_description(report_text(&prepared.report))
                        .set_buttons(rfd::MessageButtons::OkCancel)
                        .show()
                        != rfd::MessageDialogResult::Ok
                {
                    return Ok(());
                }
                // A dialog may process native events; the owner must still exist at commit time.
                if callback_app.get_webview_window(&owner).is_none() {
                    return Ok(());
                }
                crate::canvas::open_psd(&owner, prepared)
            })() else {
                return;
            };
            let _ = callback_app.emit_to(&owner, "canvas-error", error);
        });
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend(1u16.to_be_bytes());
        bytes.extend([0; 6]);
        bytes.extend(3u16.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(2u32.to_be_bytes());
        bytes.extend(8u16.to_be_bytes());
        bytes.extend(3u16.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(8u32.to_be_bytes());
        bytes.extend([0; 8]);
        bytes.extend(0u16.to_be_bytes());
        bytes.extend([255, 0, 0, 255, 0, 0]);
        bytes
    }
    #[test]
    fn prepared_psd_keeps_native_tiles_name_and_loss_report_through_save_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Test.psd");
        std::fs::write(&path, fixture()).unwrap();
        let prepared = decode(&path).unwrap();
        assert_eq!(prepared.document.dimensions(), (2, 1));
        assert_eq!(prepared.document.layers()[0].name, "Test.psd");
        assert_eq!(prepared.name, "Test.psd");
        assert_eq!(prepared.report.issues[0].code, "psd.layersFlattened");
        for locale in ["Layers", "レイヤー", "图层"] {
            assert!(report_text(&prepared.report).contains(locale));
        }
        let native = directory.path().join("Saved.lumapaint");
        let state = prepared.document.state();
        crate::project_file::write_tiled(&native, &state).unwrap();
        let (crate::project_file::ProjectData::Tiled(saved), _) =
            crate::project_file::read_any_with_fingerprint(&native).unwrap()
        else {
            panic!()
        };
        assert_eq!(saved, state);
        let recovery_dir = directory.path().join("recovery");
        let mut recovery = crate::recovery::Recovery::start(recovery_dir.clone()).unwrap();
        recovery.checkpoint_project(
            crate::project_file::ProjectData::Tiled(state.clone()),
            0,
            true,
        );
        recovery.stop();
        drop(recovery);
        let recovery = crate::recovery::Recovery::start(recovery_dir).unwrap();
        let info = recovery.info().unwrap();
        let crate::project_file::ProjectData::Tiled(saved) = recovery
            .read_candidate_project(&info.candidates[0].id)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(saved, state);
    }
    #[test]
    fn transparent_psd_worker_preserves_alpha_for_the_gpu_composite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Transparent.psd");
        std::fs::write(
            &path,
            include_bytes!("../../crates/lumapaint-formats/tests/fixtures/lp-psd-alpha.psd"),
        )
        .unwrap();
        let prepared = decode(&path).unwrap();
        assert_eq!(prepared.document.dimensions(), (4, 1));
        assert_eq!(
            &prepared
                .document
                .composite_tile(lumapaint_core::tiles::TileCoord { x: 0, y: 0 })
                .unwrap()[..16],
            &[128, 0, 0, 128, 0, 0, 64, 64, 0, 255, 0, 255, 0, 0, 0, 0]
        );
    }
    #[test]
    fn layer_worker_keeps_original_layer_names_and_native_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Layers.psd");
        std::fs::write(
            &path,
            include_bytes!("../../crates/lumapaint-formats/tests/fixtures/lp-psd-layers.psd"),
        )
        .unwrap();
        let prepared = decode(&path).unwrap();
        assert_eq!(prepared.document.layers().len(), 3);
        assert_eq!(prepared.document.layers()[0].name, "Base");
        assert_eq!(prepared.document.layers()[1].name, "日本語");
        assert!(prepared.report.issues.is_empty());
        let native = directory.path().join("Layers.lumapaint");
        let state = prepared.document.state();
        crate::project_file::write_tiled(&native, &state).unwrap();
        let (crate::project_file::ProjectData::Tiled(saved), _) =
            crate::project_file::read_any_with_fingerprint(&native).unwrap()
        else {
            panic!()
        };
        assert_eq!(saved, state);
    }
    #[test]
    fn failed_and_cancelled_imports_release_the_gate() {
        let gate = Gate::default();
        let permit = gate.claim().unwrap();
        assert!(gate.claim().is_err());
        drop(permit);
        assert!(gate.claim().is_ok());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Bad.psd");
        std::fs::write(&path, b"bad").unwrap();
        assert!(decode(&path).is_err());
    }
}
