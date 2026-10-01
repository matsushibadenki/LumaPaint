//! Backend-neutral contract for editing retained SVG sources.
//! Implementations live outside core and must preserve unrelated artwork.
use crate::vector::VectorObject;

/// One resolved SVG object replacement, or removal when `object` is `None`.
#[derive(Clone)]
pub struct SvgEdit {
    pub object_id: String,
    pub object: Option<VectorObject>,
}

/// An optional runtime service. Only source strings and LumaPaint-owned types cross this boundary.
/// The model owns validation, transactions, history and persistence. The service must not own
/// document state, modify its input, or retain references to a document.
pub trait SvgGeometryBackend: Send + Sync {
    fn objects(&self, source: &str, layer_id: &str, document_size: [f32; 2]) -> Vec<VectorObject>;
    /// Preserve object identities across unrelated removals, without changing the rendered artwork.
    fn stabilize_ids(&self, source: &str) -> Result<String, String> {
        Ok(source.to_owned())
    }
    /// Return a complete replacement source atomically, leaving the original untouched on failure.
    fn apply(
        &self,
        source: &str,
        layer_id: &str,
        document_size: [f32; 2],
        changes: &[SvgEdit],
    ) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bezier, document::Document};
    use std::sync::Arc;

    // A different backend expressed only in core's types proves that no usvg/Skia types are needed.
    struct TestBackend {
        reject: bool,
    }
    impl SvgGeometryBackend for TestBackend {
        fn objects(&self, _: &str, layer: &str, _: [f32; 2]) -> Vec<VectorObject> {
            let object: VectorObject = serde_json::from_value(serde_json::json!({
                "id": format!("{layer}::test"), "name": "Test path", "visible": true,
                "path": {"data": "M10 10L40 10L40 40Z", "fillRule":"nonZero"},
                "transform": [1.,0.,0.,1.,0.,0.], "fill": {"color":[0,0,0,255]},
                "stroke":null,"strokeWidth":0.,"kind":"compound"
            }))
            .unwrap();
            vec![bezier::editable(&object).unwrap()]
        }
        fn apply(
            &self,
            _: &str,
            _: &str,
            _: [f32; 2],
            changes: &[SvgEdit],
        ) -> Result<String, String> {
            if self.reject {
                return Err("Test backend rejected edit".into());
            }
            assert_eq!(changes.len(), 1);
            assert!(changes[0].object.is_some());
            Ok("<svg width=\"100\" height=\"100\"><path d=\"M20 20L40 10L40 40Z\"/></svg>".into())
        }
    }
    fn document() -> Document {
        let mut doc = Document::default();
        doc.import_svg(
            "Retained SVG".into(),
            "<svg width=\"100\" height=\"100\"><path d=\"M10 10L40 10L40 40Z\"/></svg>".into(),
        )
        .unwrap();
        doc
    }
    #[test]
    fn adapters_are_per_document_and_excluded_from_saved_data() {
        let mut doc = document();
        let encoded = doc.encode().unwrap();
        let other = doc.clone();
        assert!(doc.direct_objects().is_empty());
        doc.set_svg_geometry_backend(Some(Arc::new(TestBackend { reject: false })));
        assert_eq!(doc.encode().unwrap(), encoded);
        assert!(!other.has_svg_geometry_backend());
        assert!(doc.clone().has_svg_geometry_backend());
        assert!(!Document::decode(&encoded)
            .unwrap()
            .has_svg_geometry_backend());
        let id = doc.direct_objects()[0].1.id.clone();
        doc.move_vector_controls(&[(id, 0)], [10., 10.], false)
            .unwrap();
        assert_ne!(doc.encode().unwrap(), encoded);
        doc.undo();
        assert_eq!(doc.encode().unwrap(), encoded);
        assert!(doc.has_svg_geometry_backend());
        doc.set_svg_geometry_backend(None);
        assert!(doc.direct_objects().is_empty());
        assert_eq!(doc.encode().unwrap(), encoded);
    }
    #[test]
    fn failed_backend_edit_leaves_model_and_history_unchanged() {
        let mut doc = document();
        doc.set_svg_geometry_backend(Some(Arc::new(TestBackend { reject: true })));
        let id = doc.direct_objects()[0].1.id.clone();
        let original = doc.encode().unwrap();
        let could_undo = doc.snapshot().can_undo;
        assert!(doc
            .move_vector_controls(&[(id, 0)], [10., 10.], false)
            .is_err());
        assert_eq!(doc.encode().unwrap(), original);
        assert_eq!(doc.snapshot().can_undo, could_undo);
    }
    struct InvalidBackend;
    impl SvgGeometryBackend for InvalidBackend {
        fn objects(&self, source: &str, layer: &str, size: [f32; 2]) -> Vec<VectorObject> {
            TestBackend { reject: false }.objects(source, layer, size)
        }
        fn apply(&self, _: &str, _: &str, _: [f32; 2], _: &[SvgEdit]) -> Result<String, String> {
            Ok("<svg><broken>".into())
        }
    }
    #[test]
    fn model_validates_backend_results_before_committing_or_previewing() {
        let mut doc = document();
        doc.set_svg_geometry_backend(Some(Arc::new(InvalidBackend)));
        let (layer, object) = doc.direct_objects().remove(0);
        let original = doc.encode().unwrap();
        assert!(doc.direct_object_preview(&layer, object.clone()).is_err());
        assert_eq!(doc.encode().unwrap(), original);
        assert!(doc
            .move_vector_controls(&[(object.id, 0)], [10., 10.], false)
            .is_err());
        assert_eq!(doc.encode().unwrap(), original);
    }
}
