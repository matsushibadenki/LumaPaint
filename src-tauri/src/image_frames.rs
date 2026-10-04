//! Filesystem and image decoding stay outside the independent document model.
#[cfg(any(target_os = "macos", test))]
use lumapaint_core::image_frame::FrameImage;
#[cfg(any(target_os = "macos", test))]
use std::path::Path;
#[cfg(any(target_os = "macos", test))]
pub fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    });
    format!("{}:{hash:016x}", bytes.len())
}
#[cfg(any(target_os = "macos", test))]
pub fn load(path: &Path) -> Result<FrameImage, String> {
    use base64::Engine;
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 3 * 1024 * 1024 - 1024 {
        return Err("Image exceeds the current 3 MiB limit / 現在の画像上限は約3MiBです / 当前图像上限约为3MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "jpeg"
    } else {
        return Err("Place currently supports PNG and JPEG / 現在、配置はPNG・JPEGに対応しています / 当前置入支持 PNG 和 JPEG".into());
    };
    let info = lumapaint_renderer::vector::imported_raster_info(&bytes, format)?;
    let image = FrameImage {
        source_path: Some(
            std::fs::canonicalize(path)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned(),
        ),
        fingerprint: fingerprint(&bytes),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width: info.width,
        height: info.height,
        encoded_width: info.encoded_width,
        encoded_height: info.encoded_height,
        orientation_transform: info.image_transform().unwrap_or([1., 0., 0., 1., 0., 0.]),
        data_uri: format!(
            "data:image/{};base64,{}",
            format,
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
    };
    Ok(image)
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageLink {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub status: &'static str,
    pub format: &'static str,
    pub bytes: usize,
    pub modified_at: Option<u64>,
}
#[cfg(any(target_os = "macos", test))]
pub fn status(id: String, image: Option<FrameImage>) -> ImageLink {
    let Some(image) = image else {
        return ImageLink {
            id,
            name: String::new(),
            path: None,
            status: "empty",
            format: "",
            bytes: 0,
            modified_at: None,
        };
    };
    let status = match &image.source_path {
        None => "embedded",
        Some(path) => match std::fs::metadata(path) {
            Err(_) => "missing",
            Ok(m) if m.len() > 3 * 1024 * 1024 => "modified",
            Ok(_) => match std::fs::read(path) {
                Ok(bytes) if fingerprint(&bytes) == image.fingerprint => "normal",
                Ok(_) => "modified",
                Err(_) => "missing",
            },
        },
    };
    let format = if image.data_uri.starts_with("data:image/png;") {
        "PNG"
    } else {
        "JPEG"
    };
    let encoded = image.data_uri.split_once(',').map_or("", |(_, data)| data);
    let bytes = (encoded.len() / 4 * 3)
        .saturating_sub(encoded.bytes().rev().take_while(|b| *b == b'=').count());
    let modified_at = image
        .source_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    ImageLink {
        format,
        bytes,
        modified_at,
        id,
        name: image.name,
        path: image.source_path,
        status,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_detects_same_length_changes() {
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
        assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
    }
    #[test]
    fn link_status_tracks_changes_missing_files_and_embedding() {
        use base64::Engine;
        let path = std::env::temp_dir().join(format!(
            "lp-frame-link-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEUlEQVR4nGP4z8DwH4QZYAwAR8oH+WdZbrcAAAAASUVORK5CYII=").unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let image = load(&path).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(status("f".into(), Some(image.clone())).status, "normal");
        let mut modified = bytes.clone();
        modified.push(0);
        std::fs::write(&path, modified).unwrap();
        assert_eq!(status("f".into(), Some(image.clone())).status, "modified");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(status("f".into(), Some(image.clone())).status, "missing");
        let mut embedded = image;
        embedded.source_path = None;
        assert_eq!(status("f".into(), Some(embedded)).status, "embedded");
        assert_eq!(status("f".into(), None).status, "empty");
    }
}
