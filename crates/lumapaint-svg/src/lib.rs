//! SVG geometry adapter. Parser-specific trees and caches stay outside the document model.
mod edit;
use lumapaint_core::{
    document::Document,
    svg_backend::{SvgEdit, SvgGeometryBackend},
    vector::VectorObject,
};
use std::sync::Arc;

/// Resolve and edit static SVG using the same parser as the SVG renderer.
#[derive(Default)]
pub struct UsvgGeometryBackend;
impl SvgGeometryBackend for UsvgGeometryBackend {
    fn objects(&self, source: &str, layer_id: &str, size: [f32; 2]) -> Vec<VectorObject> {
        edit::targets(source, layer_id, size)
            .into_iter()
            .map(|t| t.object)
            .collect()
    }
    fn apply(
        &self,
        source: &str,
        layer_id: &str,
        size: [f32; 2],
        changes: &[SvgEdit],
    ) -> Result<String, String> {
        let mut targets = edit::targets(source, layer_id, size);
        let mut seen = std::collections::HashSet::new();
        let mut edits = Vec::new();
        for change in changes {
            if !seen.insert(&change.object_id) {
                return Err("Duplicate SVG edit target".into());
            }
            let index = targets
                .iter()
                .position(|t| t.object.id == change.object_id)
                .ok_or("SVG path missing")?;
            if let Some(object) = &change.object {
                if object.id != change.object_id {
                    return Err("SVG edit identity mismatch".into());
                }
                object.validate()?;
            }
            edits.push((targets.remove(index), change.object.clone()));
        }
        let mut output = source.to_owned();
        edit::replace_many(&mut output, edits)?;
        Ok(output)
    }
}

/// Attach the SVG editing service to a runtime document; nothing is added to its saved data.
pub fn attach(document: &mut Document) {
    document.set_svg_geometry_backend(Some(Arc::new(UsvgGeometryBackend)));
}

#[cfg(test)]
mod document_tests;

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn edits_preserve_identity_and_reject_duplicate_or_mismatched_targets() {
        let source = format!(
            r#"<svg width="100" height="100"><rect id="{}" width="20" height="20"/></svg>"#,
            "shape"
        );
        let backend = UsvgGeometryBackend;
        let layer = "l".to_owned();
        let object = backend.objects(&source, &layer, [100., 100.]).remove(0);
        let change = SvgEdit {
            object_id: object.id.clone(),
            object: Some(object),
        };
        let output = backend
            .apply(&source, &layer, [100., 100.], std::slice::from_ref(&change))
            .unwrap();
        assert_eq!(
            backend.objects(&output, &layer, [100., 100.])[0].id,
            change.object_id
        );
        assert!(backend
            .apply(
                &source,
                &layer,
                [100., 100.],
                &[change.clone(), change.clone()]
            )
            .is_err());
        let mut wrong = change;
        wrong.object.as_mut().unwrap().id = "wrong".into();
        assert!(backend
            .apply(&source, &layer, [100., 100.], &[wrong])
            .is_err());
    }
}
