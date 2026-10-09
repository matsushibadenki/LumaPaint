use super::{spatial::SpatialIndex, Journal, JournalRead};
use crate::document::SvgLayer;
use std::collections::{BTreeMap, HashMap, HashSet};

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
    handles: HashMap<String, usize>,
    slot_positions: Vec<Option<usize>>,
    free_slots: Vec<usize>,
    journal_id: u64,
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
            handles: layer
                .vector_objects
                .iter()
                .enumerate()
                .map(|(i, o)| (o.id.clone(), i))
                .collect(),
            slot_positions: (0..layer.vector_objects.len()).map(Some).collect(),
            free_slots: Vec::new(),
            journal_id: journal.instance_id(),
            cursor: journal.cursor(),
            revision,
            source_pointer: layer.source.as_ptr() as usize,
        }
    }
    fn refresh(&mut self, layer: &SvgLayer, journal: &Journal, revision: u64) -> usize {
        if self.journal_id != journal.instance_id() {
            *self = Self::build(layer, journal, revision);
            return layer.vector_objects.len();
        }
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
        let structural = self.positions.len() != layer.vector_objects.len()
            || object_changes
                .iter()
                .any(|c| c.removed || c.changes.structure);
        if pointer != self.source_pointer && object_changes.is_empty() {
            *self = Self::build(layer, journal, revision);
            return layer.vector_objects.len();
        }
        let mut scanned = 0;
        if structural {
            let next: HashMap<_, _> = layer
                .vector_objects
                .iter()
                .enumerate()
                .map(|(position, object)| (object.id.clone(), position))
                .collect();
            let notified: HashSet<_> = object_changes
                .iter()
                .filter(|c| c.removed || c.changes.structure)
                .filter_map(|c| c.target.object.as_deref())
                .collect();
            // Validate before touching the retained index. Missing notifications,
            // duplicate IDs and legacy edits keep the complete rebuild path.
            if next.len() != layer.vector_objects.len()
                || self
                    .handles
                    .keys()
                    .filter(|id| !next.contains_key(*id))
                    .any(|id| !notified.contains(id.as_str()))
                || next
                    .keys()
                    .filter(|id| !self.handles.contains_key(*id))
                    .any(|id| !notified.contains(id.as_str()))
            {
                *self = Self::build(layer, journal, revision);
                return layer.vector_objects.len();
            }
            self.handles.retain(|id, slot| {
                if next.contains_key(id) {
                    return true;
                }
                self.tree.remove(*slot);
                self.slot_positions[*slot] = None;
                self.free_slots.push(*slot);
                false
            });
            for (id, &position) in &next {
                if let Some(&slot) = self.handles.get(id) {
                    self.slot_positions[slot] = Some(position);
                } else {
                    let slot = self.free_slots.pop().unwrap_or_else(|| {
                        self.slot_positions.push(None);
                        self.slot_positions.len() - 1
                    });
                    self.slot_positions[slot] = Some(position);
                    self.tree.upsert(
                        slot,
                        layer.vector_objects[position].conservative_drawing_bounds(),
                    );
                    crate::performance::count("picking_changed_bounds_evaluations", 1);
                    self.handles.insert(id.clone(), slot);
                }
            }
            self.positions = next;
            scanned = layer.vector_objects.len();
            crate::performance::count("picking_structural_position_scans", scanned as u64);
        }
        let mut refreshed = HashSet::new();
        for change in object_changes {
            if !(change.changes.geometry
                || change.changes.transform
                || change.changes.style
                || change.changes.structure)
            {
                continue;
            }
            let id = change.target.object.as_ref().unwrap();
            if !refreshed.insert(id) {
                continue;
            }
            let Some(&position) = self.positions.get(id) else {
                if structural && change.removed {
                    continue;
                }
                *self = Self::build(layer, journal, revision);
                return layer.vector_objects.len();
            };
            crate::performance::count("picking_changed_bounds_evaluations", 1);
            if layer.vector_objects[position].id != *id
                || !self.tree.refit(
                    self.handles[id],
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
        scanned
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
        let mut positions: Vec<_> = index
            .tree
            .query(bounds)
            .items
            .into_iter()
            .filter_map(|slot| index.slot_positions[slot])
            .collect();
        positions.sort_unstable();
        positions
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

    fn layer() -> SvgLayer {
        serde_json::from_value(serde_json::json!({
            "id":"layer","name":"layer","visible":true,"source":"initial",
            "vectorLayer":true,"vectorObjects":[{
                "id":"a","name":"a","visible":true,
                "path":{"data":"M0 0H20V20H0Z","fillRule":"nonZero"},
                "transform":[1.,0.,0.,1.,0.,0.],"fill":{"color":[255,0,0,255]},
                "stroke":null,"strokeWidth":0.,"kind":"rectangle",
                "controlPoints":[[0.,0.],[20.,0.],[20.,20.],[0.,20.]]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn structural_picking_matches_rebuild_through_insert_delete_reorder_and_restore() {
        let mut layer = layer();
        let mut journal = Journal::default();
        let mut cache = PickingCache::default();
        cache.candidates(&layer, &journal, 0, [0.; 4]);
        let original_slot = cache.layers["layer"].handles["a"];
        let mut b = layer.vector_objects[0].clone();
        b.id = "b".into();
        b.transform[4] = 100.;
        let original = layer.vector_objects.clone();
        for (revision, objects) in [
            vec![b.clone(), original[0].clone()],
            vec![original[0].clone(), b.clone()],
            vec![b.clone()],
            vec![b.clone(), original[0].clone()],
            original.clone(),
        ]
        .into_iter()
        .enumerate()
        {
            let before = layer.clone();
            layer.vector_objects = objects;
            layer.source = format!("revision {revision}");
            journal.layers_changed(&[before], &[layer.clone()]);
            for bounds in [[0., 0., 20., 20.], [100., 0., 120., 20.], [f64::NAN; 4]] {
                let expected = SpatialIndex::build(
                    layer
                        .vector_objects
                        .iter()
                        .enumerate()
                        .map(|(i, o)| (i, o.conservative_drawing_bounds())),
                )
                .query(bounds)
                .items;
                assert_eq!(
                    cache.candidates(&layer, &journal, revision as u64 + 1, bounds),
                    expected
                );
            }
            if revision < 2 {
                assert_eq!(cache.layers["layer"].handles["a"], original_slot);
            }
            let ids: Vec<_> = layer.vector_objects.iter().map(|o| o.id.clone()).collect();
            assert_eq!(
                cache
                    .positions(&layer, &journal, revision as u64 + 1, &ids)
                    .0,
                (0..ids.len()).collect::<Vec<_>>()
            );
            assert!(cache.layers["layer"].slot_positions.len() <= 2);
        }
    }

    #[test]
    fn unnotified_structure_and_new_journal_use_safe_rebuild() {
        let mut layer = layer();
        let mut journal = Journal::default();
        let mut index = LayerIndex::build(&layer, &journal, 0);
        journal.push(
            Target {
                layer: layer.id.clone(),
                object: None,
            },
            Changes {
                structure: true,
                ..Changes::default()
            },
            false,
        );
        assert_eq!(
            index.refresh(&layer, &journal, 1),
            0,
            "layer-order changes do not scan object positions"
        );
        let mut b = layer.vector_objects[0].clone();
        b.id = "b".into();
        b.transform[4] = 100.;
        layer.vector_objects.push(b);
        layer.source = "unnotified".into();
        assert_eq!(index.refresh(&layer, &journal, 1), 2);
        assert_eq!(index.tree.query([100., 0., 120., 20.]).items, [1]);
        journal = Journal::default();
        layer.vector_objects[1].transform[4] = 200.;
        assert_eq!(index.refresh(&layer, &journal, 2), 2);
        assert_eq!(index.tree.query([200., 0., 220., 20.]).items, [1]);
    }

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
