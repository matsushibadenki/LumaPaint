//! Certify retained object images from document changes, without rebuilding SVG.
use lumapaint_core::{
    document::{Document, SvgLayer},
    scene::{Changes, Generations, JournalRead, Target},
    vector::VectorObject,
};
use std::collections::{HashMap, HashSet};

pub(crate) struct ObjectCacheState {
    journal_id: u64,
    cursor: u64,
    generations: Generations,
    source_key: (usize, usize),
    positions: HashMap<String, usize>,
    certified: bool,
    visible_ids: HashSet<String>,
}
pub(crate) struct TranslationDelta {
    pub offset: [f32; 2],
    pub positions: Vec<usize>,
}
fn generations(document: &Document, layer: &SvgLayer) -> Generations {
    document.scene_journal().generations(&Target {
        layer: layer.id.clone(),
        object: None,
    })
}
fn source_key(layer: &SvgLayer) -> (usize, usize) {
    (layer.source.as_ptr() as usize, layer.source.len())
}

/// Runtime identity of a composed image, never document/save-file state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceIdentity {
    journal_id: u64,
    source_key: (usize, usize),
    generations: Generations,
}

impl SourceIdentity {
    pub fn capture(document: &Document, layer: &SvgLayer) -> Self {
        Self {
            journal_id: document.scene_journal().instance_id(),
            source_key: source_key(layer),
            generations: generations(document, layer),
        }
    }
}
impl ObjectCacheState {
    pub fn new(document: &Document, layer: &SvgLayer, certify: bool) -> Self {
        let (width, height) = document.dimensions();
        Self {
            journal_id: document.scene_journal().instance_id(),
            cursor: document.scene_journal().cursor(),
            generations: generations(document, layer),
            source_key: source_key(layer),
            positions: layer
                .vector_objects
                .iter()
                .enumerate()
                .map(|(i, o)| (o.id.clone(), i))
                .collect(),
            visible_ids: layer
                .vector_objects
                .iter()
                .filter(|o| o.visible)
                .map(|o| o.id.clone())
                .collect(),
            certified: certify
                && layer.vector_layer
                && layer.source
                    == lumapaint_core::document::vector_svg(width, height, &layer.vector_objects),
        }
    }
    pub fn same_journal(&self, document: &Document) -> bool {
        self.journal_id == document.scene_journal().instance_id()
    }
    pub fn certified(&self) -> bool {
        self.certified
    }
    pub fn matches(&self, document: &Document, layer: &SvgLayer) -> bool {
        self.journal_id == document.scene_journal().instance_id()
            && self.source_key == source_key(layer)
            && self.generations == generations(document, layer)
    }
    /// Fixed-slot translations preserve source layout and object identities.
    /// Other source allocations and document forks require the ordinary lookup.
    pub fn selection(&self, document: &Document, layer: &SvgLayer) -> Option<Vec<String>> {
        (self.journal_id == document.scene_journal().instance_id()
            && self.source_key == source_key(layer))
        .then(|| {
            document
                .selected_vector_ids()
                .iter()
                .filter(|id| self.positions.contains_key(*id))
                .cloned()
                .collect()
        })
    }
    /// Visibility is cached only while the layer generations remain unchanged.
    pub fn moves_as_unit(&self, document: &Document, layer: &SvgLayer) -> Option<bool> {
        self.matches(document, layer).then(|| {
            layer.vector_layer
                && layer.visible
                && !layer.locked
                && !self.visible_ids.is_empty()
                && document
                    .selected_vector_ids()
                    .iter()
                    .filter(|id| self.visible_ids.contains(*id))
                    .count()
                    == self.visible_ids.len()
        })
    }
    pub fn observe(&mut self, document: &Document, layer: &SvgLayer) {
        self.journal_id = document.scene_journal().instance_id();
        self.cursor = document.scene_journal().cursor();
        self.generations = generations(document, layer);
        self.source_key = source_key(layer);
    }
    pub fn accept(
        &mut self,
        document: &Document,
        layer: &SvgLayer,
        cached: &mut [VectorObject],
        delta: TranslationDelta,
    ) -> usize {
        let count = delta.positions.len();
        for position in delta.positions {
            cached[position].transform = layer.vector_objects[position].transform;
        }
        self.observe(document, layer);
        count
    }
    pub fn translated(
        &self,
        document: &Document,
        layer: &SvgLayer,
        old: &[VectorObject],
        selected: &[String],
    ) -> Option<TranslationDelta> {
        if !self.certified
            || self.journal_id != document.scene_journal().instance_id()
            || old.len() != layer.vector_objects.len()
            || selected.is_empty()
        {
            return None;
        }
        let JournalRead::Incremental { changes, .. } = document.scene_journal().read(self.cursor)
        else {
            return None;
        };
        let changes: Vec<_> = changes
            .iter()
            .filter(|c| c.target.layer == layer.id)
            .collect();
        if changes.is_empty() || changes.len() % 2 != 0 {
            return None;
        }
        let parent = Changes {
            geometry: true,
            ..Default::default()
        };
        let object = Changes {
            transform: true,
            ..Default::default()
        };
        let mut changed = HashSet::new();
        // Targeted transactions emit one parent notification directly followed
        // by its object's transform notification. Reject unrelated source edits,
        // missing ranges, reorder, text/style changes and non-transform events.
        for pair in changes.as_chunks::<2>().0 {
            if pair[0].removed
                || pair[1].removed
                || pair[0].target.object.is_some()
                || pair[0].changes != parent
                || pair[1].changes != object
                || pair[1].sequence != pair[0].sequence + 1
            {
                return None;
            }
            changed.insert(pair[1].target.object.as_deref()?);
        }
        if changed.len() != selected.len()
            || selected.iter().any(|id| !changed.contains(id.as_str()))
        {
            return None;
        }
        let mut offset = None;
        let mut positions = Vec::with_capacity(selected.len());
        for id in selected {
            let &position = self.positions.get(id)?;
            let before = &old[position];
            let after = &layer.vector_objects[position];
            if before.id != *id
                || after.id != *id
                || !before.visible
                || !after.visible
                || before.transform[..4] != after.transform[..4]
            {
                return None;
            }
            let delta = [
                after.transform[4] - before.transform[4],
                after.transform[5] - before.transform[5],
            ];
            if delta.iter().any(|v| !v.is_finite()) || offset.is_some_and(|old| old != delta) {
                return None;
            }
            offset = Some(delta);
            positions.push(position);
        }
        Some(TranslationDelta {
            offset: offset?,
            positions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::{
        document::{vector_svg, TextSettings},
        vector::VectorText,
    };
    fn fixture(count: usize) -> Document {
        let mut document = Document::default();
        document
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "日本語 English 简体中文".into(),
                    ..Default::default()
                },
                position: [40., 50.],
                color: [0, 0, 0],
            })
            .unwrap();
        let mut state = document.document_state();
        let prototype = state.svg_layers[0].vector_objects[0].clone();
        state.svg_layers[0].vector_objects = (0..count)
            .map(|i| {
                let mut object = prototype.clone();
                object.id = format!("object-{i}");
                object
            })
            .collect();
        state.svg_layers[0].source = vector_svg(
            state.width,
            state.height,
            &state.svg_layers[0].vector_objects,
        );
        let mut document = Document::from_document_state(state).unwrap();
        document
            .select_vector_objects(vec![format!("object-{}", count - 1)])
            .unwrap();
        document
    }
    #[test]
    fn cached_whole_layer_selection_matches_visibility_and_lock_rules() {
        let mut document = fixture(1000);
        let layer = document.svg_layers().next().unwrap();
        let state = ObjectCacheState::new(&document, layer, true);
        assert_eq!(state.moves_as_unit(&document, layer), Some(false));
        let ids = layer.vector_objects.iter().map(|o| o.id.clone()).collect();
        document.select_vector_objects(ids).unwrap();
        let layer = document.svg_layers().next().unwrap();
        assert_eq!(state.moves_as_unit(&document, layer), Some(true));
        let mut hidden = layer.clone();
        hidden.vector_objects[0].visible = false;
        hidden.source = vector_svg(
            document.dimensions().0,
            document.dimensions().1,
            &hidden.vector_objects,
        );
        assert_eq!(state.moves_as_unit(&document, &hidden), None);
        let mut locked = layer.clone();
        locked.locked = true;
        // Use the same source allocation to isolate the lock check.
        let locked_state = ObjectCacheState::new(&document, &locked, true);
        assert_eq!(locked_state.moves_as_unit(&document, &locked), Some(false));
        document.move_selected_vectors(1., 2.).unwrap();
        assert_eq!(
            state.moves_as_unit(&document, document.svg_layers().next().unwrap()),
            None
        );
    }
    #[test]
    fn multilingual_text_translation_tracks_only_changed_transform_and_undo_redo() {
        for count in [1000, 4096] {
            let mut document = fixture(count);
            let layer = document.svg_layers().next().unwrap();
            let mut state = ObjectCacheState::new(&document, layer, true);
            let mut cached = layer.vector_objects.clone();
            let text_pointer = cached[count - 1].text.as_ref().unwrap().content.as_ptr();
            let path_pointer = cached[count - 1].path.data.as_ptr();
            let selected = document.selected_vector_ids().to_vec();
            assert!(state.matches(&document, layer));
            assert!(state
                .translated(&document, layer, &cached, &selected)
                .is_none());
            for action in 0..3 {
                match action {
                    0 => {
                        document.move_selected_vectors(12., -4.).unwrap();
                    }
                    1 => document.undo(),
                    _ => document.redo(),
                }
                let layer = document.svg_layers().next().unwrap();
                assert!(!state.matches(&document, layer));
                let delta = state
                    .translated(&document, layer, &cached, &selected)
                    .unwrap();
                assert_eq!(delta.positions, [count - 1]);
                assert_eq!(
                    delta.offset,
                    if action == 1 { [-12., 4.] } else { [12., -4.] }
                );
                assert_eq!(state.accept(&document, layer, &mut cached, delta), 1);
                assert!(state.matches(&document, layer));
                assert_eq!(
                    cached[count - 1].text.as_ref().unwrap().content.as_ptr(),
                    text_pointer
                );
                assert_eq!(cached[count - 1].path.data.as_ptr(), path_pointer);
            }
        }
    }
    #[test]
    fn retained_text_images_reject_font_shape_selection_and_foreign_source_changes() {
        let mut document = fixture(2);
        let layer = document.svg_layers().next().unwrap();
        let state = ObjectCacheState::new(&document, layer, true);
        let cached = layer.vector_objects.clone();
        let id = layer.id.clone();
        let selected = document.selected_vector_ids().to_vec();
        let mut changed = cached[1].clone();
        changed.text.as_mut().unwrap().font_size = 80.;
        document.upsert_vector_object(&id, changed).unwrap();
        assert!(state
            .translated(
                &document,
                document.svg_layers().next().unwrap(),
                &cached,
                &selected
            )
            .is_none());
        let mut document = fixture(2);
        let layer = document.svg_layers().next().unwrap();
        let state = ObjectCacheState::new(&document, layer, true);
        let cached = layer.vector_objects.clone();
        document.move_selected_vectors(10., 5.).unwrap();
        let mut layer = document.svg_layers().next().unwrap().clone();
        layer.vector_objects[1].transform[0] = 2.;
        assert!(state
            .translated(&document, &layer, &cached, &selected)
            .is_none());
        assert!(state
            .translated(&document, &layer, &cached, &["object-0".into()])
            .is_none());
        let fork = document.clone();
        assert!(state
            .translated(&fork, fork.svg_layers().next().unwrap(), &cached, &selected)
            .is_none());
        let mut file = document.document_state();
        file.svg_layers[0].source = file.svg_layers[0]
            .source
            .replace("opacity=\"1\"", "opacity=\"0.5\"");
        let mut foreign = Document::from_document_state(file).unwrap();
        foreign.select_vector_objects(selected.clone()).unwrap();
        let foreign_layer = foreign.svg_layers().next().unwrap();
        let state = ObjectCacheState::new(&foreign, foreign_layer, true);
        let cached = foreign_layer.vector_objects.clone();
        assert!(!state.certified());
        foreign.move_selected_vectors(1., 1.).unwrap();
        assert!(state
            .translated(
                &foreign,
                foreign.svg_layers().next().unwrap(),
                &cached,
                &selected
            )
            .is_none());
    }
    #[test]
    fn pending_uniform_moves_accumulate_but_journal_gaps_and_nonuniform_moves_reject() {
        let mut document = fixture(2);
        document
            .select_vector_objects(vec!["object-0".into(), "object-1".into()])
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let state = ObjectCacheState::new(&document, layer, true);
        let cached = layer.vector_objects.clone();
        let selected = document.selected_vector_ids().to_vec();
        document.move_selected_vectors(3., 4.).unwrap();
        document.move_selected_vectors(2., 1.).unwrap();
        let layer = document.svg_layers().next().unwrap();
        assert_eq!(
            state
                .translated(&document, layer, &cached, &selected)
                .unwrap()
                .offset,
            [5., 5.]
        );
        let mut wrong = layer.clone();
        wrong.vector_objects[1].transform[4] += 1.;
        assert!(state
            .translated(&document, &wrong, &cached, &selected)
            .is_none());
        for _ in 0..1100 {
            document.move_selected_vectors(1., 1.).unwrap();
        }
        assert!(state
            .translated(
                &document,
                document.svg_layers().next().unwrap(),
                &cached,
                &selected
            )
            .is_none());
    }
}
