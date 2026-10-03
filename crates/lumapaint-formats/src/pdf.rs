//! Reserved independent PDF adapter. No renderer, Skia, WebView, or UI dependency.
//! Page decoding and editable-vector conversion must be implemented before enabling dialogs.
use crate::{
    io::{ReadDocument, ReadOptions},
    ImportError,
};
pub fn read(_: &[u8], _: ReadOptions) -> Result<ReadDocument, ImportError> {
    Err(ImportError::Unsupported("pdf.import_not_implemented"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pdf_is_not_silently_flattened_or_accepted_as_native_content() {
        assert!(matches!(
            read(b"%PDF-1.7\n", ReadOptions::default()),
            Err(ImportError::Unsupported("pdf.import_not_implemented"))
        ));
    }
}
