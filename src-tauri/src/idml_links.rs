//! Local IDML asset access policy. No networking; raw bytes stay in Rust.
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const MAX_IMAGE: usize = 3 * 1024 * 1024;
fn decode(value: &str) -> Result<String, String> {
    let mut result = Vec::new();
    let b = value.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err("Invalid image URI".into());
            }
            let h = std::str::from_utf8(&b[i + 1..i + 3]).map_err(|_| "Invalid image URI")?;
            result.push(u8::from_str_radix(h, 16).map_err(|_| "Invalid image URI")?);
            i += 3;
        } else {
            result.push(b[i]);
            i += 1;
        }
    }
    let value = String::from_utf8(result).map_err(|_| "Invalid image URI")?;
    if value.contains('\0') {
        return Err("Invalid image URI".into());
    }
    Ok(value)
}
fn asset_path(uri: &str) -> Result<PathBuf, String> {
    if uri.is_empty()
        || uri.len() > 4096
        || uri.contains('?')
        || uri.contains('#')
        || uri.contains('\\')
    {
        return Err("Invalid image URI".into());
    }
    let raw = if let Some(value) = uri.strip_prefix("file:") {
        if let Some(rest) = value.strip_prefix("//") {
            if rest.starts_with('/') {
                rest
            } else if let Some(rest) = rest.strip_prefix("localhost/") {
                return Ok(PathBuf::from(format!("/{}", decode(rest)?)));
            } else {
                return Err("Remote image hosts are unsupported".into());
            }
        } else {
            value
        }
    } else {
        if uri.contains(':') || uri.starts_with("//") {
            return Err("Remote image URLs are unsupported".into());
        }
        uri
    };
    let path = decode(raw)?;
    if path.starts_with("//") || path.contains('\\') {
        return Err("Remote image hosts are unsupported".into());
    }
    Ok(PathBuf::from(path))
}
pub fn read(uri: &str, directory: &Path) -> Result<(Vec<u8>, String), String> {
    let path = asset_path(uri)?;
    let mut candidates = vec![if path.is_absolute() {
        path.clone()
    } else {
        directory.join(&path)
    }];
    if let Some(name) = path.file_name() {
        candidates.push(directory.join("Links").join(name));
        candidates.push(directory.join(name));
    }
    for candidate in candidates {
        let Ok(path) = candidate.canonicalize() else {
            continue;
        };
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_IMAGE as u64) {
            continue;
        }
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > MAX_IMAGE as u64 {
            continue;
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_IMAGE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_IMAGE
            || !(bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[255, 216]))
        {
            continue;
        }
        let source = path
            .to_str()
            .filter(|s| s.len() <= 4096)
            .ok_or("Image path is not UTF-8")?
            .to_owned();
        return Ok((bytes, source));
    }
    Err("IDML linked PNG/JPEG image is missing, unsupported or exceeds 3 MiB".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_file_uris_decode_spaces_unicode_and_percent_once() {
        assert_eq!(
            asset_path("file:///tmp/%E7%94%BB%20a%25.png").unwrap(),
            PathBuf::from("/tmp/画 a%.png")
        );
        assert_eq!(
            asset_path("file:Links/a%20b.png").unwrap(),
            PathBuf::from("Links/a b.png")
        );
        assert_eq!(
            asset_path("file://localhost/tmp/a.png").unwrap(),
            PathBuf::from("/tmp/a.png")
        );
        for uri in [
            "https://example.com/a.png",
            "file://server/a.png",
            "//server/a.png",
            "file:a%00.png",
            "file:a%GG.png",
            "file:a.png?key=x",
        ] {
            assert!(asset_path(uri).is_err(), "{uri}");
        }
    }
    #[test]
    fn stale_absolute_links_use_packaged_links_and_non_images_are_rejected() {
        let dir =
            std::env::temp_dir().join(format!("lumapaint-idml-link-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Links")).unwrap();
        let file = dir.join("Links/画 a.png");
        std::fs::write(&file, b"\x89PNG\r\n\x1a\nfixture").unwrap();
        let (bytes, path) = read("file:/missing/old/%E7%94%BB%20a.png", &dir).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert_eq!(PathBuf::from(path), file.canonicalize().unwrap());
        assert!(read("https://example.com/a.png", &dir).is_err());
        std::fs::write(dir.join("not-image.png"), b"private text").unwrap();
        assert!(read("not-image.png", &dir).is_err());
        let oversized = std::fs::File::create(dir.join("large.png")).unwrap();
        oversized.set_len(MAX_IMAGE as u64 + 1).unwrap();
        assert!(read("large.png", &dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
