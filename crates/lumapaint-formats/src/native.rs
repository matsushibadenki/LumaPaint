//! Native v1 JSON envelope. The model validates state, this adapter owns bytes/version/limits.
use lumapaint_core::document::{Document, DocumentState};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct NativeFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    state: DocumentState,
    #[serde(flatten)]
    unknown_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

pub fn encode_state(state: &DocumentState) -> Result<Vec<u8>, String> {
    Document::from_document_state(state.clone())?;
    let bytes = serde_json::to_vec(&NativeFile {
        format: "LumaPaint".into(),
        version: 1,
        state: state.clone(),
        unknown_fields: Default::default(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Project exceeds 64 MiB".into());
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Document, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Project exceeds 64 MiB".into());
    }
    let file: NativeFile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if file.format != "LumaPaint" || file.version != 1 {
        return Err("Unsupported project format or version".into());
    }
    if !file.unknown_fields.is_empty() {
        return Err("Unknown project fields".into());
    }
    Document::from_document_state(file.state)
}

/// Host convenience API. Import this trait to use the former Document codec call syntax.
/// Neither encoding nor decoding marks a document saved or attaches a renderer/runtime service.
pub trait NativeDocumentCodec: Sized {
    fn encode(&mut self) -> Result<Vec<u8>, String>;
    fn decode(bytes: &[u8]) -> Result<Self, String>;
}

impl NativeDocumentCodec for Document {
    fn encode(&mut self) -> Result<Vec<u8>, String> {
        self.finish();
        encode_state(&self.document_state())
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_round_trip_preserves_model_and_excludes_services() {
        let state = Document::default().document_state();
        let bytes = encode_state(&state).unwrap();
        let loaded = decode(&bytes).unwrap();
        assert!(!loaded.has_svg_geometry_backend());
        assert_eq!(encode_state(&loaded.document_state()).unwrap(), bytes);
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::to_value(loaded.document_state()).unwrap()
        );
    }
    #[test]
    fn legacy_defaults_and_invalid_envelopes() {
        let legacy = br#"{"format":"LumaPaint","version":1,"width":960,"height":640,"layerVisible":true,"strokes":[]}"#;
        assert_eq!(decode(legacy).unwrap().dimensions(), (960, 640));
        let original: serde_json::Value = serde_json::from_slice(legacy).unwrap();
        for (key, value) in [
            ("version", 2.into()),
            ("format", "PDF".into()),
            ("unknown", true.into()),
            ("width", 0.into()),
        ] {
            let mut invalid = original.clone();
            invalid[key] = value;
            assert!(decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        assert!(decode(b"[]").is_err());
        for duplicate in ["version", "width"] {
            let mut source = String::from_utf8(legacy.to_vec()).unwrap();
            source.insert_str(1, &format!("\"{duplicate}\":1,"));
            assert!(decode(source.as_bytes()).is_err());
        }
        assert!(decode(&vec![b' '; MAX_FILE_BYTES + 1]).is_err());
    }
}
