use super::{spatial::SpatialIndex, Journal, JournalRead};
use crate::document::SvgLayer;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
pub(crate) struct PickingState(std::sync::Mutex<PickingCache>);
impl Clone for PickingState {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl PickingState {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, PickingCache> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

#[derive(Clone, Default)]
pub(crate) struct PickingCache {
    layers: BTreeMap<String, LayerIndex>,
}
#[derive(Clone)]
struct LayerIndex {
    tree: SpatialIndex,
    positions: HashMap<String, usize>,
    cursor: u64,
    revision: u64,
    source_pointer: usize,
}
impl LayerIndex {
    fn build(layer: &SvgLayer, journal: &Journal, revision: u64) -> Self {
        Self {
            tree: SpatialIndex::build(
                layer
                    .vector_objects
                    .iter()
                    .enumerate()
                    .map(|(i, o)| (i, o.conservative_drawing_bounds())),
            ),
            positions: layer
                .vector_objects
                .iter()
                .enumerate()
                .map(|(i, o)| (o.id.clone(), i))
                .collect(),
            cursor: journal.cursor(),
            revision,
            source_pointer: layer.source.as_ptr() as usize,
        }
    }
    fn refresh(&mut self, layer: &SvgLayer, journal: &Journal, revision: u64) -> usize {
        let pointer = layer.source.as_ptr() as usize;
        if self.revision == revision
            && self.source_pointer == pointer
            && self.cursor == journal.cursor()
        {
            return 0;
        }
        let JournalRead::Incremental { changes, .. } = journal.read(self.cursor) else {
            *self = Self::build(layer, journal, revision);
            return layer.vector_objects.len();
        };
        let changes: Vec<_> = changes
            .iter()
            .filter(|c| c.target.layer == layer.id)
            .collect();
        let object_changes: Vec<_> = changes
            .iter()
            .filter(|c| c.target.object.is_some())
            .collect();
        // Legacy/preview changes without a notification remain correct. Unknown
        // source edits use the complete rebuild instead of trusting stale bounds.
        if self.positions.len() != layer.vector_objects.len()
            || (pointer != self.source_pointer && object_changes.is_empty())
            || object_changes
                .iter()
                .any(|c| c.removed || c.changes.structure)
        {
            *self = Self::build(layer, journal, revision);
            return layer.vector_objects.len();
        }
        for change in object_changes {
            if !(change.changes.geometry || change.changes.transform || change.changes.style) {
                continue;
            }
            let id = change.target.object.as_ref().unwrap();
            let Some(&position) = self.positions.get(id) else {
                *self = Self::build(layer, journal, revision);
                return layer.vector_objects.len();
            };
            if layer.vector_objects[position].id != *id
                || !self.tree.refit(
                    position,
                    layer.vector_objects[position].conservative_drawing_bounds(),
                )
            {
                *self = Self::build(layer, journal, revision);
                return layer.vector_objects.len();
            }
        }
        self.cursor = journal.cursor();
        self.revision = revision;
        self.source_pointer = pointer;
        0
    }
}
impl PickingCache {
    pub(crate) fn positions(
        &mut self,
        layer: &SvgLayer,
        journal: &Journal,
        revision: u64,
        ids: &[String],
    ) -> (Vec<usize>, usize) {
        let mut scanned = 0;
        let index = self.layers.entry(layer.id.clone()).or_insert_with(|| {
            scanned = layer.vector_objects.len();
            LayerIndex::build(layer, journal, revision)
        });
        scanned += index.refresh(layer, journal, revision);
        let mut positions: Vec<_> = ids
            .iter()
            .filter_map(|id| index.positions.get(id).copied())
            .collect();
        positions.sort_unstable();
        positions.dedup();
        (positions, scanned)
    }
    pub(crate) fn candidates(
        &mut self,
        layer: &SvgLayer,
        journal: &Journal,
        revision: u64,
        bounds: [f64; 4],
    ) -> Vec<usize> {
        let index = self
            .layers
            .entry(layer.id.clone())
            .or_insert_with(|| LayerIndex::build(layer, journal, revision));
        index.refresh(layer, journal, revision);
        index.tree.query(bounds).items
    }
    pub(crate) fn retain(&mut self, layers: &[SvgLayer]) {
        self.layers
            .retain(|id, _| layers.iter().any(|l| &l.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Changes, Target};

    #[test]
    fn notified_bounds_membership_changes_do_not_rebuild_picking() {
        let mut layer: SvgLayer = serde_json::from_value(serde_json::json!({
            "id":"layer", "name":"layer", "visible":true, "source":"before",
            "vectorLayer":true, "vectorObjects":[{
                "id":"a","name":"a","visible":true,
                "path":{"data":"M0 0H20V20H0Z","fillRule":"nonZero"},
                "transform":[1.,0.,0.,1.,0.,0.],"fill":{"color":[255,0,0,255]},
                "stroke":null,"strokeWidth":0.,"kind":"rectangle",
                "controlPoints":[[0.,0.],[20.,0.],[20.,20.],[0.,20.]]
            }]
        }))
        .unwrap();
        let mut journal = Journal::default();
        let mut index = LayerIndex::build(&layer, &journal, 0);
        let points = layer.vector_objects[0].control_points.clone();
        let query = [100., 100., 110., 110.];
        assert!(index.tree.query(query).items.is_empty());
        for (revision, unknown) in [(1, true), (2, false), (3, true), (4, false)] {
            layer.vector_objects[0].control_points =
                if unknown { Vec::new() } else { points.clone() };
            layer.source = format!("revision {revision}");
            journal.push(
                Target {
                    layer: layer.id.clone(),
                    object: Some("a".into()),
                },
                Changes {
                    geometry: true,
                    ..Changes::default()
                },
                false,
            );
            assert_eq!(index.refresh(&layer, &journal, revision), 0);
            assert_eq!(
                index.tree.query(query).items,
                if unknown { vec![0] } else { vec![] }
            );
        }
    }
}
