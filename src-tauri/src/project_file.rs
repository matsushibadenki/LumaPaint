//! Bounded reads and same-directory atomic replacement. No path supplied by the WebView.
use lumapaint_core::document::{Document, MAX_FILE_BYTES};
use std::{
    io::{Read, Write},
    path::Path,
};

pub fn read(path: &Path) -> Result<Document, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Document::decode(&bytes)
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid project directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_round_trip_and_invalid_read() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.lumapaint");
        std::fs::write(&path, b"old").unwrap();
        let bytes = Document::default().encode().unwrap();
        write(&path, &bytes).unwrap();
        assert_eq!(read(&path).unwrap().snapshot().stroke_count, 0);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, b"invalid").unwrap();
        assert!(read(&path).is_err());
    }
    #[test]
    fn failed_replacement_keeps_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("folder");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep"), b"data").unwrap();
        assert!(write(&destination, b"new").is_err());
        assert_eq!(std::fs::read(destination.join("keep")).unwrap(), b"data");
    }
}
