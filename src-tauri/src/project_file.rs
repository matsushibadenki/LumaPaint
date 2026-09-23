//! Bounded reads and same-directory atomic replacement. No path supplied by the WebView.
use lumapaint_core::document::{Document, MAX_FILE_BYTES};
use lumapaint_core::{tile_container, tiles::TiledRasterState};
use serde::Serialize;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFingerprint {
    length: usize,
    hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectFormat {
    Legacy,
    Tiled,
}

pub enum ProjectData {
    Legacy(Box<Document>),
    Tiled(TiledRasterState),
}

impl ProjectData {
    pub fn format(&self) -> ProjectFormat {
        match self {
            Self::Legacy(_) => ProjectFormat::Legacy,
            Self::Tiled(_) => ProjectFormat::Tiled,
        }
    }
}

pub fn read(path: &Path) -> Result<Document, String> {
    read_with_fingerprint(path).map(|(document, _)| document)
}

pub fn read_with_fingerprint(path: &Path) -> Result<(Document, FileFingerprint), String> {
    let (project, fingerprint) = read_any_with_fingerprint(path)?;
    match project {
        ProjectData::Legacy(document) => Ok((*document, fingerprint)),
        ProjectData::Tiled(_) => Err("This project uses the tiled document format".into()),
    }
}

pub fn read_any_with_fingerprint(path: &Path) -> Result<(ProjectData, FileFingerprint), String> {
    let bytes = read_bytes(path)?;
    let fingerprint = fingerprint_bytes(&bytes);
    let project = if tile_container::is_container(&bytes) {
        ProjectData::Tiled(tile_container::decode(&bytes)?)
    } else {
        ProjectData::Legacy(Box::new(Document::decode(&bytes)?))
    };
    Ok((project, fingerprint))
}

pub fn fingerprint(path: &Path) -> Result<FileFingerprint, String> {
    read_bytes(path).map(|bytes| fingerprint_bytes(&bytes))
}

pub fn format(path: &Path) -> Result<ProjectFormat, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut prefix = Vec::with_capacity(8);
    file.take(8)
        .read_to_end(&mut prefix)
        .map_err(|e| e.to_string())?;
    Ok(if tile_container::is_container(&prefix) {
        ProjectFormat::Tiled
    } else {
        ProjectFormat::Legacy
    })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let limit = MAX_FILE_BYTES.max(tile_container::MAX_TILE_CONTAINER_BYTES);
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Project exceeds the supported size limit".into());
    }
    Ok(bytes)
}

fn fingerprint_bytes(bytes: &[u8]) -> FileFingerprint {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    FileFingerprint {
        length: bytes.len(),
        hash: hasher.finish(),
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let limit = MAX_FILE_BYTES.max(tile_container::MAX_TILE_CONTAINER_BYTES);
    if bytes.len() > limit {
        return Err("Project exceeds the supported size limit".into());
    }
    let parent = path.parent().ok_or("Invalid project directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Read a next-generation tiled project through the same bounded file boundary.
pub fn read_tiled(path: &Path) -> Result<TiledRasterState, String> {
    match read_any_with_fingerprint(path)?.0 {
        ProjectData::Tiled(state) => Ok(state),
        ProjectData::Legacy(_) => Err("This project uses the legacy document format".into()),
    }
}

/// Atomically write a next-generation tiled project beside the destination.
pub fn write_tiled(path: &Path, state: &TiledRasterState) -> Result<(), String> {
    let bytes = tile_container::encode(state)?;
    write(path, &bytes)
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

    #[test]
    fn fingerprint_detects_same_length_external_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.lumapaint");
        std::fs::write(&path, b"first").unwrap();
        let original = fingerprint(&path).unwrap();
        std::fs::write(&path, b"other").unwrap();
        assert_ne!(fingerprint(&path).unwrap(), original);
    }
}
