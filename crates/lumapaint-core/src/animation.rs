//! Layer animation data. Pixel exposures hold until the next cel; vector channels
//! are evaluated independently. Preview evaluation never changes the artwork.
use super::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Animation {
    pub fps: u32,
    pub duration: u32,
    pub looping: bool,
    pub tracks: BTreeMap<String, Track>,
}
impl Default for Animation {
    fn default() -> Self {
        Self {
            fps: 24,
            duration: 120,
            looping: true,
            tracks: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Track {
    #[serde(with = "cel_frames")]
    pub cels: BTreeMap<u32, Arc<Cel>>,
    pub channels: BTreeMap<Property, Vec<Keyframe>>,
    /// Explicit pivot in document pixels, fixed when the first key is created.
    pub anchor: [f32; 2],
}
// Array entries retain numeric frame types through the native envelope's serde
// flatten buffer (JSON object keys become strings in that representation).
mod cel_frames {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &BTreeMap<u32, Arc<Cel>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<u32, Arc<Cel>>, D::Error> {
        let entries = Vec::<(u32, Arc<Cel>)>::deserialize(deserializer)?;
        let mut result = BTreeMap::new();
        for (frame, cel) in entries {
            if result.len() >= 2048 || result.insert(frame, cel).is_some() {
                return Err(serde::de::Error::custom(
                    "Too many or duplicate animation cels",
                ));
            }
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cel {
    pub source: Option<String>,
    pub strokes: Vec<Stroke>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Property {
    X,
    Y,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
}
impl Property {
    pub fn base(self) -> f32 {
        match self {
            Self::ScaleX | Self::ScaleY | Self::Opacity => 100.,
            _ => 0.,
        }
    }
    fn valid(self, value: f32) -> bool {
        value.is_finite()
            && match self {
                Self::Opacity => (0. ..=100.).contains(&value),
                Self::ScaleX | Self::ScaleY => (0.1..=10000.).contains(&value),
                Self::Rotation => value.abs() <= 36000.,
                _ => value.abs() <= 65536.,
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Interpolation {
    Linear,
    Hold,
    Ease,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Keyframe {
    pub frame: u32,
    pub value: f32,
    pub interpolation: Interpolation,
}
impl Track {
    pub fn value(&self, property: Property, frame: u32) -> f32 {
        let Some(keys) = self.channels.get(&property).filter(|keys| !keys.is_empty()) else {
            return property.base();
        };
        let i = keys.partition_point(|k| k.frame <= frame);
        if i == 0 {
            return keys[0].value;
        }
        let a = &keys[i - 1];
        let Some(b) = keys.get(i) else {
            return a.value;
        };
        let t = (frame - a.frame) as f32 / (b.frame - a.frame) as f32;
        let t = match a.interpolation {
            Interpolation::Hold => 0.,
            Interpolation::Linear => t,
            Interpolation::Ease => t * t * (3. - 2. * t),
        };
        a.value + (b.value - a.value) * t
    }
}
impl Animation {
    /// Wall-clock playback skips overdue frames instead of accumulating timer drift.
    pub fn playback_frame(&self, first: u32, elapsed: std::time::Duration) -> (u32, bool) {
        let elapsed = (elapsed.as_secs_f64() * self.fps as f64).floor() as u64 + first as u64;
        if self.looping {
            ((elapsed % self.duration as u64) as u32, true)
        } else {
            (
                elapsed.min((self.duration - 1) as u64) as u32,
                elapsed < (self.duration - 1) as u64,
            )
        }
    }
    pub fn same_picture(&self, a: u32, b: u32) -> bool {
        self.tracks.values().all(|track| {
            let ca = track.cels.range(..=a).next_back().map(|(_, c)| c);
            let cb = track.cels.range(..=b).next_back().map(|(_, c)| c);
            let same_cel = match (ca, cb) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            same_cel
                && track
                    .channels
                    .keys()
                    .all(|p| track.value(*p, a) == track.value(*p, b))
        })
    }
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        self.validate_inner(width, height, true)
    }
    fn validate_inner(&self, width: u32, height: u32, validate_cels: bool) -> Result<(), String> {
        if !(1..=120).contains(&self.fps)
            || !(1..=18000).contains(&self.duration)
            || self.tracks.len() > 256
        {
            return Err("Invalid animation duration, frame rate or track count".into());
        }
        let mut bytes = 0usize;
        let mut frames = 0;
        let mut total_keys = 0;
        for (id, track) in &self.tracks {
            if id.is_empty()
                || id.len() > 256
                || track
                    .anchor
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 65536.)
                || (!track.cels.is_empty() && !track.channels.is_empty())
            {
                return Err("Invalid animation track".into());
            }
            for (frame, cel) in &track.cels {
                frames += 1;
                bytes = bytes.saturating_add(cel.source.as_ref().map_or(0, String::len));
                bytes = bytes.saturating_add(
                    cel.strokes
                        .iter()
                        .map(|s| s.points.len().saturating_mul(32))
                        .sum::<usize>(),
                );
                if *frame >= self.duration || frames > 2048 || bytes > 48 * 1024 * 1024 {
                    return Err("Animation cel budget exceeded".into());
                }
                // Reuse the normal document validator for all stored paint data.
                if validate_cels {
                    let mut state = Document::default().document_state();
                    state.width = width;
                    state.height = height;
                    state.strokes = cel.strokes.clone();
                    state.paint_source = cel.source.clone();
                    Document::from_document_state(state)?;
                }
            }
            for (property, keys) in &track.channels {
                total_keys += keys.len();
                if total_keys > 32768
                    || keys.len() > 4096
                    || keys.windows(2).any(|w| w[0].frame >= w[1].frame)
                    || keys
                        .iter()
                        .any(|k| k.frame >= self.duration || !property.valid(k.value))
                {
                    return Err("Invalid animation keyframe".into());
                }
            }
        }
        Ok(())
    }
}
impl Document {
    pub fn animation(&self) -> &Animation {
        &self.animation
    }
    fn commit_animation(&mut self, next: Animation) -> Result<(), String> {
        next.validate_inner(self.width, self.height, false)?;
        self.finish();
        let previous = std::mem::replace(&mut self.animation, Arc::new(next));
        self.vector_undo
            .push(VectorHistoryEntry::Animation(previous));
        self.vector_redo.clear();
        self.redo.clear();
        self.redo_order.clear();
        self.undo_order.push(HistoryKind::Vector);
        self.revision += 1;
        Ok(())
    }
    pub fn set_animation_settings(
        &mut self,
        fps: u32,
        duration: u32,
        looping: bool,
    ) -> Result<(), String> {
        let mut next = self.animation.as_ref().clone();
        next.fps = fps;
        next.duration = duration;
        next.looping = looping;
        // Reject truncation of authored data; expanding always preserves keys.
        self.commit_animation(next)
    }
    fn animation_layer(&self, id: &str, pixel: bool) -> Result<(), String> {
        let valid = if id == "layer-1" {
            pixel && !self.layer_locked
        } else {
            self.svg_layers.iter().any(|l| {
                l.id == id && !l.locked && if pixel { l.paint_layer } else { !l.paint_layer }
            })
        };
        if !valid || self.locked_artwork_layers.contains(id) {
            return Err("Select an unlocked compatible layer / 対応するロックされていないレイヤーを選択してください / 请选择未锁定的兼容图层".into());
        }
        Ok(())
    }
    pub fn capture_animation_cel(
        &mut self,
        id: &str,
        frame: u32,
        blank: bool,
    ) -> Result<(), String> {
        self.animation_layer(id, true)?;
        if self.has_active_stroke() {
            return Err("Finish the stroke before recording a cel".into());
        }
        let cel = if blank {
            Cel {
                source: None,
                strokes: vec![],
            }
        } else if id == "layer-1" {
            Cel {
                source: self.paint_source.clone(),
                strokes: self.strokes.clone(),
            }
        } else {
            Cel {
                source: self
                    .svg_layers
                    .iter()
                    .find(|l| l.id == id)
                    .map(|l| l.source.clone()),
                strokes: vec![],
            }
        };
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.entry(id.into()).or_default();
        if !track.channels.is_empty() {
            return Err("Layer already has vector animation".into());
        }
        track.cels.insert(frame, Arc::new(cel));
        self.commit_animation(next)
    }
    pub fn blank_animation_cel(&mut self, id: &str, frame: u32) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let cel = Arc::new(Cel {
            source: None,
            strokes: vec![],
        });
        let mut next = self.animation.as_ref().clone();
        next.tracks
            .entry(id.into())
            .or_default()
            .cels
            .insert(frame, cel.clone());
        next.validate_inner(self.width, self.height, false)?;
        self.finish();
        let mut before = self.vector_history_state();
        if id == "layer-1" {
            before.strokes = Some(self.strokes.clone());
        }
        self.animation = Arc::new(next);
        self.apply_cel(id, &cel);
        self.selected_layer = Some(id.into());
        self.layer_groups.selected = vec![id.into()];
        self.selected_vector_objects.clear();
        self.selection = None;
        self.record_vector_edit(before);
        Ok(())
    }
    pub fn duplicate_animation_cel(
        &mut self,
        id: &str,
        from: u32,
        frame: u32,
    ) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(id).ok_or("No cel to duplicate")?;
        let cel = track
            .cels
            .range(..=from)
            .next_back()
            .map(|(_, c)| c.clone())
            .ok_or("No cel to duplicate")?;
        track.cels.insert(frame, cel);
        self.commit_animation(next)
    }
    pub fn set_animation_key(
        &mut self,
        id: &str,
        property: Property,
        key: Keyframe,
    ) -> Result<(), String> {
        self.animation_layer(id, false)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.entry(id.into()).or_insert_with(|| Track {
            anchor: [self.width as f32 * 0.5, self.height as f32 * 0.5],
            ..Track::default()
        });
        if !track.cels.is_empty() {
            return Err("Layer already has pixel animation".into());
        }
        let keys = track.channels.entry(property).or_default();
        keys.retain(|k| k.frame != key.frame);
        keys.push(key);
        keys.sort_by_key(|k| k.frame);
        self.commit_animation(next)
    }
    pub fn move_animation_key(
        &mut self,
        id: &str,
        property: Property,
        from: u32,
        frame: u32,
    ) -> Result<(), String> {
        self.animation_layer(id, false)?;
        let mut next = self.animation.as_ref().clone();
        let keys = next
            .tracks
            .get_mut(id)
            .and_then(|t| t.channels.get_mut(&property))
            .ok_or("No keyframe")?;
        if keys.iter().any(|k| k.frame == frame) {
            return Err("A keyframe already exists at the destination / 移動先にはキーフレームがあります / 目标位置已有关键帧".into());
        }
        let key = keys
            .iter_mut()
            .find(|k| k.frame == from)
            .ok_or("No keyframe")?;
        key.frame = frame;
        keys.sort_by_key(|k| k.frame);
        self.commit_animation(next)
    }
    pub fn remove_animation_key(
        &mut self,
        id: &str,
        frame: u32,
        property: Option<Property>,
    ) -> Result<(), String> {
        self.animation_layer(id, property.is_none())?;
        let mut next = self.animation.as_ref().clone();
        if let Some(track) = next.tracks.get_mut(id) {
            if let Some(p) = property {
                if let Some(keys) = track.channels.get_mut(&p) {
                    keys.retain(|k| k.frame != frame);
                }
            } else {
                track.cels.remove(&frame);
            }
        }
        self.commit_animation(next)
    }
    /// Bring one recorded cel into the regular painting tools. Recording commits
    /// subsequent changes back to the timeline; the load itself is undoable.
    pub fn load_animation_cel(&mut self, id: &str, frame: u32) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let cel = self
            .animation
            .tracks
            .get(id)
            .and_then(|t| t.cels.range(..=frame).next_back())
            .map(|(_, c)| c.clone())
            .ok_or("No cel at this frame")?;
        self.finish();
        let mut before = self.vector_history_state();
        if id == "layer-1" {
            before.strokes = Some(self.strokes.clone());
        }
        self.apply_cel(id, &cel);
        self.selected_layer = Some(id.to_string());
        self.layer_groups.selected = vec![id.to_string()];
        self.selected_vector_objects.clear();
        self.selection = None;
        self.record_vector_edit(before);
        Ok(())
    }
    fn apply_cel(&mut self, id: &str, cel: &Cel) {
        if id == "layer-1" {
            self.paint_source = cel.source.clone();
            self.strokes = cel.strokes.clone();
            self.point_count = self.strokes.iter().map(|s| s.points.len()).sum();
            self.active = None;
        } else if let Some(layer) = self.svg_layers.iter_mut().find(|l| l.id == id) {
            layer.source = cel.source.clone().unwrap_or_else(|| {
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"></svg>",
                    self.width, self.height
                )
            });
        }
    }
    /// Update only animated layers in a retained preview document. Callers rebuild
    /// this preview when the source revision changes, never per display refresh.
    pub fn evaluate_animation(&mut self, source: &Document, frame: u32) {
        let frame = frame.min(source.animation.duration - 1);
        let previous_layers: Vec<_> = self
            .svg_layers
            .iter()
            .filter(|l| source.animation.tracks.contains_key(&l.id))
            .cloned()
            .collect();
        for (id, track) in &source.animation.tracks {
            if let Some((_, cel)) = track.cels.range(..=frame).next_back() {
                self.apply_cel(id, cel);
            } else if !track.cels.is_empty() {
                self.apply_cel(
                    id,
                    &Cel {
                        source: None,
                        strokes: vec![],
                    },
                );
            }
            if track.channels.is_empty() {
                continue;
            }
            let Some(original) = source
                .svg_layers
                .iter()
                .find(|l| &l.id == id && !l.paint_layer)
            else {
                continue;
            };
            let Some(layer) = self.svg_layers.iter_mut().find(|l| &l.id == id) else {
                continue;
            };
            *layer = original.clone();
            let x = track.value(Property::X, frame);
            let y = track.value(Property::Y, frame);
            let sx = track.value(Property::ScaleX, frame) / 100.;
            let sy = track.value(Property::ScaleY, frame) / 100.;
            let (sin, cos) = track
                .value(Property::Rotation, frame)
                .to_radians()
                .sin_cos();
            let [ax, ay] = track.anchor;
            let (a, b, c, d) = (cos * sx, sin * sx, -sin * sy, cos * sy);
            let (e, f) = (x + ax - a * ax - c * ay, y + ay - b * ax - d * ay);
            for object in &mut layer.vector_objects {
                let [oa, ob, oc, od, oe, of] = object.transform;
                object.transform = [
                    a * oa + c * ob,
                    b * oa + d * ob,
                    a * oc + c * od,
                    b * oc + d * od,
                    a * oe + c * of + e,
                    b * oe + d * of + f,
                ];
            }
            layer.opacity *= track.value(Property::Opacity, frame) / 100.;
            layer.source = if layer.vector_layer {
                vector_svg(self.width, self.height, &layer.vector_objects)
            } else {
                let content = original
                    .source
                    .find("<svg")
                    .map(|i| &original.source[i..])
                    .unwrap_or(&original.source);
                format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><g transform=\"matrix({a} {b} {c} {d} {e} {f})\">{content}</g></svg>",self.width,self.height)
            };
        }
        self.selected_vector_objects.clear();
        self.selection = None;
        for previous in &previous_layers {
            if let Some(layer) = self.svg_layers.iter().find(|l| l.id == previous.id) {
                self.scene_journal
                    .layers_changed(std::slice::from_ref(previous), std::slice::from_ref(layer));
            }
        }
        self.revision = self.revision.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(frame: u32, value: f32, interpolation: Interpolation) -> Keyframe {
        Keyframe {
            frame,
            value,
            interpolation,
        }
    }
    #[test]
    fn clock_uses_elapsed_time_and_holds_last_frame_without_loop() {
        let mut animation = Animation {
            fps: 24,
            duration: 48,
            ..Default::default()
        };
        assert_eq!(
            animation.playback_frame(12, std::time::Duration::from_millis(500)),
            (24, true)
        );
        assert_eq!(
            animation.playback_frame(0, std::time::Duration::from_secs(3)),
            (24, true)
        );
        animation.looping = false;
        assert_eq!(
            animation.playback_frame(0, std::time::Duration::from_secs(3)),
            (47, false)
        );
        assert!(animation.same_picture(0, 20));
    }
    #[test]
    fn independent_channels_interpolate_hold_and_ease_without_drift() {
        let mut track = Track::default();
        for mode in [
            Interpolation::Linear,
            Interpolation::Hold,
            Interpolation::Ease,
        ] {
            track.channels.insert(
                Property::X,
                vec![key(0, 0., mode), key(20, 100., Interpolation::Linear)],
            );
            assert_eq!(track.value(Property::X, 0), 0.);
            assert_eq!(track.value(Property::X, 20), 100.);
            assert_eq!(track.value(Property::X, 100), 100.);
            let expected = match mode {
                Interpolation::Linear => 25.,
                Interpolation::Hold => 0.,
                Interpolation::Ease => 15.625,
            };
            assert_eq!(track.value(Property::X, 5), expected);
            assert_eq!(track.value(Property::Opacity, 5), 100.);
        }
    }
    #[test]
    fn cels_hold_blank_duplicate_and_load_are_undoable_and_persisted() {
        let mut d = Document {paint_source:Some("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"480\"><rect width=\"20\" height=\"20\" fill=\"red\"/></svg>".into()), ..Default::default() };
        d.capture_animation_cel("layer-1", 0, false).unwrap();
        d.capture_animation_cel("layer-1", 10, true).unwrap();
        d.duplicate_animation_cel("layer-1", 0, 20).unwrap();
        let encoded = serde_json::to_string(&d.document_state()).unwrap();
        let mut restored =
            Document::from_document_state(serde_json::from_str(&encoded).unwrap()).unwrap();
        assert_eq!(restored.animation.tracks["layer-1"].cels.len(), 3);
        let mut preview = restored.clone();
        preview.evaluate_animation(&restored, 9);
        assert_eq!(preview.paint_source, restored.paint_source);
        preview.evaluate_animation(&restored, 10);
        assert!(preview.paint_source.is_none());
        preview.evaluate_animation(&restored, 25);
        assert_eq!(preview.paint_source, restored.paint_source);
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), encoded);
        restored.load_animation_cel("layer-1", 10).unwrap();
        assert!(restored.paint_source.is_none());
        restored.undo();
        assert!(restored.paint_source.is_some());
        restored.redo();
        assert!(restored.paint_source.is_none());
        d.undo();
        assert_eq!(d.animation.tracks["layer-1"].cels.len(), 2);
        d.redo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), encoded);
    }
    #[test]
    fn rejected_animation_edits_leave_document_history_and_revision_intact() {
        let mut d = Document::default();
        d.capture_animation_cel("layer-1", 20, false).unwrap();
        let before = serde_json::to_string(&d.document_state()).unwrap();
        let revision = d.revision();
        let undo = d.undo_order.len();
        assert!(d.set_animation_settings(24, 10, true).is_err());
        assert!(d.capture_animation_cel("layer-1", 18000, false).is_err());
        assert!(d
            .set_animation_key("layer-1", Property::X, key(0, 10., Interpolation::Linear))
            .is_err());
        assert_eq!(d.revision(), revision);
        assert_eq!(d.undo_order.len(), undo);
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
    }
    #[test]
    fn vector_key_edit_move_and_history_round_trip() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        d.set_animation_key(&id, Property::X, key(0, 0., Interpolation::Linear))
            .unwrap();
        d.set_animation_key(&id, Property::X, key(20, 100., Interpolation::Linear))
            .unwrap();
        d.move_animation_key(&id, Property::X, 20, 30).unwrap();
        assert_eq!(d.animation.tracks[&id].channels[&Property::X][1].frame, 30);
        d.undo();
        assert_eq!(d.animation.tracks[&id].channels[&Property::X][1].frame, 20);
        d.redo();
        assert_eq!(d.animation.tracks[&id].channels[&Property::X][1].frame, 30);
        let before = serde_json::to_string(&d.document_state()).unwrap();
        assert!(d.move_animation_key(&id, Property::X, 30, 0).is_err());
        assert!(d
            .set_animation_key(&id, Property::ScaleX, key(0, -1., Interpolation::Linear))
            .is_err());
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
        d.remove_animation_key(&id, 30, Some(Property::X)).unwrap();
        assert_eq!(d.animation.tracks[&id].channels[&Property::X].len(), 1);
        d.undo();
        assert_eq!(d.animation.tracks[&id].channels[&Property::X].len(), 2);
    }
    #[test]
    fn legacy_files_default_to_an_empty_timeline_and_invalid_cels_are_rejected() {
        let mut value = serde_json::to_value(Document::default().document_state()).unwrap();
        value.as_object_mut().unwrap().remove("animation");
        let mut d = Document::from_document_state(serde_json::from_value(value).unwrap()).unwrap();
        assert_eq!(d.animation.fps, 24);
        assert!(d.animation.tracks.is_empty());
        d.capture_animation_cel("layer-1", 0, false).unwrap();
        let mut state = d.document_state();
        state
            .animation
            .tracks
            .get_mut("layer-1")
            .unwrap()
            .cels
            .insert(
                1,
                Arc::new(Cel {
                    source: Some("invalid".into()),
                    strokes: vec![],
                }),
            );
        assert!(Document::from_document_state(state).is_err());
    }
}
