use lumapaint_core::document::animation::{Interpolation, Property};
use serde::{Deserialize, Serialize};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Get,
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
    Load {
        layer: String,
        frame: u32,
    },
    Key {
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
    pub frame: u32,
    pub playing: bool,
    pub preview: bool,
    pub data: Option<Data>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub document_id: u64,
    pub selected_layer: String,
    pub selected_objects: Vec<String>,
    pub fps: u32,
    pub duration: u32,
    pub looping: bool,
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
    pub channels:
        std::collections::BTreeMap<Property, Vec<lumapaint_core::document::animation::Keyframe>>,
    pub motions: Vec<lumapaint_core::motion::Clip>,
}
#[tauri::command]
pub fn motion_catalog() -> &'static lumapaint_core::motion::Catalog {
    lumapaint_core::motion::catalog()
}
#[tauri::command]
pub async fn timeline(window: tauri::WebviewWindow, request: Request) -> Result<Response, String> {
    #[cfg(target_os = "macos")]
    {
        crate::canvas::on_main(window, move || {
            crate::canvas::platform::timeline::command(request)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, request);
        Err("Animation preview requires the native canvas / アニメーションのプレビューにはネイティブキャンバスが必要です / 动画预览需要原生画布".into())
    }
}
