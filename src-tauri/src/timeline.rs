use lumapaint_core::document::animation::{Interpolation, Property};
use serde::{Deserialize, Serialize};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Get,
    Refresh,
    Select {
        layer: String,
    },
    Tick,
    Play,
    Pause,
    Stop,
    StopMotion {
        #[serde(rename = "documentId")]
        document_id: u64,
    },
    Seek {
        frame: u32,
    },
    Settings {
        fps: u32,
        duration: u32,
        looping: bool,
    },
    Onion {
        #[serde(rename = "documentId")]
        document_id: u64,
        settings: lumapaint_core::document::animation::OnionSettings,
    },
    ExposureBatch {
        #[serde(rename = "documentId")]
        document_id: u64,
        revision: u64,
        layer: String,
        frames: Vec<u32>,
        edit: lumapaint_core::document::animation::ExposureBatchEdit,
    },
    ExposureGroup {
        #[serde(rename = "documentId")]
        document_id: u64,
        revision: u64,
        edits: Vec<lumapaint_core::document::animation::ExposureTrackEdit>,
    },
    EditExposure {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        from: u32,
        start: u32,
        end: u32,
    },
    AssignCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        frame: u32,
        cel: u32,
    },
    RenameCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        cel: u32,
        name: String,
    },
    CreateCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
    },
    DeleteCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        cel: u32,
    },
    ForkCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        frame: u32,
    },
    RemoveExposure {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        frame: u32,
    },
    Capture {
        layer: String,
        frame: u32,
        blank: bool,
    },
    Duplicate {
        layer: String,
        from: u32,
        frame: u32,
    },
    LoadCel {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        cel: u32,
    },
    Load {
        layer: String,
        frame: u32,
    },
    Key {
        #[serde(default, rename = "documentId")]
        document_id: Option<u64>,
        layer: String,
        frame: u32,
        property: Property,
        value: f32,
        interpolation: Interpolation,
    },
    MoveKey {
        layer: String,
        property: Property,
        from: u32,
        frame: u32,
    },
    Remove {
        layer: String,
        frame: u32,
        property: Option<Property>,
    },
    SetMotion {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        clip: Box<lumapaint_core::motion::Clip>,
    },
    RemoveMotion {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        id: String,
    },
    MoveMotion {
        #[serde(rename = "documentId")]
        document_id: u64,
        layer: String,
        id: String,
        position: usize,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub exposure_selection: Option<(String, Vec<u32>)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_group_selection: Option<Vec<(String, Vec<u32>)>>,
    pub frame: u32,
    pub playing: bool,
    pub preview: bool,
    pub data: Option<Data>,
    /// True means retain the last metadata; false + null means no document.
    pub data_unchanged: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub document_id: u64,
    pub revision: u64,
    pub selected_layer: String,
    pub selected_objects: Vec<String>,
    pub fps: u32,
    pub duration: u32,
    pub looping: bool,
    pub onion: lumapaint_core::document::animation::OnionSettings,
    pub layers: Vec<Layer>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub pixel: bool,
    pub locked: bool,
    pub cels: Vec<u32>,
    pub cel_library: Vec<CelInfo>,
    pub exposures: Vec<ExposureInfo>,
    pub channels:
        std::collections::BTreeMap<Property, Vec<lumapaint_core::document::animation::Keyframe>>,
    pub motions: Vec<lumapaint_core::motion::Clip>,
}
#[tauri::command]
pub fn motion_catalog() -> &'static lumapaint_core::motion::Catalog {
    lumapaint_core::motion::catalog()
}
#[tauri::command]
pub fn motion_fonts() -> Vec<lumapaint_renderer::vector::font_viewer::FontFace> {
    lumapaint_renderer::vector::font_viewer::variable_catalog()
}
#[tauri::command]
pub async fn timeline(window: tauri::WebviewWindow, request: Request) -> Result<Response, String> {
    #[cfg(target_os = "macos")]
    {
        let (response, job) = crate::canvas::on_main(window.clone(), move || {
            let response = crate::canvas::platform::timeline::command(request)?;
            Ok((response, crate::canvas::platform::timeline::onion_job()))
        })
        .await?;
        if let Some(job) = job {
            crate::cel_previews::schedule(window, job);
        }
        Ok(response)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, request);
        Err("Animation preview requires the native canvas / アニメーションのプレビューにはネイティブキャンバスが必要です / 动画预览需要原生画布".into())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThumbnailRequest {
    pub document_id: u64,
    pub cels: Vec<(String, u32)>,
    #[serde(default)]
    pub library: bool,
}
#[tauri::command]
pub async fn timeline_cel_thumbnails(
    window: tauri::WebviewWindow,
    request: ThumbnailRequest,
) -> Result<Vec<crate::cel_previews::Thumbnail>, String> {
    #[cfg(target_os = "macos")]
    {
        let sources = crate::canvas::on_main(window, move || {
            crate::canvas::platform::timeline::thumbnail_sources(request)
        })
        .await?;
        crate::diagnostic_jobs::spawn_blocking(move || crate::cel_previews::thumbnails(sources))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        // The native thumbnail backend is macOS-only; consume the wire fields
        // explicitly so other targets do not report them as unread dead code.
        let _ = (window, request.document_id, request.cels, request.library);
        Err("Cel previews require the native canvas".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CelInfo {
    pub id: u32,
    pub name: String,
    pub uses: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureInfo {
    pub id: String,
    pub start: u32,
    pub end: u32,
    pub cel: u32,
}
