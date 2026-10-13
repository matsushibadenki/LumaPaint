//! Independent drawing identities and half-open exposure intervals.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedCel {
    pub name: String,
    pub drawing: Arc<Cel>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Exposure {
    #[serde(default)]
    pub id: String,
    pub cel: u32,
    /// None follows the next exposure or the timeline end (legacy holds).
    pub end: Option<u32>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TrackData {
    #[serde(
        default,
        with = "cel_frames",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    cels: BTreeMap<u32, Arc<Cel>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cel_library: Vec<(u32, NamedCel)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exposures: Vec<(u32, Exposure)>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pixel: bool,
    #[serde(default)]
    next_cel_id: u32,
    channels: BTreeMap<Property, Vec<Keyframe>>,
    anchor: [f32; 2],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    motions: Vec<crate::motion::Clip>,
}
impl From<Track> for TrackData {
    fn from(mut t: Track) -> Self {
        t.migrate_cels();
        t.ensure_exposure_ids();
        Self {
            cels: BTreeMap::new(),
            pixel: t.pixel,
            next_cel_id: t.next_cel_id,
            cel_library: t.cel_library.into_iter().collect(),
            exposures: t.exposures.into_iter().collect(),
            channels: t.channels,
            anchor: t.anchor,
            motions: t.motions,
        }
    }
}
impl TryFrom<TrackData> for Track {
    type Error = String;
    fn try_from(data: TrackData) -> Result<Self, String> {
        if data.cel_library.len() > 2048 || data.exposures.len() > 2048 {
            return Err("Cel/exposure budget exceeded".into());
        }
        let mut t = Self {
            next_cel_id: data.next_cel_id,
            pixel: data.pixel || !data.cel_library.is_empty(),
            cels: data.cels,
            cel_library: BTreeMap::new(),
            exposures: BTreeMap::new(),
            channels: data.channels,
            anchor: data.anchor,
            motions: data.motions,
        };
        // Duplicate wire identities must remain invalid until document validation.
        for (id, cel) in data.cel_library {
            if t.cel_library.insert(id, cel).is_some() {
                return Err("Duplicate or mixed cel/exposure data".into());
            }
        }
        for (frame, exposure) in data.exposures {
            if t.exposures.insert(frame, exposure).is_some() {
                return Err("Duplicate or mixed cel/exposure data".into());
            }
        }
        if !t.cels.is_empty() && (t.pixel || !t.cel_library.is_empty() || !t.exposures.is_empty()) {
            return Err("Duplicate or mixed cel/exposure data".into());
        }
        t.migrate_cels();
        t.ensure_exposure_ids();
        t.validate_exposure_ids()?;
        t.rebuild_cels();
        Ok(t)
    }
}
// A process epoch plus a monotonic sequence is independent of undoable state.
// IDs are opaque occurrence labels, not security tokens.
fn new_exposure_id() -> String {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    };
    static EPOCH: OnceLock<u128> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let epoch = EPOCH.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    });
    format!("e-{epoch:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}
impl Track {
    fn ensure_exposure_ids(&mut self) {
        let mut used: std::collections::BTreeSet<String> = self
            .exposures
            .values()
            .filter(|e| !e.id.is_empty())
            .map(|e| e.id.clone())
            .collect();
        for exposure in self.exposures.values_mut().filter(|e| e.id.is_empty()) {
            loop {
                let id = new_exposure_id();
                if used.insert(id.clone()) {
                    exposure.id = id;
                    break;
                }
            }
        }
    }
    fn migrate_cels(&mut self) {
        if self.pixel || !self.cel_library.is_empty() || self.cels.is_empty() {
            return;
        }
        self.pixel = true;
        let old = std::mem::take(&mut self.cels);
        let starts: Vec<_> = old.keys().copied().collect();
        for (index, (frame, drawing)) in old.into_iter().enumerate() {
            let id = self
                .cel_library
                .iter()
                .find(|(_, c)| Arc::ptr_eq(&c.drawing, &drawing))
                .map(|(id, _)| *id)
                .unwrap_or(self.cel_library.len() as u32 + 1);
            self.cel_library.entry(id).or_insert_with(|| NamedCel {
                name: id.to_string(),
                drawing,
            });
            self.exposures.insert(
                frame,
                Exposure {
                    id: new_exposure_id(),
                    cel: id,
                    end: starts.get(index + 1).copied(),
                },
            );
        }
        self.next_cel_id = self
            .cel_library
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.rebuild_cels();
    }
    pub fn exposure_end(&self, start: u32, duration: u32) -> u32 {
        self.exposures
            .get(&start)
            .and_then(|e| e.end)
            .unwrap_or_else(|| {
                self.exposures
                    .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                    .next()
                    .map_or(duration, |(f, _)| *f)
            })
    }
    pub fn exposure_at(&self, frame: u32, duration: u32) -> Option<(u32, &Exposure)> {
        self.exposures
            .range(..=frame)
            .next_back()
            .filter(|(start, _)| frame < self.exposure_end(**start, duration))
            .map(|(f, e)| (*f, e))
    }
    pub(super) fn rebuild_cels(&mut self) {
        if !self.pixel && self.cel_library.is_empty() {
            return;
        }
        self.cels.clear();
        let blank = Arc::new(Cel {
            source: None,
            strokes: vec![],
        });
        for (start, e) in &self.exposures {
            if let Some(c) = self.cel_library.get(&e.cel) {
                self.cels.insert(*start, c.drawing.clone());
            }
            if let Some(end) = e.end {
                if !self.exposures.contains_key(&end) {
                    self.cels.insert(end, blank.clone());
                }
            }
        }
        // A library without exposure still represents a pixel-animation track.
        if self.exposures.is_empty() {
            self.cels.insert(0, blank);
        }
    }
    pub(super) fn refresh_exposures(&mut self, duration: u32) -> Result<(), String> {
        self.migrate_cels();
        self.ensure_exposure_ids();
        self.validate_exposures(duration)?;
        self.rebuild_cels();
        Ok(())
    }
    pub(super) fn validate_playback_cache(&self) -> Result<(), String> {
        if !self.pixel {
            return Ok(());
        }
        for (frame, cel) in &self.cels {
            if let Some(e) = self.exposures.get(frame) {
                if !self
                    .cel_library
                    .get(&e.cel)
                    .is_some_and(|c| Arc::ptr_eq(&c.drawing, cel))
                {
                    return Err("Invalid resolved exposure".into());
                }
            } else if !(cel.source.is_none()
                && cel.strokes.is_empty()
                && (self.exposures.is_empty() && *frame == 0
                    || self.exposures.values().any(|e| e.end == Some(*frame))))
            {
                return Err("Invalid exposure gap".into());
            }
        }
        if self
            .exposures
            .values()
            .filter_map(|e| e.end)
            .any(|end| !self.exposures.contains_key(&end) && !self.cels.contains_key(&end))
            || self.exposures.is_empty() && !self.cels.contains_key(&0)
            || self.exposures.keys().any(|f| !self.cels.contains_key(f))
        {
            return Err("Missing resolved exposure".into());
        }
        Ok(())
    }
    fn validate_exposure_ids(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        for e in self.exposures.values() {
            if e.id.is_empty()
                || e.id.len() > 64
                || !e.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || !ids.insert(&e.id)
            {
                return Err("Invalid or duplicate exposure identity".into());
            }
        }
        Ok(())
    }
    pub(super) fn validate_exposures(&self, duration: u32) -> Result<(), String> {
        if !self.pixel && self.cels.keys().any(|f| *f >= duration) {
            return Err("Cel frame outside timeline".into());
        }
        if self.cel_library.len() > 2048 || self.exposures.len() > 2048 {
            return Err("Cel/exposure budget exceeded".into());
        }
        for (id, cel) in &self.cel_library {
            if *id == 0
                || cel.name.trim().is_empty()
                || cel.name.chars().count() > 64
                || cel.name.chars().any(char::is_control)
            {
                return Err("Invalid cel name / セル名が不正です / 画格名称无效".into());
            }
        }
        self.validate_exposure_ids()?;
        for (start, e) in &self.exposures {
            let end = self.exposure_end(*start, duration);
            let next = self
                .exposures
                .range((
                    std::ops::Bound::Excluded(*start),
                    std::ops::Bound::Unbounded,
                ))
                .next()
                .map_or(duration, |(f, _)| *f);
            if *start >= duration
                || end <= *start
                || end > next
                || !self.cel_library.contains_key(&e.cel)
            {
                return Err("Exposure overlaps or references a missing cel / 露出が重なるかセルがありません / 曝光重叠或画格不存在".into());
            }
        }
        Ok(())
    }
    pub(crate) fn stored_cels(&self) -> Vec<(u32, &Arc<Cel>)> {
        if self.cel_library.is_empty() && !self.pixel {
            self.cels
                .values()
                .enumerate()
                .map(|(i, c)| (i as u32, c))
                .collect()
        } else {
            self.cel_library
                .values()
                .enumerate()
                .map(|(i, c)| (i as u32, &c.drawing))
                .collect()
        }
    }
    fn new_cel(&mut self, drawing: Arc<Cel>) -> Result<u32, String> {
        self.pixel = true;
        if self.cel_library.len() >= 2048 {
            return Err("Cel library is full".into());
        }
        let id = self.next_cel_id.max(
            self.cel_library
                .keys()
                .next_back()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Cel identity exhausted")?,
        );
        self.next_cel_id = id.checked_add(1).ok_or("Cel identity exhausted")?;
        self.cel_library.insert(
            id,
            NamedCel {
                name: id.to_string(),
                drawing,
            },
        );
        Ok(id)
    }
    pub(super) fn assign_cel(&mut self, frame: u32, cel: u32, duration: u32) -> Result<(), String> {
        if frame >= duration || !self.cel_library.contains_key(&cel) {
            return Err("Invalid exposure assignment".into());
        }
        if let Some(e) = self.exposures.get_mut(&frame) {
            e.cel = cel;
        } else {
            let end = self.exposures.range(frame..).next().map(|(f, _)| *f);
            if let Some((start, _)) = self.exposure_at(frame, duration) {
                self.exposures.get_mut(&start).unwrap().end = Some(frame);
            }
            self.exposures.insert(
                frame,
                Exposure {
                    id: new_exposure_id(),
                    cel,
                    end,
                },
            );
        }
        self.refresh_exposures(duration)
    }
    pub(super) fn record_cel(
        &mut self,
        frame: u32,
        drawing: Arc<Cel>,
        duration: u32,
        reuse: bool,
    ) -> Result<(), String> {
        self.refresh_exposures(duration)?;
        if frame >= duration {
            return Err("Frame outside timeline".into());
        }
        if let Some(id) = reuse
            .then(|| self.exposure_at(frame, duration).map(|(_, e)| e.cel))
            .flatten()
        {
            self.cel_library.get_mut(&id).unwrap().drawing = drawing;
            self.rebuild_cels();
            Ok(())
        } else {
            let id = self.new_cel(drawing)?;
            self.assign_cel(frame, id, duration)
        }
    }
}
impl Document {
    pub fn validate_animation_exposure_edit(
        &self,
        layer: &str,
        from: u32,
        start: u32,
        end: u32,
    ) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let track = self.animation.tracks.get(layer).ok_or("No exposure")?;
        if !track.exposures.contains_key(&from)
            || start >= end
            || end > self.animation.duration
            || track.exposures.iter().any(|(f, _)| {
                *f != from && start < track.exposure_end(*f, self.animation.duration) && end > *f
            })
        {
            return Err("Invalid or overlapping exposure / 露出区間が不正または重複しています / 曝光区间无效或重叠".into());
        }
        Ok(())
    }
    pub fn edit_animation_exposure(
        &mut self,
        layer: &str,
        from: u32,
        start: u32,
        end: u32,
    ) -> Result<(), String> {
        self.validate_animation_exposure_edit(layer, from, start, end)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(layer).ok_or("No exposure")?;
        track.refresh_exposures(next.duration)?;
        if start == from && end == track.exposure_end(from, next.duration) {
            return Ok(());
        }
        if let Some((previous, _)) = track.exposures.range(..from).next_back() {
            let previous = *previous;
            let previous_end = track.exposure_end(previous, next.duration);
            if track.exposures[&previous].end.is_none() {
                track.exposures.get_mut(&previous).unwrap().end = Some(previous_end);
            }
        }
        let exposure = track.exposures.remove(&from).ok_or("No exposure")?;
        if track.exposures.contains_key(&start) {
            return Err(
                "Exposure destination occupied / 移動先に露出があります / 目标位置已有曝光".into(),
            );
        }
        track.exposures.insert(
            start,
            Exposure {
                id: exposure.id,
                cel: exposure.cel,
                end: Some(end),
            },
        );
        self.commit_animation(next)
    }
    pub fn assign_animation_cel(
        &mut self,
        layer: &str,
        frame: u32,
        cel: u32,
    ) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(layer).ok_or("No cel library")?;
        track.refresh_exposures(next.duration)?;
        track.assign_cel(frame, cel, next.duration)?;
        self.commit_animation(next)
    }
    pub fn rename_animation_cel(
        &mut self,
        layer: &str,
        cel: u32,
        name: String,
    ) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let mut next = self.animation.as_ref().clone();
        next.tracks
            .get_mut(layer)
            .and_then(|t| t.cel_library.get_mut(&cel))
            .ok_or("No cel")?
            .name = name;
        self.commit_animation(next)
    }
    pub fn create_animation_cel(&mut self, layer: &str) -> Result<u32, String> {
        self.animation_layer(layer, true)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.entry(layer.into()).or_default();
        track.refresh_exposures(next.duration)?;
        let id = track.new_cel(Arc::new(Cel {
            source: None,
            strokes: vec![],
        }))?;
        self.commit_animation(next)?;
        Ok(id)
    }
    pub fn remove_animation_cel(&mut self, layer: &str, cel: u32) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(layer).ok_or("No cel")?;
        if track.exposures.values().any(|e| e.cel == cel) {
            return Err("Cel is in use / 使用中のセルは削除できません / 画格正在使用中".into());
        }
        if track.cel_library.remove(&cel).is_none() {
            return Err("No cel".into());
        }
        track.cels.clear();
        self.commit_animation(next)
    }
    pub fn fork_animation_cel(&mut self, layer: &str, frame: u32) -> Result<(), String> {
        self.animation_layer(layer, true)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(layer).ok_or("No exposure")?;
        let (start, e) = track
            .exposure_at(frame, next.duration)
            .ok_or("No exposure")?;
        let drawing = track.cel_library[&e.cel].drawing.clone();
        let id = track.new_cel(drawing)?;
        track.exposures.get_mut(&start).unwrap().cel = id;
        self.commit_animation(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drawing(color: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"480\"><rect width=\"20\" height=\"20\" fill=\"{color}\"/></svg>")
    }
    fn document() -> Document {
        let mut d = Document {
            paint_source: Some(drawing("red")),
            ..Default::default()
        };
        d.capture_animation_cel("layer-1", 0, false).unwrap();
        d
    }
    #[test]
    fn shared_cels_update_every_exposure_and_forks_edit_independently() {
        let mut d = document();
        d.duplicate_animation_cel("layer-1", 0, 10).unwrap();
        let id = d.animation().tracks["layer-1"].exposures[&0].cel;
        assert_eq!(d.animation().tracks["layer-1"].cel_library.len(), 1);
        assert_eq!(d.animation().tracks["layer-1"].exposures[&10].cel, id);
        d.paint_source = Some(drawing("blue"));
        d.capture_animation_cel("layer-1", 10, false).unwrap();
        let track = &d.animation().tracks["layer-1"];
        assert_eq!(track.exposures.len(), 2);
        assert!(Arc::ptr_eq(&track.cels[&0], &track.cels[&10]));
        assert!(track.cels[&0].source.as_ref().unwrap().contains("blue"));
        d.fork_animation_cel("layer-1", 12).unwrap();
        d.paint_source = Some(drawing("green"));
        d.capture_animation_cel("layer-1", 10, false).unwrap();
        let track = &d.animation().tracks["layer-1"];
        assert_eq!(track.cel_library.len(), 2);
        assert!(track.cels[&0].source.as_ref().unwrap().contains("blue"));
        assert!(track.cels[&10].source.as_ref().unwrap().contains("green"));
        let encoded = serde_json::to_string(&d.document_state()).unwrap();
        let restored =
            Document::from_document_state(serde_json::from_str(&encoded).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_string(&restored.document_state()).unwrap(),
            encoded
        );
    }
    #[test]
    fn exposure_move_resize_gaps_and_delete_preserve_drawings_and_history() {
        let mut d = document();
        d.duplicate_animation_cel("layer-1", 0, 10).unwrap();
        d.edit_animation_exposure("layer-1", 0, 2, 8).unwrap();
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        let t = &d.animation().tracks["layer-1"];
        assert!(t.exposure_at(0, 120).is_none());
        assert!(t.exposure_at(7, 120).is_some());
        assert!(t.exposure_at(8, 120).is_none());
        assert!(t.exposure_at(10, 120).is_some());
        let mut preview = d.clone();
        preview.evaluate_animation(&d, 8);
        assert!(preview.paint_source.is_none());
        preview.evaluate_animation(&d, 2);
        assert!(preview.paint_source.is_some());
        d.undo();
        assert!(d.animation().tracks["layer-1"].exposures.contains_key(&0));
        d.redo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        let revision = d.revision();
        for (start, end) in [(7, 11), (9, 9), (0, 121)] {
            assert!(d.edit_animation_exposure("layer-1", 2, start, end).is_err());
            assert_eq!(d.revision(), revision);
            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        }
        d.remove_animation_key("layer-1", 2, None).unwrap();
        assert_eq!(d.animation().tracks["layer-1"].cel_library.len(), 1);
        assert!(d.remove_animation_cel("layer-1", 1).is_err());
        d.assign_animation_cel("layer-1", 3, 1).unwrap();
        assert_eq!(d.animation().tracks["layer-1"].exposures[&3].cel, 1);
    }
    #[test]
    fn library_survives_without_exposures_and_serializes_each_drawing_once() {
        let mut d = document();
        d.duplicate_animation_cel("layer-1", 0, 10).unwrap();
        let encoded = serde_json::to_string(&d.document_state()).unwrap();
        assert_eq!(encoded.matches("fill=\\\"red").count(), 2); // editable artwork + one library drawing
        let restored =
            Document::from_document_state(serde_json::from_str(&encoded).unwrap()).unwrap();
        let t = &restored.animation().tracks["layer-1"];
        assert!(Arc::ptr_eq(&t.cels[&0], &t.cels[&10]));
        d.remove_animation_key("layer-1", 0, None).unwrap();
        d.remove_animation_key("layer-1", 10, None).unwrap();
        let mut p = d.clone();
        p.evaluate_animation(&d, 12);
        assert!(p.paint_source.is_none());
        d.rename_animation_cel("layer-1", 1, "歩き A".into())
            .unwrap();
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        let mut r = Document::from_document_state(serde_json::from_str(&saved).unwrap()).unwrap();
        assert_eq!(
            r.animation().tracks["layer-1"].cel_library[&1].name,
            "歩き A"
        );
        r.assign_animation_cel("layer-1", 4, 1).unwrap();
        r.remove_animation_key("layer-1", 4, None).unwrap();
        r.remove_animation_cel("layer-1", 1).unwrap();
        let encoded = serde_json::to_string(&r.document_state()).unwrap();
        let r = Document::from_document_state(serde_json::from_str(&encoded).unwrap()).unwrap();
        assert!(r.animation().tracks["layer-1"].pixel);
        assert!(r.animation().tracks["layer-1"].exposures.is_empty());
        let mut r = r;
        assert_eq!(r.create_animation_cel("layer-1").unwrap(), 2);
    }
    #[test]
    fn legacy_frames_migrate_and_invalid_reference_data_is_rejected() {
        let cel = Cel {
            source: None,
            strokes: vec![],
        };
        let legacy = serde_json::json!({"cels":[[0,cel],[10,cel]],"channels":{},"anchor":[0,0]});
        let t: Track = serde_json::from_value(legacy).unwrap();
        assert_eq!(t.exposures.len(), 2);
        assert_eq!(t.exposure_end(0, 120), 10);
        assert_eq!(t.exposure_end(10, 120), 120);
        assert!(!serde_json::to_value(&t)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("cels"));
        let mut wire = serde_json::to_value(&t).unwrap();
        wire["exposures"][0][1]["cel"] = serde_json::json!(99);
        let t: Track = serde_json::from_value(wire).unwrap();
        assert!(t.validate_exposures(120).is_err());
        let mut wire = serde_json::to_value(Track::default()).unwrap();
        wire["celLibrary"] =
            serde_json::json!([[1,{"name":"A","drawing":cel}],[1,{"name":"B","drawing":cel}]]);
        assert!(serde_json::from_value::<Track>(wire).is_err());
    }
}
