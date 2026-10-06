//! A collection is a font container. Select its face by PDF PostScript names,
//! never by an arbitrary first face when multiple faces are present.
use super::*;
fn base_name(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, base)) if tag.len() == 6 && tag.bytes().all(|c| c.is_ascii_uppercase()) => base,
        _ => name,
    }
}
pub(super) fn face_index(data: &[u8], names: &[&[u8]]) -> Result<u32, ImportError> {
    let Some(count) = ttf_parser::fonts_in_collection(data) else {
        if data.starts_with(b"ttcf") {
            return Err(ImportError::Malformed("pdf.font_collection"));
        }
        return Ok(0);
    };
    if count == 0 || count > 64 {
        return Err(ImportError::LimitExceeded("pdf.font_collection"));
    }
    if count == 1 {
        return Ok(0);
    }
    let names: Vec<_> = names
        .iter()
        .filter_map(|n| std::str::from_utf8(n).ok())
        .map(base_name)
        .collect();
    let mut selected = None;
    for index in 0..count {
        let face = ttf_parser::Face::parse(data, index)
            .map_err(|_| ImportError::Malformed("pdf.font_collection"))?;
        for name in face.names() {
            if name.name_id != ttf_parser::name_id::POST_SCRIPT_NAME {
                continue;
            }
            let name = name.to_string().or_else(|| {
                name.name
                    .iter()
                    .all(u8::is_ascii)
                    .then(|| String::from_utf8(name.name.to_vec()).ok())
                    .flatten()
            });
            if name.as_deref().is_some_and(|n| names.contains(&n)) {
                if selected.is_some_and(|old| old != index) {
                    return Err(ImportError::Unsupported("pdf.font_collection_face"));
                }
                selected = Some(index);
            }
        }
    }
    selected.ok_or(ImportError::Unsupported("pdf.font_collection_face"))
}
#[cfg(test)]
pub(super) fn collection(faces: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"ttcf\0\x01\0\0".to_vec();
    out.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    out.resize(12 + faces.len() * 4, 0);
    for (index, face) in faces.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let offset = out.len();
        out[12 + index * 4..16 + index * 4].copy_from_slice(&(offset as u32).to_be_bytes());
        let mut face = face.clone();
        let count = u16::from_be_bytes(face[4..6].try_into().unwrap());
        for i in 0..usize::from(count) {
            let record = 12 + i * 16;
            let table = u32::from_be_bytes(face[record + 8..record + 12].try_into().unwrap());
            // Collections do not use the standalone font checksumAdjustment.
            if &face[record..record + 4] == b"head" {
                face[table as usize + 8..table as usize + 12].fill(0);
            }
            face[record + 8..record + 12].copy_from_slice(&(table + offset as u32).to_be_bytes());
        }
        out.extend(face);
    }
    out
}
#[cfg(test)]
pub(super) fn rename_and_scale(data: &[u8], name: &str, units: u16) -> Vec<u8> {
    let mut out = data.to_vec();
    let count = u16::from_be_bytes(out[4..6].try_into().unwrap());
    for i in 0..usize::from(count) {
        let r = 12 + i * 16;
        let offset = u32::from_be_bytes(out[r + 8..r + 12].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(out[r + 12..r + 16].try_into().unwrap()) as usize;
        if &out[r..r + 4] == b"head" {
            out[offset + 18..offset + 20].copy_from_slice(&units.to_be_bytes());
            out[offset + 8..offset + 12].fill(0);
        }
        if &out[r..r + 4] == b"name" {
            let n = u16::from_be_bytes(out[offset + 2..offset + 4].try_into().unwrap());
            let strings = usize::from(u16::from_be_bytes(
                out[offset + 4..offset + 6].try_into().unwrap(),
            ));
            for j in 0..usize::from(n) {
                let record = offset + 6 + j * 12;
                let platform = u16::from_be_bytes(out[record..record + 2].try_into().unwrap());
                let id = u16::from_be_bytes(out[record + 6..record + 8].try_into().unwrap());
                if id != 6 {
                    continue;
                }
                let len = usize::from(u16::from_be_bytes(
                    out[record + 8..record + 10].try_into().unwrap(),
                ));
                let pos = offset
                    + strings
                    + usize::from(u16::from_be_bytes(
                        out[record + 10..record + 12].try_into().unwrap(),
                    ));
                let encoded = if platform == 3 {
                    name.encode_utf16()
                        .flat_map(u16::to_be_bytes)
                        .collect::<Vec<_>>()
                } else {
                    name.as_bytes().to_vec()
                };
                assert_eq!(encoded.len(), len);
                out[pos..pos + len].copy_from_slice(&encoded);
            }
        }
        let mut sum = 0u32;
        for chunk in out[offset..offset + length].chunks(4) {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            sum = sum.wrapping_add(u32::from_be_bytes(word));
        }
        out[r + 4..r + 8].copy_from_slice(&sum.to_be_bytes());
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_unique_postscript_face_and_rejects_ambiguity() {
        let font = include_bytes!("../../../tests/fixtures/lp-pdf-test.ttf");
        let second = rename_and_scale(font, "LPPDFBest", 2000);
        let bytes = collection(&[font.to_vec(), second]);
        assert_eq!(face_index(&bytes, &[b"ABCDEF+LPPDFBest"]).unwrap(), 1);
        assert_eq!(face_index(&bytes, &[b"LPPDFTest"]).unwrap(), 0);
        for names in [
            vec![],
            vec![b"Missing".as_slice()],
            vec![b"LPPDFTest".as_slice(), b"LPPDFBest".as_slice()],
        ] {
            assert!(matches!(
                face_index(&bytes, &names),
                Err(ImportError::Unsupported("pdf.font_collection_face"))
            ));
        }
        assert!(face_index(
            &collection(&[font.to_vec(), font.to_vec()]),
            &[b"LPPDFTest"]
        )
        .is_err());
        let mut excessive = bytes;
        excessive[8..12].copy_from_slice(&65u32.to_be_bytes());
        assert!(matches!(
            face_index(&excessive, &[]),
            Err(ImportError::LimitExceeded("pdf.font_collection"))
        ));
        assert!(face_index(b"ttcf", &[]).is_err());
    }
}
