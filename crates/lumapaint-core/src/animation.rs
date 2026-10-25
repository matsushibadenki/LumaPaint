//! Layer animation data. Pixel exposures hold until the next cel; vector channels
//! are evaluated independently. Preview evaluation never changes the artwork.
use super::*;
use std::{collections::BTreeMap, sync::Arc};
#[path = "animation/exposures.rs"]
mod exposures;
pub use exposures::{Exposure, NamedCel};
#[path = "animation/exposure_batch.rs"]
mod exposure_batch;
pub use exposure_batch::{ExposureBatchEdit, ExposureTrackEdit};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Animation {
    pub fps: u32,
    pub duration: u32,
    pub looping: bool,
    #[serde(default)]
    pub onion: OnionSettings,
    pub tracks: BTreeMap<String, Track>,
}
impl Default for Animation {
    fn default() -> Self {
        Self {
            fps: 24,
            duration: 120,
            looping: true,
            onion: OnionSettings::default(),
            tracks: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnionSpacing {
    #[default]
    Cels,
    Frames,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnionColor {
    Original,
    Half,
    #[default]
    Tint,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnionScope {
    #[default]
    Layer,
    Scene,
}
/// Authored viewing preferences. Reference pixels are never stored in a project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnionSettings {
    #[serde(default)]
    pub scope: OnionScope,
    pub enabled: bool,
    pub before: u32,
    pub after: u32,
    pub step: u32,
    pub opacity: f32,
    pub fade: f32,
    pub previous_color: [u8; 3],
    pub next_color: [u8; 3],
    pub spacing: OnionSpacing,
    pub color: OnionColor,
    pub fixed_frames: Vec<u32>,
}
impl Default for OnionSettings {
    fn default() -> Self {
        Self {
            scope: OnionScope::Layer,
            enabled: false,
            before: 2,
            after: 2,
            step: 1,
            opacity: 35.,
            fade: 10.,
            previous_color: [65, 145, 255],
            next_color: [255, 90, 105],
            spacing: OnionSpacing::Cels,
            color: OnionColor::Tint,
            fixed_frames: vec![],
        }
    }
}
impl OnionSettings {
    /// Scene boundaries are the union of pixel exposure starts and blank ends.
    /// Continuously animated vector scenes use frame spacing, since each frame
    /// can have a different picture. No duration-sized frame table is allocated.
    pub fn scene_references(&self, animation: &Animation, frame: u32) -> Vec<SceneReference> {
        if !self.enabled || self.validate(animation.duration).is_err() {
            return vec![];
        }
        let continuous = animation
            .tracks
            .values()
            .any(|t| !t.channels.is_empty() || !t.motions.is_empty());
        let boundaries: std::collections::BTreeSet<_> = std::iter::once(0)
            .chain(
                animation
                    .tracks
                    .values()
                    .flat_map(|t| t.cels.keys().copied()),
            )
            .collect();
        let frames: Vec<_> = boundaries.into_iter().collect();
        let insertion = frames.partition_point(|f| *f <= frame);
        let mut targets = vec![];
        for (before, count) in [(true, self.before), (false, self.after)] {
            for distance in 1..=count {
                let offset = distance * self.step;
                let target = if self.spacing == OnionSpacing::Frames || continuous {
                    if before {
                        frame.checked_sub(offset)
                    } else {
                        frame
                            .checked_add(offset)
                            .filter(|f| *f < animation.duration)
                    }
                } else if before {
                    insertion
                        .checked_sub(1 + offset as usize)
                        .and_then(|i| frames.get(i).copied())
                } else {
                    frames.get(insertion + offset as usize - 1).copied()
                };
                if let Some(target) = target {
                    targets.push((target, before, distance));
                }
            }
        }
        targets.extend(
            self.fixed_frames
                .iter()
                .copied()
                .filter(|f| *f != frame)
                .map(|f| (f, f < frame, 1)),
        );
        let mut result: Vec<SceneReference> = vec![];
        for (target, previous, distance) in targets {
            let opacity = (self.opacity - self.fade * (distance - 1) as f32).max(0.) / 100.;
            if let Some(existing) = result.iter_mut().find(|r| r.frame == target) {
                existing.opacity = existing.opacity.max(opacity);
            } else if opacity > 0. {
                result.push(SceneReference {
                    frame: target,
                    opacity,
                    color: if previous {
                        self.previous_color
                    } else {
                        self.next_color
                    },
                });
            }
        }
        result.sort_by(|a, b| a.opacity.total_cmp(&b.opacity));
        result
    }
    pub fn validate(&self, duration: u32) -> Result<(), String> {
        if self.before > 4
            || self.after > 4
            || !(1..=24).contains(&self.step)
            || !self.opacity.is_finite()
            || !(0. ..=100.).contains(&self.opacity)
            || !self.fade.is_finite()
            || !(0. ..=100.).contains(&self.fade)
            || self.fixed_frames.len() > 4
            || self.fixed_frames.iter().any(|f| *f >= duration)
            || self.fixed_frames.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("Invalid onion skin settings / オニオンスキンの設定範囲が不正です / 洋葱皮设置超出范围".into());
        }
        Ok(())
    }
    /// Nearest distinct exposures by default; relative frame and fixed references
    /// share the same held-cel lookup and never wrap across the timeline edges.
    pub fn references(&self, track: &Track, frame: u32, duration: u32) -> Vec<OnionReference> {
        if !self.enabled || self.validate(duration).is_err() {
            return vec![];
        }
        let current = track.cels.range(..=frame).next_back().map(|(_, c)| c);
        let frames: Vec<_> = track.cels.keys().copied().collect();
        let insertion = frames.partition_point(|f| *f <= frame);
        let mut targets = vec![];
        for (before, count) in [(true, self.before), (false, self.after)] {
            for distance in 1..=count {
                let offset = distance * self.step;
                let target = match self.spacing {
                    OnionSpacing::Frames => {
                        if before {
                            frame.checked_sub(offset)
                        } else {
                            frame.checked_add(offset).filter(|f| *f < duration)
                        }
                    }
                    OnionSpacing::Cels => {
                        if before {
                            insertion
                                .checked_sub(usize::from(current.is_some()) + offset as usize)
                                .and_then(|i| frames.get(i).copied())
                        } else {
                            frames.get(insertion + offset as usize - 1).copied()
                        }
                    }
                };
                if let Some(target) = target {
                    targets.push((target, before, distance));
                }
            }
        }
        targets.extend(self.fixed_frames.iter().copied().map(|f| (f, f < frame, 1)));
        let mut result: Vec<OnionReference> = vec![];
        for (target, previous, distance) in targets {
            let Some((start, cel)) = track.cels.range(..=target).next_back() else {
                continue;
            };
            if current.is_some_and(|c| Arc::ptr_eq(c, cel))
                || (cel.source.is_none() && cel.strokes.is_empty())
            {
                continue;
            }
            let opacity = (self.opacity - self.fade * (distance - 1) as f32).max(0.) / 100.;
            if let Some(existing) = result.iter_mut().find(|r| Arc::ptr_eq(&r.cel, cel)) {
                if opacity > existing.opacity {
                    existing.opacity = opacity;
                }
                continue;
            }
            if opacity > 0. {
                result.push(OnionReference {
                    frame: *start,
                    cel: cel.clone(),
                    opacity,
                    color: if previous {
                        self.previous_color
                    } else {
                        self.next_color
                    },
                });
            }
        }
        // Draw nearer skins last. Stable order preserves independently colored sides.
        result.sort_by(|a, b| a.opacity.total_cmp(&b.opacity));
        result
    }
}
#[derive(Clone, Debug)]
pub struct SceneReference {
    pub frame: u32,
    pub opacity: f32,
    pub color: [u8; 3],
}
#[derive(Clone, Debug)]
pub struct OnionReference {
    pub frame: u32,
    pub cel: Arc<Cel>,
    pub opacity: f32,
    pub color: [u8; 3],
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(try_from = "exposures::TrackData", into = "exposures::TrackData")]
pub struct Track {
    /// Resolved playback cache; only the library and exposure references are saved.
    pub cels: BTreeMap<u32, Arc<Cel>>,
    pub pixel: bool,
    pub next_cel_id: u32,
    pub cel_library: BTreeMap<u32, NamedCel>,
    pub exposures: BTreeMap<u32, Exposure>,
    pub channels: BTreeMap<Property, Vec<Keyframe>>,
    /// Explicit pivot in document pixels, fixed when the first key is created.
    pub anchor: [f32; 2],
    pub motions: Vec<crate::motion::Clip>,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvaluationStats {
    pub evaluated_tracks: usize,
    pub reused_tracks: usize,
}
impl Track {
    pub fn same_picture(&self, a: u32, b: u32) -> bool {
        let ca = self.cels.range(..=a).next_back().map(|(_, c)| c);
        let cb = self.cels.range(..=b).next_back().map(|(_, c)| c);
        let same_cel = match (ca, cb) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_cel
            // Active effects stay conservative: text scrambling can use the
            // absolute frame even when the clip's normalized progress holds.
            && self.motions.iter().all(|m| !m.enabled || a == b || (a < m.start && b < m.start))
            && self.channels.keys().all(|p| self.value(*p, a) == self.value(*p, b))
    }
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
        self.tracks.values().all(|track| track.same_picture(a, b))
    }

    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        self.validate_inner(width, height, true)
    }
    fn validate_inner(&self, width: u32, height: u32, validate_cels: bool) -> Result<(), String> {
        self.onion.validate(self.duration)?;
        if !(1..=120).contains(&self.fps)
            || !(1..=18000).contains(&self.duration)
            || self.tracks.len() > 256
        {
            return Err("Invalid animation duration, frame rate or track count".into());
        }
        let mut bytes = 0usize;
        let mut frames = 0;
        let mut exposures = 0;
        let mut total_keys = 0;
        let mut total_motions = 0;
        let mut motion_bytes = 0usize;
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
            track.validate_exposures(self.duration)?;
            if validate_cels {
                track.validate_playback_cache()?;
            }
            exposures += track.exposures.len();
            if exposures > 2048 {
                return Err("Exposure budget exceeded".into());
            }
            for (frame, cel) in track.stored_cels() {
                frames += 1;
                bytes = bytes.saturating_add(cel.source.as_ref().map_or(0, String::len));
                bytes = bytes.saturating_add(
                    cel.strokes
                        .iter()
                        .map(|s| s.points.len().saturating_mul(32))
                        .sum::<usize>(),
                );
                if frame >= 2048 || frames > 2048 || bytes > 48 * 1024 * 1024 {
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
            total_motions += track.motions.len();
            if track
                .motions
                .iter()
                .filter(|m| {
                    m.enabled
                        && crate::motion::preset(&m.preset).is_some_and(|p| {
                            p.operations.contains(&crate::motion::Operation::Repeat)
                        })
                })
                .fold(1u64, |total, clip| {
                    total.saturating_mul(clip.copies as u64 + 1)
                })
                > 125
            {
                return Err("Trail expansion exceeds the layer budget / 残像の展開数が上限を超えます / 残影展开超过图层预算".into());
            }
            if track.motions.len() > 64
                || total_motions > 512
                || (!track.cels.is_empty() && !track.motions.is_empty())
            {
                return Err("Motion clip budget exceeded".into());
            }
            let mut ids = std::collections::BTreeSet::new();
            for motion in &track.motions {
                motion.validate(self.duration)?;
                motion_bytes = motion_bytes.saturating_add(motion.bytes());
                if motion_bytes > 2 * 1024 * 1024 {
                    return Err("Motion parameter budget exceeded".into());
                }
                if !ids.insert(&motion.id) {
                    return Err("Duplicate motion clip".into());
                }
            }
        }
        Ok(())
    }
}
impl Document {
    pub fn set_motion_clip(
        &mut self,
        layer_id: &str,
        clip: crate::motion::Clip,
    ) -> Result<(), String> {
        self.animation_layer(layer_id, false)?;
        clip.validate(18000)?;
        if let Some(layer) = self.svg_layers.iter().find(|l| l.id == layer_id) {
            if crate::motion::preset(&clip.preset).is_some_and(|p| {
                p.operations
                    .contains(&crate::motion::Operation::KanjiStroke)
            }) {
                let contents = layer
                    .vector_objects
                    .iter()
                    .filter(|o| clip.objects.is_empty() || clip.objects.contains(&o.id))
                    .filter_map(|o| o.text.as_ref())
                    .map(|text| text.content.as_str())
                    .collect::<Vec<_>>();
                let chars = contents
                    .iter()
                    .flat_map(|text| text.chars())
                    .filter(|c| crate::motion::kanji::is_cjk(*c))
                    .collect::<Vec<_>>();
                let unsupported_variant = contents.iter().any(|text| {
                    text.chars().any(|c| {
                        ('\u{fe00}'..='\u{fe0f}').contains(&c)
                            || ('\u{e0100}'..='\u{e01ef}').contains(&c)
                    })
                });
                if unsupported_variant
                    || chars.is_empty()
                    || chars.len() > 128
                    || chars.iter().any(|c| !crate::motion::kanji::contains(*c))
                {
                    return Err("Select up to 128 kanji with stroke data / 筆順データのある漢字を128字以内で選択してください / 请选择最多128个有笔顺数据的汉字".into());
                }
            }
            if !clip.morph_target.is_empty() {
                let count = crate::bezier::cubic_contours(&clip.morph_target)?.len();
                if !layer.vector_layer
                    || layer
                        .vector_objects
                        .iter()
                        .filter(|o| clip.objects.is_empty() || clip.objects.contains(&o.id))
                        .any(|o| {
                            o.text.is_some()
                                || o.image_frame.is_some()
                                || crate::bezier::cubic_contours(&o.path.data)
                                    .map_or(true, |p| p.len() != count)
                        })
                {
                    return Err("Select editable paths with matching contour counts / 同じ輪郭数の編集可能なパスを選択してください / 请选择轮廓数量相同的可编辑路径".into());
                }
            }
            if clip
                .objects
                .iter()
                .any(|id| !layer.vector_objects.iter().any(|o| &o.id == id))
            {
                return Err("Motion target no longer exists".into());
            }
            if !clip.objects.is_empty()
                && (!layer.vector_layer
                    || layer
                        .vector_objects
                        .iter()
                        .any(|o| o.clipping_group.is_some()))
            {
                return Err("Use the whole layer for retained SVG or clipping groups / SVG・クリッピンググループはレイヤー全体を対象にしてください / SVG及剪切组请选择整个图层".into());
            }
            if crate::motion::preset(&clip.preset).is_some_and(|p| {
                p.category.starts_with("tx-")
                    || p.operations.iter().any(|op| {
                        matches!(
                            op,
                            crate::motion::Operation::Counter
                                | crate::motion::Operation::FontAxes
                                | crate::motion::Operation::TextSwap
                                | crate::motion::Operation::Typewriter
                                | crate::motion::Operation::Scramble
                        )
                    })
            }) && !layer.vector_objects.iter().any(|o| {
                o.text.is_some() && (clip.objects.is_empty() || clip.objects.contains(&o.id))
            }) {
                return Err("Select editable text for a text motion / 文字モーションの対象に編集可能な文字を選択してください / 文字动效请选择可编辑文字".into());
            }
        }
        let mut next = self.animation.as_ref().clone();
        next.duration = next.duration.max(clip.start + clip.duration * clip.repeats);
        let track = next.tracks.entry(layer_id.into()).or_insert_with(|| Track {
            anchor: [self.width as f32 * 0.5, self.height as f32 * 0.5],
            ..Default::default()
        });
        if !track.cels.is_empty() {
            return Err("Motion requires a vector track".into());
        }
        if let Some(old) = track.motions.iter_mut().find(|m| m.id == clip.id) {
            *old = clip;
        } else {
            track.motions.push(clip);
        }
        if let Some(layer) = self.svg_layers.iter().find(|l| l.id == layer_id) {
            let overhead = track
                .motions
                .iter()
                .filter(|m| m.enabled)
                .map(|m| 16_384usize.saturating_add(m.particle_count as usize * 256))
                .sum::<usize>();
            let objects = layer.vector_objects.len().max(1);
            if layer
                .source
                .len()
                .saturating_add(overhead.saturating_mul(objects))
                > 4 * 1024 * 1024
            {
                return Err("Motion preview exceeds the layer budget / モーションの描画量が上限を超えます / 动效预览超过图层预算".into());
            }
        }
        self.commit_animation(next)
    }
    pub fn remove_motion_clip(&mut self, layer_id: &str, clip_id: &str) -> Result<(), String> {
        self.animation_layer(layer_id, false)?;
        let mut next = self.animation.as_ref().clone();
        if let Some(track) = next.tracks.get_mut(layer_id) {
            track.motions.retain(|m| m.id != clip_id);
        }
        self.commit_animation(next)
    }
    pub fn move_motion_clip(
        &mut self,
        layer_id: &str,
        clip_id: &str,
        position: usize,
    ) -> Result<(), String> {
        self.animation_layer(layer_id, false)?;
        let mut next = self.animation.as_ref().clone();
        let track = next.tracks.get_mut(layer_id).ok_or("No motion track")?;
        if position >= track.motions.len() {
            return Err("Invalid motion order".into());
        }
        let old = track
            .motions
            .iter()
            .position(|m| m.id == clip_id)
            .ok_or("No motion clip")?;
        let clip = track.motions.remove(old);
        track.motions.insert(position, clip);
        self.commit_animation(next)
    }
    pub fn animation(&self) -> &Animation {
        &self.animation
    }
    fn commit_animation(&mut self, mut next: Animation) -> Result<(), String> {
        for track in next.tracks.values_mut() {
            track.refresh_exposures(next.duration)?;
        }
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
    pub fn set_animation_onion(&mut self, onion: OnionSettings) -> Result<(), String> {
        onion.validate(self.animation.duration)?;
        if self.animation.onion == onion {
            return Ok(());
        }
        let mut next = self.animation.as_ref().clone();
        next.onion = onion;
        self.commit_animation(next)
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
        let reuse = !blank && track.exposures.contains_key(&frame);
        track.record_cel(frame, Arc::new(cel), next.duration, reuse)?;
        self.commit_animation(next)
    }
    pub fn blank_animation_cel(&mut self, id: &str, frame: u32) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let cel = Arc::new(Cel {
            source: None,
            strokes: vec![],
        });
        let mut next = self.animation.as_ref().clone();
        next.tracks.entry(id.into()).or_default().record_cel(
            frame,
            cel.clone(),
            next.duration,
            false,
        )?;
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
        track.refresh_exposures(next.duration)?;
        let cel_id = track
            .exposure_at(from, next.duration)
            .ok_or("No cel to duplicate")?
            .1
            .cel;
        let _ = cel;
        track.assign_cel(frame, cel_id, next.duration)?;
        self.commit_animation(next)
    }
    pub(super) fn filter_animation_keys(
        &mut self,
        id: &str,
        interpolation: Interpolation,
    ) -> Result<(), String> {
        self.animation_layer(id, false)?;
        let track=self.animation.tracks.get(id).filter(|t|t.channels.values().any(|keys|!keys.is_empty()))
            .ok_or("No keyframes on this layer / このレイヤーにはキーフレームがありません / 此图层没有关键帧")?;
        if track
            .channels
            .values()
            .flatten()
            .all(|k| k.interpolation == interpolation)
        {
            return Ok(());
        }
        let mut next = self.animation.as_ref().clone();
        for key in next
            .tracks
            .get_mut(id)
            .unwrap()
            .channels
            .values_mut()
            .flatten()
        {
            key.interpolation = interpolation;
        }
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
                track.refresh_exposures(next.duration)?;
                track.exposures.remove(&frame);
                track.rebuild_cels();
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
            .and_then(|t| {
                t.exposure_at(frame, self.animation.duration)
                    .and_then(|(_, e)| t.cel_library.get(&e.cel))
            })
            .map(|c| c.drawing.clone())
            .ok_or("No cel at this frame")?;
        self.load_cel_drawing(id, &cel)
    }
    /// Load a drawing by identity without adding or changing any exposure.
    pub fn load_animation_library_cel(&mut self, id: &str, cel: u32) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let drawing = self
            .animation
            .tracks
            .get(id)
            .and_then(|t| t.cel_library.get(&cel))
            .map(|c| c.drawing.clone())
            .ok_or("Cel is no longer available")?;
        self.load_cel_drawing(id, &drawing)
    }
    /// Native previews retain drawing data in Rust, including unfinished edits.
    pub fn animation_drawing(&self, id: &str) -> Result<Arc<Cel>, String> {
        self.animation_layer(id, true)?;
        Ok(Arc::new(if id == "layer-1" {
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
        }))
    }
    pub fn record_animation_library_cel(&mut self, id: &str, cel: u32) -> Result<(), String> {
        if self.has_active_stroke() {
            return Err("Finish the stroke before recording a cel".into());
        }
        let drawing = self.animation_drawing(id)?;
        let mut next = self.animation.as_ref().clone();
        next.tracks
            .get_mut(id)
            .and_then(|t| t.cel_library.get_mut(&cel))
            .ok_or("Cel is no longer available")?
            .drawing = drawing;
        self.commit_animation(next)
    }
    /// Reapply a library history change after Undo/Redo without adding history.
    pub fn restore_animation_library_drawing(&mut self, id: &str, cel: u32) -> Result<(), String> {
        self.animation_layer(id, true)?;
        let drawing = self
            .animation
            .tracks
            .get(id)
            .and_then(|t| t.cel_library.get(&cel))
            .map(|c| c.drawing.clone())
            .ok_or("Cel is no longer available")?;
        self.apply_cel(id, &drawing);
        Ok(())
    }
    fn load_cel_drawing(&mut self, id: &str, cel: &Cel) -> Result<(), String> {
        self.finish();
        let mut before = self.vector_history_state();
        if id == "layer-1" {
            before.strokes = Some(self.strokes.clone());
        }
        self.apply_cel(id, cel);
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
        self.evaluate_animation_with_outlines(source, frame, None);
    }
    pub fn evaluate_animation_with_outlines(
        &mut self,
        source: &Document,
        frame: u32,
        outlines: Option<&crate::motion::Outline<'_>>,
    ) {
        self.evaluate_animation_incremental_with_outlines(source, frame, None, outlines);
    }
    /// Reuses layers from a previously evaluated frame of the SAME source revision.
    /// Pass None after source edits, document/page switches, or preview recreation.
    /// This hint is runtime-only and never becomes authored or saved animation data.
    pub fn evaluate_animation_incremental_with_outlines(
        &mut self,
        source: &Document,
        frame: u32,
        previous_frame: Option<u32>,
        outlines: Option<&crate::motion::Outline<'_>>,
    ) -> EvaluationStats {
        let frame = frame.min(source.animation.duration - 1);
        let previous_frame = previous_frame.map(|f| f.min(source.animation.duration - 1));
        let mut stats = EvaluationStats::default();
        let changed = |track: &Track| {
            previous_frame.is_none_or(|previous| !track.same_picture(previous, frame))
        };
        let previous_layers: Vec<_> = self
            .svg_layers
            .iter()
            .filter(|l| source.animation.tracks.get(&l.id).is_some_and(changed))
            .cloned()
            .collect();
        for (id, track) in &source.animation.tracks {
            if !changed(track) {
                stats.reused_tracks += 1;
                continue;
            }
            stats.evaluated_tracks += 1;
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
            if track.channels.is_empty() && track.motions.is_empty() {
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
            crate::motion::apply(
                layer,
                &track.motions,
                frame,
                source.animation.fps,
                [self.width, self.height],
                outlines,
            );
        }
        self.selected_vector_objects.clear();
        self.selection = None;
        for previous in &previous_layers {
            if let Some(layer) = self.svg_layers.iter().find(|l| l.id == previous.id) {
                self.scene_journal
                    .layers_changed(std::slice::from_ref(previous), std::slice::from_ref(layer));
            }
        }
        if previous_frame.is_none() || stats.evaluated_tracks > 0 {
            self.revision = self.revision.saturating_add(1);
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ink() -> Arc<Cel> {
        Arc::new(Cel { source:Some("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"4\" y=\"4\" width=\"8\" height=\"8\"/></svg>".into()), strokes:vec![] })
    }
    #[test]
    fn scene_references_merge_tracks_keep_fixed_frames_and_bound_continuous_animation() {
        let mut animation = Animation {
            duration: 24,
            ..Default::default()
        };
        for (id, frames) in [("a", vec![0, 6, 12]), ("b", vec![2, 8, 14])] {
            let mut track = Track::default();
            for frame in frames {
                track.cels.insert(frame, ink());
            }
            animation.tracks.insert(id.into(), track);
        }
        let settings = OnionSettings {
            enabled: true,
            scope: OnionScope::Scene,
            fixed_frames: vec![7, 9],
            ..Default::default()
        };
        let refs = settings.scene_references(&animation, 9);
        let mut frames: Vec<_> = refs.iter().map(|r| r.frame).collect();
        frames.sort();
        assert_eq!(frames, [2, 6, 7, 12, 14]);
        assert_eq!(
            refs.iter().find(|r| r.frame == 7).unwrap().color,
            settings.previous_color
        );
        animation
            .tracks
            .get_mut("a")
            .unwrap()
            .channels
            .insert(Property::X, vec![]);
        let mut frames: Vec<_> = settings
            .scene_references(&animation, 0)
            .iter()
            .map(|r| r.frame)
            .collect();
        frames.sort();
        assert_eq!(frames, [1, 2, 7, 9]);
        let mut json = serde_json::to_value(&settings).unwrap();
        json.as_object_mut().unwrap().remove("scope");
        assert_eq!(
            serde_json::from_value::<OnionSettings>(json).unwrap().scope,
            OnionScope::Layer
        );
    }
    #[test]
    fn onion_neighbors_use_distinct_exposures_without_repeating_holds() {
        let mut track = Track::default();
        for frame in [0, 3, 6, 9, 12] {
            track.cels.insert(frame, ink());
        }
        let settings = OnionSettings {
            enabled: true,
            opacity: 50.,
            fade: 10.,
            ..Default::default()
        };
        let refs = settings.references(&track, 7, 24);
        assert_eq!(
            refs.iter().map(|r| r.frame).collect::<Vec<_>>(),
            [0, 12, 3, 9]
        );
        assert_eq!(
            refs.iter().map(|r| r.opacity).collect::<Vec<_>>(),
            [0.4, 0.4, 0.5, 0.5]
        );
        assert_eq!(refs[2].color, settings.previous_color);
        assert_eq!(refs[3].color, settings.next_color);
        assert!(settings
            .references(&track, 0, 24)
            .iter()
            .all(|r| r.frame > 0));
        let first = track.cels[&0].clone();
        track.cels.insert(3, first);
        assert_eq!(
            settings
                .references(&track, 7, 24)
                .iter()
                .filter(|r| r.frame <= 3)
                .count(),
            1
        );
        track.cels.insert(
            9,
            Arc::new(Cel {
                source: None,
                strokes: vec![],
            }),
        );
        assert!(!settings
            .references(&track, 7, 24)
            .iter()
            .any(|r| r.frame == 9));
    }
    #[test]
    fn onion_relative_frames_fixed_references_and_settings_roundtrip() {
        let mut track = Track::default();
        for frame in [0, 5, 10, 20] {
            track.cels.insert(frame, ink());
        }
        let settings = OnionSettings {
            enabled: true,
            spacing: OnionSpacing::Frames,
            before: 4,
            after: 4,
            fixed_frames: vec![0, 20],
            ..Default::default()
        };
        let refs = settings.references(&track, 7, 24);
        assert_eq!(refs.len(), 3);
        assert!(refs.iter().any(|r| r.frame == 20));
        assert!(refs.iter().all(|r| r.frame != 5));
        assert_eq!(
            refs.iter().find(|r| r.frame == 0).unwrap().opacity,
            settings.opacity / 100.,
            "Fixed references keep their full configured opacity"
        );
        let mut doc = Document::default();
        doc.set_animation_onion(settings.clone()).unwrap();
        let encoded = serde_json::to_value(doc.document_state()).unwrap();
        let restored =
            Document::from_document_state(serde_json::from_value(encoded.clone()).unwrap())
                .unwrap();
        assert_eq!(restored.animation().onion, settings);
        doc.undo();
        assert_eq!(doc.animation().onion, OnionSettings::default());
        doc.redo();
        assert_eq!(doc.animation().onion, settings);
        let mut legacy = encoded;
        legacy["animation"].as_object_mut().unwrap().remove("onion");
        assert_eq!(
            Document::from_document_state(serde_json::from_value(legacy).unwrap())
                .unwrap()
                .animation()
                .onion,
            OnionSettings::default()
        );
        let revision = doc.revision();
        let mut invalid = settings;
        invalid.opacity = f32::NAN;
        assert!(doc.set_animation_onion(invalid).is_err());
        assert_eq!(doc.revision(), revision);
    }
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
    #[ignore = "isolated Release evaluator benchmark; opt in explicitly"]
    fn benchmark_incremental_timeline_evaluation() {
        use std::time::Instant;
        let mut doc = Document::default();
        let mut ids = vec![];
        for i in 0..16 {
            let id = doc.add_vector_layer().unwrap();
            doc.set_animation_key(&id, Property::X, key(0, 0., Interpolation::Linear))
                .unwrap();
            if i == 0 {
                doc.set_animation_key(&id, Property::X, key(119, 100., Interpolation::Linear))
                    .unwrap();
            }
            ids.push(id);
        }
        let object: crate::vector::VectorObject = serde_json::from_value(serde_json::json!({
            "id":"shape","name":"Shape","path":{"data":"M0 0H12V12H0Z","fillRule":"nonZero"},
            "transform":[1,0,0,1,0,0],"fill":{"color":[30,60,180,255]},"stroke":null,"strokeWidth":0,"visible":true
        })).unwrap();
        let mut state = doc.document_state();
        for layer in &mut state.svg_layers {
            if ids.contains(&layer.id) {
                layer.vector_objects = (0..160)
                    .map(|i| {
                        let mut o = object.clone();
                        o.id = format!("shape-{i}");
                        o.transform[4] = (i % 16) as f32 * 16.;
                        o.transform[5] = (i / 16) as f32 * 16.;
                        o
                    })
                    .collect();
                layer.source = vector_svg(state.width, state.height, &layer.vector_objects);
            }
        }
        let source = Document::from_document_state(state).unwrap();
        let mut trials = vec![];
        for trial in 0..6 {
            let mut final_layers = vec![];
            for incremental in if trial % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let mut preview = source.clone_for_rendering();
                let mut previous = None;
                for frame in 0..8 {
                    preview.evaluate_animation_incremental_with_outlines(
                        &source,
                        frame,
                        if incremental { previous } else { None },
                        None,
                    );
                    previous = Some(frame);
                }
                let mut samples_ms = vec![];
                let mut evaluated = 0;
                let mut reused = 0;
                for frame in 1..=36 {
                    let start = Instant::now();
                    let stats = preview.evaluate_animation_incremental_with_outlines(
                        &source,
                        frame,
                        if incremental { previous } else { None },
                        None,
                    );
                    std::hint::black_box(&preview.svg_layers);
                    samples_ms.push(start.elapsed().as_secs_f64() * 1000.);
                    evaluated += stats.evaluated_tracks;
                    reused += stats.reused_tracks;
                    previous = Some(frame);
                }
                assert_eq!(evaluated, if incremental { 36 } else { 576 });
                assert_eq!(reused, if incremental { 540 } else { 0 });
                final_layers.push(
                    preview
                        .svg_layers
                        .iter()
                        .map(|l| (l.id.clone(), l.source.clone(), l.opacity))
                        .collect::<Vec<_>>(),
                );
                trials.push(serde_json::json!({"trial":trial,"incremental":incremental,
                    "samplesMs":samples_ms,"evaluatedTracks":evaluated,"reusedTracks":reused}));
            }
            assert_eq!(final_layers[0], final_layers[1]);
        }
        let result = serde_json::json!({"tracks":16,"movingTracks":1,"objects":2560,
            "framesPerTrial":36,"warmupFrames":8,"trials":trials,
            "scope":"CPU evaluator only; no GPU, IPC, input latency, or FPS claim"});
        if let Ok(path) = std::env::var("LUMAPAINT_TIMELINE_BENCH_OUTPUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        }
        println!("timeline evaluator: 16 -> 1 evaluated tracks per frame, final layers equal");
    }
    #[test]
    fn unchanged_tracks_preserve_holds_and_conservatively_evaluate_active_motions() {
        let mut track = Track::default();
        track.channels.insert(
            Property::X,
            vec![
                key(0, 12., Interpolation::Hold),
                key(20, 24., Interpolation::Linear),
            ],
        );
        assert!(track.same_picture(0, 19));
        assert!(!track.same_picture(19, 20));
        track.motions.push(crate::motion::Clip {
            id: "later".into(),
            start: 40,
            duration: 10,
            ..Default::default()
        });
        assert!(track.same_picture(21, 39));
        assert!(!track.same_picture(39, 40));
        assert!(!track.same_picture(60, 61));
        track.motions[0].enabled = false;
        assert!(track.same_picture(60, 61));
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
    fn library_cel_drawing_stays_independent_from_exposure_timing() {
        let mut d = Document::default();
        let cel = d.create_animation_cel("layer-1").unwrap();
        d.load_animation_library_cel("layer-1", cel).unwrap();
        d.begin(Point { x: 20., y: 20. }, Brush::default()).unwrap();
        d.finish();
        d.record_animation_library_cel("layer-1", cel).unwrap();
        assert!(d.animation().tracks["layer-1"].exposures.is_empty());
        assert_eq!(
            d.animation().tracks["layer-1"].cel_library[&cel]
                .drawing
                .strokes
                .len(),
            1
        );
        d.undo();
        assert!(d.animation().tracks["layer-1"].cel_library[&cel]
            .drawing
            .strokes
            .is_empty());
        d.redo();
        d.assign_animation_cel("layer-1", 0, cel).unwrap();
        d.assign_animation_cel("layer-1", 5, cel).unwrap();
        let before: Vec<_> = d.animation().tracks["layer-1"]
            .exposures
            .iter()
            .map(|(f, e)| (*f, e.id.clone(), e.end))
            .collect();
        d.begin(Point { x: 40., y: 40. }, Brush::default()).unwrap();
        d.finish();
        d.record_animation_library_cel("layer-1", cel).unwrap();
        let track = &d.animation().tracks["layer-1"];
        assert_eq!(track.cels[&0].strokes.len(), 2);
        assert!(Arc::ptr_eq(&track.cels[&0], &track.cels[&5]));
        assert_eq!(
            before,
            track
                .exposures
                .iter()
                .map(|(f, e)| (*f, e.id.clone(), e.end))
                .collect::<Vec<_>>()
        );
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(
            restored.animation().tracks["layer-1"].cel_library[&cel]
                .drawing
                .strokes
                .len(),
            2
        );
        let revision = d.revision();
        assert!(d.load_animation_library_cel("layer-1", cel + 100).is_err());
        assert!(d
            .record_animation_library_cel("layer-1", cel + 100)
            .is_err());
        assert_eq!(d.revision(), revision);
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
