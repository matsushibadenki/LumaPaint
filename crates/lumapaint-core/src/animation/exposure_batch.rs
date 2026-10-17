//! Atomic timing edits. Plans contain only IDs and intervals, never drawing data.
use super::*;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExposureBatchEdit {
    Shift { delta: i32, ripple: bool },
    Hold { length: u32, ripple: bool },
    Delete { ripple: bool },
    Reorder { before: Option<u32> },
}
/// One track in a document-wide atomic edit. No drawing payloads cross the UI boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExposureTrackEdit {
    pub layer: String,
    pub frames: Vec<u32>,
    pub edit: ExposureBatchEdit,
}
type GroupPlan = (Vec<(String, Plan)>, u32);
type Plan = (BTreeMap<u32, Exposure>, u32, Vec<u32>);
fn invalid() -> String {
    "Invalid selection, collision or timing limit / 選択・区間の重複・時間の上限を確認してください / 请检查选择、区间重叠和时间限制".into()
}
impl Track {
    fn exposure_batch_plan(
        &self,
        frames: &[u32],
        edit: &ExposureBatchEdit,
        duration: u32,
    ) -> Result<Plan, String> {
        let selected: BTreeSet<_> = frames.iter().copied().collect();
        if frames.is_empty()
            || frames.len() > 2048
            || selected.len() != frames.len()
            || selected.iter().any(|f| !self.exposures.contains_key(f))
        {
            return Err(invalid());
        }
        let rows: Vec<_> = self
            .exposures
            .iter()
            .map(|(f, e)| (*f, self.exposure_end(*f, duration), e.cel))
            .collect();
        let first = *selected.first().unwrap();
        let mut result = vec![];
        let mut expand = false;
        match *edit {
            ExposureBatchEdit::Shift { delta, ripple } => {
                expand = ripple;
                for (start, end, cel) in &rows {
                    let offset = if selected.contains(start) || (ripple && *start >= first) {
                        i64::from(delta)
                    } else {
                        0
                    };
                    result.push((
                        i64::from(*start) + offset,
                        i64::from(*end) + offset,
                        *cel,
                        *start,
                    ));
                }
            }
            ExposureBatchEdit::Hold { length, ripple } => {
                if !(1..=18000).contains(&length) {
                    return Err(invalid());
                }
                expand = ripple;
                let mut offset = 0i64;
                for (start, end, cel) in &rows {
                    let old = i64::from(end - start);
                    let length = if selected.contains(start) {
                        i64::from(length)
                    } else {
                        old
                    };
                    let original = *start;
                    let start = i64::from(*start) + offset;
                    result.push((start, start + length, *cel, original));
                    if ripple {
                        offset += length - old;
                    }
                }
            }
            ExposureBatchEdit::Delete { ripple } => {
                let mut removed = 0i64;
                for (start, end, cel) in &rows {
                    if selected.contains(start) {
                        if ripple {
                            removed += i64::from(end - start);
                        }
                    } else {
                        result.push((
                            i64::from(*start) - removed,
                            i64::from(*end) - removed,
                            *cel,
                            *start,
                        ));
                    }
                }
            }
            ExposureBatchEdit::Reorder { before } => {
                if before.is_some_and(|f| !self.exposures.contains_key(&f)) {
                    return Err(invalid());
                }
                if before.is_some_and(|f| selected.contains(&f)) {
                    return Ok((self.exposures.clone(), duration, frames.to_vec()));
                }
                let mut remaining: Vec<_> = rows
                    .iter()
                    .copied()
                    .filter(|(f, _, _)| !selected.contains(f))
                    .collect();
                let index = before
                    .and_then(|f| remaining.iter().position(|(start, _, _)| *start == f))
                    .unwrap_or(remaining.len());
                remaining.splice(
                    index..index,
                    rows.iter()
                        .copied()
                        .filter(|(f, _, _)| selected.contains(f)),
                );
                let mut cursor = i64::from(rows[0].0);
                for (index, (start, end, cel)) in remaining.iter().enumerate() {
                    let length = i64::from(end - start);
                    result.push((cursor, cursor + length, *cel, *start));
                    cursor += length;
                    if let Some(next) = rows.get(index + 1) {
                        cursor += i64::from(next.0 - rows[index].1);
                    }
                }
            }
        }
        result.sort_by_key(|(start, _, _, _)| *start);
        let limit = if expand { 18000 } else { duration };
        let mut previous = 0;
        let mut required = duration;
        let mut plan = BTreeMap::new();
        let mut retained = Vec::new();
        for (start, end, cel, original) in result {
            if selected.contains(&original) {
                retained.push(start as u32);
            }
            if start < previous || start < 0 || end <= start || end > i64::from(limit) {
                return Err(invalid());
            }
            previous = end;
            required = required.max(end as u32);
            if plan
                .insert(
                    start as u32,
                    Exposure {
                        id: self.exposures[&original].id.clone(),
                        cel,
                        end: Some(end as u32),
                    },
                )
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok((plan, required, retained))
    }
}
impl Document {
    fn exposure_group_plan(&self, edits: &[ExposureTrackEdit]) -> Result<GroupPlan, String> {
        if edits.is_empty()
            || edits.len() > 256
            || edits.iter().map(|e| e.frames.len()).sum::<usize>() > 2048
        {
            return Err(invalid());
        }
        let mut layers = BTreeSet::new();
        let mut duration = self.animation.duration;
        let mut plans = Vec::with_capacity(edits.len());
        for edit in edits {
            if !layers.insert(&edit.layer) {
                return Err(invalid());
            }
            self.animation_layer(&edit.layer, true)?;
            let plan = self
                .animation
                .tracks
                .get(&edit.layer)
                .ok_or_else(invalid)?
                .exposure_batch_plan(&edit.frames, &edit.edit, self.animation.duration)?;
            duration = duration.max(plan.1);
            plans.push((edit.layer.clone(), plan));
        }
        Ok((plans, duration))
    }
    pub fn animation_exposure_group_selection(
        &self,
        edits: &[ExposureTrackEdit],
    ) -> Result<Vec<(String, Vec<u32>)>, String> {
        Ok(self
            .exposure_group_plan(edits)?
            .0
            .into_iter()
            .map(|(layer, (_, _, frames))| (layer, frames))
            .collect())
    }
    pub fn edit_animation_exposure_group(
        &mut self,
        edits: &[ExposureTrackEdit],
    ) -> Result<(), String> {
        // Every plan must succeed against the SAME source before touching state or history.
        let (plans, duration) = self.exposure_group_plan(edits)?;
        let unchanged = duration == self.animation.duration
            && plans.iter().all(|(layer, (plan, _, _))| {
                let track = &self.animation.tracks[layer];
                plan.len() == track.exposures.len()
                    && plan.iter().all(|(frame, exposure)| {
                        track.exposures.get(frame).is_some_and(|old| {
                            old.id == exposure.id
                                && old.cel == exposure.cel
                                && track.exposure_end(*frame, duration)
                                    == exposure
                                        .end
                                        .unwrap_or_else(|| track.exposure_end(*frame, duration))
                        })
                    })
            });
        if unchanged {
            return Ok(());
        }
        let mut next = self.animation.as_ref().clone();
        next.duration = duration;
        for (layer, (plan, _, _)) in plans {
            next.tracks.get_mut(&layer).unwrap().exposures = plan;
        }
        self.commit_animation(next)
    }
    /// Maps selected occurrences through the exact validated timing plan.
    /// Drawing IDs alone cannot identify repeated placements of the same cel.
    pub fn animation_exposure_batch_selection(
        &self,
        layer: &str,
        frames: &[u32],
        edit: &ExposureBatchEdit,
    ) -> Result<Vec<u32>, String> {
        self.animation_layer(layer, true)?;
        Ok(self
            .animation
            .tracks
            .get(layer)
            .ok_or_else(invalid)?
            .exposure_batch_plan(frames, edit, self.animation.duration)?
            .2)
    }
    pub fn validate_animation_exposure_batch(
        &self,
        layer: &str,
        frames: &[u32],
        edit: &ExposureBatchEdit,
    ) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        self.animation
            .tracks
            .get(layer)
            .ok_or_else(invalid)?
            .exposure_batch_plan(frames, edit, self.animation.duration)?;
        Ok(())
    }
    pub fn edit_animation_exposure_batch(
        &mut self,
        layer: &str,
        frames: &[u32],
        edit: &ExposureBatchEdit,
    ) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let track = self.animation.tracks.get(layer).ok_or_else(invalid)?;
        let (plan, duration, _) =
            track.exposure_batch_plan(frames, edit, self.animation.duration)?;
        let unchanged = duration == self.animation.duration
            && plan.len() == track.exposures.len()
            && plan.iter().all(|(f, e)| {
                track.exposures.get(f).is_some_and(|old| {
                    old.id == e.id
                        && old.cel == e.cel
                        && track.exposure_end(*f, duration)
                            == e.end.unwrap_or_else(|| track.exposure_end(*f, duration))
                })
            });
        if unchanged {
            return Ok(());
        }
        let mut next = self.animation.as_ref().clone();
        next.duration = duration;
        next.tracks.get_mut(layer).unwrap().exposures = plan;
        self.commit_animation(next)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> Document {
        let mut d = Document::default();
        d.set_animation_settings(24, 12, true).unwrap();
        for frame in [0, 3, 6, 9] {
            d.capture_animation_cel("layer-1", frame, true).unwrap();
        }
        d
    }
    fn group_doc() -> (Document, String) {
        let mut d = Document::default();
        d.set_animation_settings(24, 12, true).unwrap();
        let layer = d.add_paint_layer().unwrap();
        for (id, starts) in [("layer-1", [0, 4, 8]), (layer.as_str(), [1, 5, 9])] {
            for frame in starts {
                d.capture_animation_cel(id, frame, true).unwrap();
                d.edit_animation_exposure(id, frame, frame, frame + 2)
                    .unwrap();
            }
        }
        (d, layer)
    }
    fn jobs(layer: &str, frames: [u32; 2], edit: ExposureBatchEdit) -> Vec<ExposureTrackEdit> {
        vec![
            ExposureTrackEdit {
                layer: "layer-1".into(),
                frames: vec![frames[0]],
                edit: edit.clone(),
            },
            ExposureTrackEdit {
                layer: layer.into(),
                frames: vec![frames[1]],
                edit,
            },
        ]
    }
    #[test]
    fn group_timing_is_atomic_and_preserves_artwork_ids_and_one_history_entry() {
        let (mut d, layer) = group_doc();
        let original = serde_json::to_string(&d.document_state()).unwrap();
        let drawing = d.animation().tracks[&layer].cel_library[&1].drawing.clone();
        let edits = jobs(
            &layer,
            [0, 1],
            ExposureBatchEdit::Shift {
                delta: 1,
                ripple: false,
            },
        );
        assert_eq!(
            d.animation_exposure_group_selection(&edits).unwrap(),
            vec![("layer-1".into(), vec![1]), (layer.clone(), vec![2])]
        );
        let revision = d.revision();
        d.edit_animation_exposure_group(&edits).unwrap();
        assert_eq!(d.revision(), revision + 1);
        assert!(Arc::ptr_eq(
            &drawing,
            &d.animation().tracks[&layer].cel_library[&1].drawing
        ));
        let changed = serde_json::to_string(&d.document_state()).unwrap();
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(
            serde_json::to_string(&restored.document_state()).unwrap(),
            changed
        );
        d.undo();
        assert_eq!(
            serde_json::to_string(&d.document_state()).unwrap(),
            original
        );
        d.redo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), changed);
    }
    #[test]
    fn group_rejection_leaves_every_track_revision_and_history_untouched() {
        let (mut d, layer) = group_doc();
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        let revision = d.revision();
        let mut edits = jobs(
            &layer,
            [0, 1],
            ExposureBatchEdit::Shift {
                delta: 1,
                ripple: false,
            },
        );
        edits[1].edit = ExposureBatchEdit::Shift {
            delta: 3,
            ripple: false,
        };
        assert!(d.edit_animation_exposure_group(&edits).is_err());
        assert_eq!(d.revision(), revision);
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        edits[1] = edits[0].clone();
        assert!(d.edit_animation_exposure_group(&edits).is_err());
        d.svg_layers
            .iter_mut()
            .find(|l| l.id == layer)
            .unwrap()
            .locked = true;
        let locked = serde_json::to_string(&d.document_state()).unwrap();
        assert!(d
            .edit_animation_exposure_group(&jobs(
                &layer,
                [0, 1],
                ExposureBatchEdit::Delete { ripple: true }
            ))
            .is_err());
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), locked);
        assert_eq!(d.revision(), revision);
        assert!(d.edit_animation_exposure_group(&[]).is_err());
    }
    #[test]
    fn group_ripple_hold_delete_and_reorder_use_original_duration_per_track() {
        let (mut d, layer) = group_doc();
        let ids: Vec<_> = d.animation().tracks["layer-1"]
            .exposures
            .values()
            .map(|e| e.id.clone())
            .collect();
        let edits = jobs(
            &layer,
            [0, 1],
            ExposureBatchEdit::Hold {
                length: 6,
                ripple: true,
            },
        );
        d.edit_animation_exposure_group(&edits).unwrap();
        assert_eq!(d.animation().duration, 15);
        assert_eq!(
            d.animation().tracks["layer-1"]
                .exposures
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [0, 8, 12]
        );
        assert_eq!(
            d.animation().tracks[&layer]
                .exposures
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [1, 9, 13]
        );
        d.undo();
        let mut edits = jobs(&layer, [0, 1], ExposureBatchEdit::Reorder { before: None });
        edits[0].edit = ExposureBatchEdit::Reorder { before: Some(8) };
        edits[1].edit = ExposureBatchEdit::Reorder { before: Some(9) };
        d.edit_animation_exposure_group(&edits).unwrap();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&4].id, ids[0]);
        d.undo();
        let edits = jobs(&layer, [0, 1], ExposureBatchEdit::Delete { ripple: true });
        d.edit_animation_exposure_group(&edits).unwrap();
        assert_eq!(
            d.animation().tracks["layer-1"]
                .exposures
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [2, 6]
        );
        assert_eq!(
            d.animation().tracks[&layer]
                .exposures
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [3, 7]
        );
        assert_eq!(d.animation().tracks[&layer].cel_library.len(), 3);
    }
    #[test]
    fn reordering_identical_shared_cels_still_moves_occurrence_identities() {
        let (mut d, layer) = group_doc();
        for id in ["layer-1", layer.as_str()] {
            let starts: Vec<_> = d.animation().tracks[id].exposures.keys().copied().collect();
            let cel = d.animation().tracks[id].exposures[&starts[0]].cel;
            for f in starts {
                d.assign_animation_cel(id, f, cel).unwrap();
            }
        }
        let original = d.animation().tracks["layer-1"].exposures[&0].id.clone();
        d.edit_animation_exposure_group(&jobs(
            &layer,
            [0, 1],
            ExposureBatchEdit::Reorder { before: None },
        ))
        .unwrap();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&8].id, original);
    }
    #[test]
    fn persistent_occurrence_ids_survive_timing_history_and_legacy_migration() {
        let mut d = doc();
        let ids: Vec<_> = d.animation().tracks["layer-1"]
            .exposures
            .values()
            .map(|e| e.id.clone())
            .collect();
        d.edit_animation_exposure_batch(
            "layer-1",
            &[0, 3],
            &ExposureBatchEdit::Reorder { before: None },
        )
        .unwrap();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&6].id, ids[0]);
        d.undo();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&0].id, ids[0]);
        d.redo();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&6].id, ids[0]);
        let track = &d.animation().tracks["layer-1"];
        let mut wire = serde_json::to_value(track).unwrap();
        let restored: Track = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(restored.exposures[&6].id, ids[0]);
        for entry in wire["exposures"].as_array_mut().unwrap() {
            entry[1].as_object_mut().unwrap().remove("id");
        }
        let migrated: Track = serde_json::from_value(wire).unwrap();
        let unique: std::collections::BTreeSet<_> =
            migrated.exposures.values().map(|e| &e.id).collect();
        assert_eq!(unique.len(), 4);
        assert!(!unique.contains(&String::new()));
        let mut wire = serde_json::to_value(track).unwrap();
        wire["exposures"][1][1]["id"] = wire["exposures"][0][1]["id"].clone();
        assert!(serde_json::from_value::<Track>(wire).is_err());
    }
    #[test]
    fn undo_branch_creates_fresh_ids_and_split_keeps_original_occurrence() {
        let mut d = doc();
        let cel = d.animation().tracks["layer-1"].exposures[&0].cel;
        let original = d.animation().tracks["layer-1"].exposures[&0].id.clone();
        d.assign_animation_cel("layer-1", 1, cel).unwrap();
        let split = d.animation().tracks["layer-1"].exposures[&1].id.clone();
        assert_ne!(split, original);
        assert_eq!(d.animation().tracks["layer-1"].exposures[&0].id, original);
        d.undo();
        d.assign_animation_cel("layer-1", 1, cel).unwrap();
        assert_ne!(d.animation().tracks["layer-1"].exposures[&1].id, split);
        let assigned = d.animation().tracks["layer-1"].exposures[&1].id.clone();
        d.assign_animation_cel("layer-1", 1, cel).unwrap();
        d.edit_animation_exposure("layer-1", 1, 1, 2).unwrap();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&1].id, assigned);
    }
    #[test]
    fn selected_occurrences_follow_shared_cels_through_timing_edits() {
        let mut d = doc();
        let cel = d.animation().tracks["layer-1"].exposures[&0].cel;
        d.assign_animation_cel("layer-1", 3, cel).unwrap();
        assert_eq!(
            d.animation_exposure_batch_selection(
                "layer-1",
                &[0, 3],
                &ExposureBatchEdit::Reorder { before: None }
            )
            .unwrap(),
            vec![6, 9]
        );
        assert_eq!(
            d.animation_exposure_batch_selection(
                "layer-1",
                &[3, 6],
                &ExposureBatchEdit::Shift {
                    delta: 2,
                    ripple: true
                }
            )
            .unwrap(),
            vec![5, 8]
        );
        assert_eq!(
            d.animation_exposure_batch_selection(
                "layer-1",
                &[0, 3],
                &ExposureBatchEdit::Hold {
                    length: 2,
                    ripple: true
                }
            )
            .unwrap(),
            vec![0, 2]
        );
        assert!(d
            .animation_exposure_batch_selection(
                "layer-1",
                &[0, 3],
                &ExposureBatchEdit::Delete { ripple: true }
            )
            .unwrap()
            .is_empty());
        assert!(d
            .animation_exposure_batch_selection(
                "layer-1",
                &[4],
                &ExposureBatchEdit::Reorder { before: None }
            )
            .is_err());
    }
    fn rows(d: &Document) -> Vec<(u32, u32, u32)> {
        let t = &d.animation().tracks["layer-1"];
        t.exposures
            .iter()
            .map(|(f, e)| (*f, t.exposure_end(*f, d.animation().duration), e.cel))
            .collect()
    }
    #[test]
    fn batch_shift_rejects_collision_atomically_and_keeps_cel_arcs() {
        let mut d = doc();
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        let revision = d.revision();
        for frames in [vec![0, 0], vec![2], vec![0, 6]] {
            assert!(d
                .edit_animation_exposure_batch(
                    "layer-1",
                    &frames,
                    &ExposureBatchEdit::Shift {
                        delta: 1,
                        ripple: false
                    }
                )
                .is_err());
            assert_eq!(d.revision(), revision);
            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        }
        let cel = d.animation().tracks["layer-1"].cel_library[&2]
            .drawing
            .clone();
        d.edit_animation_exposure_batch(
            "layer-1",
            &[3, 6],
            &ExposureBatchEdit::Shift {
                delta: 2,
                ripple: true,
            },
        )
        .unwrap();
        assert_eq!(
            rows(&d),
            vec![(0, 3, 1), (5, 8, 2), (8, 11, 3), (11, 14, 4)]
        );
        assert_eq!(d.animation().duration, 14);
        assert!(Arc::ptr_eq(
            &cel,
            &d.animation().tracks["layer-1"].cel_library[&2].drawing
        ));
        d.undo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        d.redo();
        assert_eq!(rows(&d)[1].0, 5);
    }
    #[test]
    fn ripple_holds_delete_and_reorder_preserve_lengths_and_gap_slots() {
        let mut d = doc();
        d.edit_animation_exposure_batch(
            "layer-1",
            &[0, 6],
            &ExposureBatchEdit::Hold {
                length: 4,
                ripple: true,
            },
        )
        .unwrap();
        assert_eq!(
            rows(&d),
            vec![(0, 4, 1), (4, 7, 2), (7, 11, 3), (11, 14, 4)]
        );
        d.edit_animation_exposure_batch(
            "layer-1",
            &[0, 7],
            &ExposureBatchEdit::Reorder { before: None },
        )
        .unwrap();
        assert_eq!(
            rows(&d),
            vec![(0, 3, 2), (3, 6, 4), (6, 10, 1), (10, 14, 3)]
        );
        d.edit_animation_exposure_batch(
            "layer-1",
            &[3, 10],
            &ExposureBatchEdit::Delete { ripple: true },
        )
        .unwrap();
        assert_eq!(rows(&d), vec![(0, 3, 2), (3, 7, 1)]);
        assert_eq!(d.animation().tracks["layer-1"].cel_library.len(), 4);
        assert_eq!(d.animation().duration, 14);
        let encoded = serde_json::to_string(&d.document_state()).unwrap();
        let restored =
            Document::from_document_state(serde_json::from_str(&encoded).unwrap()).unwrap();
        assert_eq!(rows(&restored), rows(&d));
    }
    #[test]
    fn ordinary_group_edits_preserve_gaps_and_empty_selection_is_rejected() {
        let mut d = doc();
        let initial = d.revision();
        d.edit_animation_exposure_batch(
            "layer-1",
            &[9],
            &ExposureBatchEdit::Reorder { before: Some(9) },
        )
        .unwrap();
        assert_eq!(d.revision(), initial);
        d.edit_animation_exposure_batch(
            "layer-1",
            &[0, 3, 6, 9],
            &ExposureBatchEdit::Hold {
                length: 2,
                ripple: false,
            },
        )
        .unwrap();
        d.edit_animation_exposure_batch(
            "layer-1",
            &[0, 6],
            &ExposureBatchEdit::Shift {
                delta: 1,
                ripple: false,
            },
        )
        .unwrap();
        assert_eq!(rows(&d), vec![(1, 3, 1), (3, 5, 2), (7, 9, 3), (9, 11, 4)]);
        d.edit_animation_exposure_batch(
            "layer-1",
            &[7],
            &ExposureBatchEdit::Reorder { before: Some(1) },
        )
        .unwrap();
        assert_eq!(rows(&d), vec![(1, 3, 3), (3, 5, 1), (7, 9, 2), (9, 11, 4)]);
        let revision = d.revision();
        assert!(d
            .edit_animation_exposure_batch(
                "layer-1",
                &[],
                &ExposureBatchEdit::Delete { ripple: false }
            )
            .is_err());
        assert!(d
            .edit_animation_exposure_batch(
                "layer-1",
                &[1],
                &ExposureBatchEdit::Hold {
                    length: 18001,
                    ripple: true
                }
            )
            .is_err());
        assert!(d
            .edit_animation_exposure_batch(
                "layer-1",
                &[1],
                &ExposureBatchEdit::Shift {
                    delta: i32::MIN,
                    ripple: true
                }
            )
            .is_err());
        assert_eq!(d.revision(), revision);
        d.edit_animation_exposure_batch(
            "layer-1",
            &[1],
            &ExposureBatchEdit::Shift {
                delta: 0,
                ripple: false,
            },
        )
        .unwrap();
        assert_eq!(d.revision(), revision);
    }
}
