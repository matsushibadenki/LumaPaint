//! Runtime change notifications. These are derived data, never native-file state.
use crate::{document::SvgLayer, vector::VectorObject};
use std::collections::{BTreeMap, VecDeque};

pub mod pages;
pub(crate) mod picking;
pub mod spatial;

const JOURNAL_CAPACITY: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub layer: String,
    pub object: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Generations {
    pub geometry: u64,
    pub transform: u64,
    pub style: u64,
    pub visibility: u64,
    pub structure: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub geometry: bool,
    pub transform: bool,
    pub style: bool,
    pub visibility: bool,
    pub structure: bool,
}
impl Changes {
    fn all() -> Self {
        Self {
            geometry: true,
            transform: true,
            style: true,
            visibility: true,
            structure: true,
        }
    }
    fn any(self) -> bool {
        self.geometry || self.transform || self.style || self.visibility || self.structure
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub sequence: u64,
    pub target: Target,
    pub changes: Changes,
    pub generations: Generations,
    pub removed: bool,
    pub before_bounds: Option<spatial::Bounds>,
    pub after_bounds: Option<spatial::Bounds>,
}

/// Consumers own their cursor. A missing range requires a complete rebuild.
#[derive(Clone, Debug, PartialEq)]
pub enum JournalRead {
    Incremental { cursor: u64, changes: Vec<Change> },
    Rebuild { cursor: u64 },
}

#[derive(Debug)]
pub struct Journal {
    instance_id: u64,
    sequence: u64,
    generations: BTreeMap<Target, Generations>,
    events: VecDeque<Change>,
}
fn next_journal_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            instance_id: next_journal_id(),
            sequence: 0,
            generations: BTreeMap::new(),
            events: VecDeque::new(),
        }
    }
}
impl Clone for Journal {
    fn clone(&self) -> Self {
        Self {
            instance_id: next_journal_id(),
            sequence: self.sequence,
            generations: self.generations.clone(),
            events: self.events.clone(),
        }
    }
}
impl Journal {
    /// Separate documents and mutable forks must not share renderer cursors.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
    pub fn cursor(&self) -> u64 {
        self.sequence
    }
    pub fn generations(&self, target: &Target) -> Generations {
        self.generations.get(target).copied().unwrap_or_default()
    }
    pub fn read(&self, cursor: u64) -> JournalRead {
        if cursor > self.sequence || self.events.front().is_some_and(|c| cursor < c.sequence - 1) {
            return JournalRead::Rebuild {
                cursor: self.sequence,
            };
        }
        JournalRead::Incremental {
            cursor: self.sequence,
            changes: {
                let start = self.events.front().map_or(0, |first| {
                    cursor.saturating_add(1).saturating_sub(first.sequence) as usize
                });
                self.events
                    .range(start.min(self.events.len())..)
                    .cloned()
                    .collect()
            },
        }
    }
    fn push(&mut self, target: Target, changes: Changes, removed: bool) {
        let _timer = crate::performance::time("scene_journal_push");
        if !changes.any() {
            return;
        }
        crate::performance::count("scene_journal_events", 1);
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("scene sequence exhausted");
        let generations = self.generations.entry(target.clone()).or_default();
        // Monotonic sequence values prevent delete/recreate ABA and Undo rollback.
        if changes.geometry {
            generations.geometry = self.sequence;
        }
        if changes.transform {
            generations.transform = self.sequence;
        }
        if changes.style {
            generations.style = self.sequence;
        }
        if changes.visibility {
            generations.visibility = self.sequence;
        }
        if changes.structure {
            generations.structure = self.sequence;
        }
        self.events.push_back(Change {
            sequence: self.sequence,
            target,
            changes,
            generations: *generations,
            removed,
            before_bounds: None,
            after_bounds: None,
        });
        if removed {
            // Sequence values are globally monotonic, so a later recreation
            // gets a fresh generation without retaining deleted IDs forever.
            let target = &self.events.back().unwrap().target;
            self.generations.remove(target);
        }
        while self.events.len() > JOURNAL_CAPACITY {
            self.events.pop_front();
        }
    }
    pub(crate) fn layers_changed(&mut self, before: &[SvgLayer], after: &[SvgLayer]) {
        let old: BTreeMap<_, _> = before
            .iter()
            .enumerate()
            .map(|(i, l)| (&l.id, (i, l)))
            .collect();
        let new: BTreeMap<_, _> = after
            .iter()
            .enumerate()
            .map(|(i, l)| (&l.id, (i, l)))
            .collect();
        for (id, (position, layer)) in &new {
            let target = Target {
                layer: (*id).clone(),
                object: None,
            };
            if let Some((old_position, previous)) = old.get(id) {
                self.push(
                    target,
                    Changes {
                        geometry: previous.source != layer.source,
                        transform: false,
                        style: previous.effective_opacity() != layer.effective_opacity(),
                        visibility: previous.visible != layer.visible,
                        structure: old_position != position,
                    },
                    false,
                );
                self.objects_changed(id, &previous.vector_objects, &layer.vector_objects);
            } else {
                self.push(target, Changes::all(), false);
                self.objects_changed(id, &[], &layer.vector_objects);
            }
        }
        for (id, (_, layer)) in old.iter().filter(|(id, _)| !new.contains_key(*id)) {
            self.objects_changed(id, &layer.vector_objects, &[]);
            self.push(
                Target {
                    layer: (*id).clone(),
                    object: None,
                },
                Changes::all(),
                true,
            );
        }
    }
    /// Targeted edits do not need to compare unrelated layers or objects.
    pub(crate) fn object_transformed(
        &mut self,
        layer: &str,
        object: &str,
        before_bounds: Option<spatial::Bounds>,
        after_bounds: Option<spatial::Bounds>,
    ) {
        self.push(
            Target {
                layer: layer.into(),
                object: None,
            },
            Changes {
                geometry: true,
                ..Changes::default()
            },
            false,
        );
        self.push(
            Target {
                layer: layer.into(),
                object: Some(object.into()),
            },
            Changes {
                transform: true,
                ..Changes::default()
            },
            false,
        );
        let change = self.events.back_mut().unwrap();
        change.before_bounds = before_bounds;
        change.after_bounds = after_bounds;
    }
    fn objects_changed(&mut self, layer: &str, before: &[VectorObject], after: &[VectorObject]) {
        let old: BTreeMap<_, _> = before
            .iter()
            .enumerate()
            .map(|(i, o)| (&o.id, (i, o)))
            .collect();
        let new: BTreeMap<_, _> = after
            .iter()
            .enumerate()
            .map(|(i, o)| (&o.id, (i, o)))
            .collect();
        for (id, (position, object)) in &new {
            let changes = if let Some((old_position, previous)) = old.get(id) {
                Changes {
                    geometry: previous.path != object.path
                        || previous.control_points != object.control_points
                        || previous.kind != object.kind
                        || previous.text != object.text
                        || previous.live_corners != object.live_corners
                        || previous.rectangle_radii != object.rectangle_radii,
                    transform: previous.transform != object.transform,
                    style: previous.fill != object.fill
                        || previous.stroke != object.stroke
                        || previous.stroke_width != object.stroke_width
                        || previous.stroke_style != object.stroke_style
                        || previous.fill_gradient != object.fill_gradient
                        || previous.stroke_gradient != object.stroke_gradient
                        || previous.opacity != object.opacity
                        || previous.blend_mode != object.blend_mode
                        || previous.image_frame != object.image_frame,
                    visibility: previous.visible != object.visible,
                    structure: old_position != position
                        || previous.group_path != object.group_path
                        || previous.clipping_group != object.clipping_group,
                }
            } else {
                Changes::all()
            };
            self.push(
                Target {
                    layer: layer.into(),
                    object: Some((*id).clone()),
                },
                changes,
                false,
            );
        }
        for id in old.keys().filter(|id| !new.contains_key(*id)) {
            self.push(
                Target {
                    layer: layer.into(),
                    object: Some((*id).clone()),
                },
                Changes::all(),
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Point};
    fn rectangle(id: &str, x: f32) -> VectorObject {
        serde_json::from_value(serde_json::json!({
            "id":id,"name":id,"visible":true,"path":{"data":"M0 0H20V20H0Z","fillRule":"nonZero"},
            "transform":[1.,0.,0.,1.,x,0.],"fill":{"color":[255,0,0,255]},"stroke":null,
            "strokeWidth":0.,"kind":"rectangle","controlPoints":[[0.,0.],[20.,0.],[20.,20.],[0.,20.]]
        })).unwrap()
    }
    #[test]
    fn edits_history_failed_transactions_and_native_reload() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let a = rectangle("a", 0.);
        let b = rectangle("b", 0.);
        document.upsert_vector_object(&layer, a.clone()).unwrap();
        document.upsert_vector_object(&layer, b.clone()).unwrap();
        let target = |id: &str| Target {
            layer: layer.clone(),
            object: Some(id.into()),
        };
        let a_before = document.scene_journal().generations(&target("a"));
        let b_before = document.scene_journal().generations(&target("b"));
        assert_eq!(
            document.vector_at(Point { x: 10., y: 10. }, 0.).as_deref(),
            Some("b")
        );
        let mut moved = b.clone();
        moved.transform[4] = 100.;
        document
            .upsert_vector_object(&layer, moved.clone())
            .unwrap();
        let b_after = document.scene_journal().generations(&target("b"));
        assert_eq!(document.scene_journal().generations(&target("a")), a_before);
        assert_eq!(b_after.geometry, b_before.geometry);
        assert_eq!(b_after.style, b_before.style);
        assert!(b_after.transform > b_before.transform);
        assert_eq!(
            document.vector_at(Point { x: 10., y: 10. }, 0.).as_deref(),
            Some("a")
        );
        assert_eq!(
            document.vector_at(Point { x: 110., y: 10. }, 0.).as_deref(),
            Some("b")
        );
        document.undo();
        assert!(document.scene_journal().generations(&target("b")).transform > b_after.transform);
        assert_eq!(
            document.vector_at(Point { x: 10., y: 10. }, 0.).as_deref(),
            Some("b")
        );
        document.redo();
        let cursor = document.scene_journal().cursor();
        moved.transform[0] = f32::NAN;
        assert!(document.upsert_vector_object(&layer, moved).is_err());
        assert_eq!(document.scene_journal().cursor(), cursor);
        document.select_vector_objects(vec!["b".into()]).unwrap();
        let before_delete = document.scene_journal().cursor();
        document.delete_selected_vector_objects().unwrap();
        assert_eq!(document.vector_at(Point { x: 110., y: 10. }, 0.), None);
        assert!(
            matches!(document.scene_journal().read(before_delete),JournalRead::Incremental {changes,..}
            if changes.iter().any(|c|c.target.object.as_deref()==Some("b")&&c.removed))
        );
        document.undo();
        assert_eq!(
            document.vector_at(Point { x: 110., y: 10. }, 0.).as_deref(),
            Some("b")
        );
        let encoded = document.encode().unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("sceneJournal"));
        let loaded = Document::decode(&encoded).unwrap();
        assert_eq!(loaded.scene_journal().cursor(), 0);
        for x in -2..125 {
            let point = Point {
                x: x as f32,
                y: 10.,
            };
            let expected = loaded
                .svg_layers()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .flat_map(|l| l.vector_objects.iter().rev())
                .find(|o| o.hit_test([point.x, point.y], 1.))
                .map(|o| o.id.clone());
            assert_eq!(loaded.vector_at(point, 1.), expected);
        }
    }
    #[test]
    fn independent_consumers_and_missing_ranges() {
        let mut journal = Journal::default();
        let target = Target {
            layer: "l".into(),
            object: Some("o".into()),
        };
        journal.push(
            target.clone(),
            Changes {
                transform: true,
                ..Default::default()
            },
            false,
        );
        let first = journal.cursor();
        journal.push(
            target.clone(),
            Changes {
                style: true,
                ..Default::default()
            },
            false,
        );
        assert_eq!(journal.generations(&target).transform, first);
        assert_eq!(journal.generations(&target).style, first + 1);
        assert!(
            matches!(journal.read(first), JournalRead::Incremental { changes, .. } if changes.len()==1)
        );
        assert!(
            matches!(journal.read(0), JournalRead::Incremental { changes, .. } if changes.len()==2)
        );
        for _ in 0..JOURNAL_CAPACITY {
            journal.push(target.clone(), Changes::all(), false);
        }
        assert!(matches!(journal.read(first), JournalRead::Rebuild { .. }));
        let last = journal.cursor();
        assert!(
            matches!(journal.read(last), JournalRead::Incremental { changes, .. } if changes.is_empty())
        );
        assert!(matches!(
            journal.read(last + 1),
            JournalRead::Rebuild { .. }
        ));
    }
}
