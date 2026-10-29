//! Canvas cropping keeps artwork outside the new canvas editable.
use super::*;

/// Immutable input for a host-side resampling worker.
pub struct ImageResamplePlan {
    pub old: (u32, u32),
    pub new: (u32, u32),
    pub resolution: u32,
    /// Layer id, unclipped SVG, and whether it is a canonical full-canvas PNG.
    pub sources: Vec<(String, String, bool)>,
    pub pixel_masks: Vec<(String, crate::layer_mask::LayerMask)>,
    strokes: Vec<Stroke>,
    paper: crate::natural_media::PaperSettings,
}
impl Document {
    /// Prepare a restricted, lossless-to-editing document conversion. Pixel
    /// sources are accepted only when they contain JPEG images and transforms;
    /// the worker must additionally verify their uncropped bounds.
    pub fn prepare_image_resample(&self, resolution: u32) -> Result<ImageResamplePlan, String> {
        if !(1..=1200).contains(&resolution) {
            return Err("Invalid resolution".into());
        }
        if self.active.is_some()
            || self.pages.is_some()
            || self.artboards
            || self
                .strokes
                .iter()
                .any(|stroke| !rotation_safe_stroke(stroke))
            || self.paint_source.is_some()
            || !self.animation.tracks.is_empty()
            || self
                .layer_effects
                .values()
                .any(|effects| effects.screentone.is_some())
            || !self.saved_pixel_selections.is_empty()
            || self.previous_pixel_selection.is_some()
            || self.selection.is_some()
            || self.quick_mask
            || self.path_editing.is_some()
            || !self.compound_shapes.is_empty()
            || !self.saved_paths.is_empty()
            || !self.vector_assets.is_empty()
            || self.pixel_aspect_ratio != 1.
            || self.bit_depth != 8
            || self.color_mode != ColorMode::Rgb
            || self.color_profile != ColorProfile::Srgb
        {
            return Err("Resampling is unavailable for this document / この文書のリサンプリングは未対応です / 暂不支持对此文档重新采样".into());
        }
        let width =
            ((self.width as f64 * resolution as f64 / self.resolution as f64).round()) as u32;
        let height =
            ((self.height as f64 * resolution as f64 / self.resolution as f64).round()) as u32;
        if width == 0
            || height == 0
            || width > MAX_DOCUMENT_DIMENSION
            || height > MAX_DOCUMENT_DIMENSION
            || u64::from(width) * u64::from(height) > 16_777_216
            || u64::from(self.width) * u64::from(self.height) > 16_777_216
        {
            return Err("Resampled image dimensions exceed the limit / リサンプリング後の画像サイズが上限を超えます / 重采样图像尺寸超出限制".into());
        }
        let sx = width as f32 / self.width as f32;
        let sy = height as f32 / self.height as f32;
        if !self.strokes.is_empty() && (sx - sy).abs() > 0.000001 {
            return Err("Editable strokes require equal scaling on both axes / 編集可能な筆跡には縦横同率の拡大縮小が必要です / 可编辑笔画需要等比例缩放".into());
        }
        for stroke in &self.strokes {
            let mut brush = stroke.brush;
            brush.size *= sx;
            brush.envelope.attack *= sx;
            brush.envelope.decay *= sx;
            brush.envelope.hold *= sx;
            brush.envelope.release *= sx;
            brush.validate()?;
            if stroke.points.iter().any(|p| {
                !(Point {
                    x: p.x * sx,
                    y: p.y * sy,
                })
                .valid()
            }) {
                return Err("Scaled stroke coordinates exceed the limit / 筆跡の座標が上限を超えます / 缩放后的笔画坐标超出限制".into());
            }
        }
        let pixel_masks = self
            .layer_effects
            .iter()
            .filter_map(|(id, effects)| {
                effects
                    .mask
                    .as_ref()
                    .filter(|mask| mask.kind == crate::layer_mask::MaskKind::Pixel)
                    .map(|mask| (id.clone(), mask.clone()))
            })
            .collect();
        let mut sources = Vec::new();
        for layer in &self.svg_layers {
            if layer.vector_layer {
                if self.noncanonical_vector_sources.contains(&layer.id) {
                    return Err("Unsupported vector source".into());
                }
                continue;
            }
            let prefix = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><image width=\"{}\" height=\"{}\" href=\"data:image/png;base64,", self.width, self.height, self.width, self.height);
            let canonical_png = layer
                .source
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix("\"/></svg>"))
                .is_some_and(|data| {
                    !data.is_empty()
                        && data
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
                });
            let xml = roxmltree::Document::parse(&layer.source).map_err(|e| e.to_string())?;
            for node in xml.descendants().filter(|n| n.is_element()) {
                if !matches!(node.tag_name().name(), "svg" | "g" | "image")
                    || (node.tag_name().name() == "svg"
                        && node.parent().is_some_and(|p| p.is_element()))
                    || node.attributes().any(|a| {
                        !matches!(
                            a.name(),
                            "width"
                                | "height"
                                | "x"
                                | "y"
                                | "transform"
                                | "href"
                                | "preserveAspectRatio"
                                | "opacity"
                                | "id"
                        )
                    })
                    || (node.tag_name().name() == "image"
                        && !node.attribute("href").is_some_and(|v| {
                            v.starts_with("data:image/jpeg;base64,")
                                || (canonical_png && v.starts_with("data:image/png;base64,"))
                        }))
                {
                    return Err("Unsupported pixel source for resampling / このピクセルレイヤーはリサンプリング未対応です / 暂不支持对此像素图层重新采样".into());
                }
            }
            let content = unclipped_pixel_source(&layer.source, self.width, self.height)?;
            let unbounded = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{content}</svg>", self.width, self.height);
            sources.push((layer.id.clone(), unbounded, canonical_png));
        }
        Ok(ImageResamplePlan {
            old: (self.width, self.height),
            new: (width, height),
            resolution,
            sources,
            pixel_masks,
            strokes: self.strokes.clone(),
            paper: self.paper,
        })
    }

    /// Atomically commit worker results and record a single Undo entry.
    pub fn commit_image_resample(
        &mut self,
        plan: &ImageResamplePlan,
        replacements: Vec<(String, String)>,
        pixel_masks: Vec<(String, crate::layer_mask::LayerMask)>,
    ) -> Result<bool, String> {
        let checked = self.prepare_image_resample(plan.resolution)?;
        if checked.old != plan.old
            || checked.new != plan.new
            || checked.sources != plan.sources
            || checked.pixel_masks != plan.pixel_masks
            || checked.strokes != plan.strokes
            || checked.paper != plan.paper
            || replacements.len() != checked.sources.len()
            || replacements
                .iter()
                .zip(&checked.sources)
                .any(|((id, _), (expected, _, _))| id != expected)
            || pixel_masks.len() != checked.pixel_masks.len()
            || pixel_masks
                .iter()
                .zip(&checked.pixel_masks)
                .any(|((id, _), (expected, _))| id != expected)
        {
            return Err("Document changed during resampling / リサンプリング中に文書が変更されました / 重采样期间文档已更改".into());
        }
        let mut next = self.clone();
        let mut previous = next.vector_history_state();
        previous.crop_dimensions = Some(plan.old);
        previous.resample_resolution = Some(next.resolution);
        previous.strokes = Some(next.strokes.clone());
        let sx = plan.new.0 as f32 / plan.old.0 as f32;
        let sy = plan.new.1 as f32 / plan.old.1 as f32;
        let matrix = [sx, 0., 0., sy, 0., 0.];
        if next.paper.enabled
            && next.paper.kind != crate::natural_media::PaperKind::Smooth
            && next.paper.depth != 0.
        {
            previous.paper = Some(next.paper);
            next.paper.transform_texture(matrix)?;
        }
        next.width = plan.new.0;
        next.height = plan.new.1;
        next.resolution = plan.resolution;
        for stroke in &mut next.strokes {
            transform_stroke_texture(stroke, matrix)?;
            for point in &mut stroke.points {
                point.x *= sx;
                point.y *= sy;
            }
            stroke.brush.size *= sx;
            stroke.brush.envelope.attack *= sx;
            stroke.brush.envelope.decay *= sx;
            stroke.brush.envelope.hold *= sx;
            stroke.brush.envelope.release *= sx;
        }
        let mut replacements = replacements.into_iter();
        for layer in &mut next.svg_layers {
            if layer.vector_layer {
                for object in &mut layer.vector_objects {
                    object.transform = crate::image_frame::multiply(matrix, object.transform);
                }
                layer.source = vector_svg(next.width, next.height, &layer.vector_objects);
            } else {
                layer.source = replacements.next().ok_or("Missing resampled layer")?.1;
            }
            validate_svg_layer(layer)?;
        }
        for guide in &mut next.guides.items {
            if let Some(axis) = &guide.axis {
                guide.position *= if axis == "vertical" { sx } else { sy };
            }
            for object in &mut guide.objects {
                object.transform = crate::image_frame::multiply(matrix, object.transform);
            }
        }
        next.guides.origin[0] *= sx;
        next.guides.origin[1] *= sy;
        next.guides.validate()?;
        let mut pixel_masks = pixel_masks.into_iter();
        for effects in next.layer_effects.values_mut() {
            if let Some(mask) = &mut effects.mask {
                if mask.kind == crate::layer_mask::MaskKind::Pixel {
                    *mask = pixel_masks.next().ok_or("Missing resampled pixel mask")?.1;
                    mask.validate()?;
                } else {
                    mask.transform_by(matrix)?;
                }
            }
        }
        next.record_vector_edit(previous);
        next.revision += 1;
        *self = next;
        Ok(true)
    }
    pub fn crop_canvas(&mut self, bounds: [f32; 4]) -> Result<bool, String> {
        if bounds.iter().any(|v| !v.is_finite()) || bounds[2] <= 0. || bounds[3] <= 0. {
            return Err("Invalid crop bounds".into());
        }
        let x = bounds[0].floor();
        let y = bounds[1].floor();
        let width = (bounds[0] + bounds[2]).ceil() - x;
        let height = (bounds[1] + bounds[3]).ceil() - y;
        if x < 0.
            || y < 0.
            || x + width > self.width as f32
            || y + height > self.height as f32
            || width < 1.
            || height < 1.
        {
            return Err("Crop bounds must fit the canvas".into());
        }
        if x == 0. && y == 0. && width == self.width as f32 && height == self.height as f32 {
            return Ok(false);
        }
        self.resize_canvas_at(x as i32, y as i32, width as u32, height as u32)
    }

    /// Rotate static SVG and editable vector artwork without rasterizing it.
    pub fn rotate_image_quarter_turns(&mut self, turns: u8) -> Result<bool, String> {
        if !(1..=3).contains(&turns) {
            return Err("Invalid rotation".into());
        }
        if self.active.is_some()
            || self
                .strokes
                .iter()
                .any(|stroke| !rotation_safe_stroke(stroke))
            || self.animation.tracks.iter().any(|(layer_id, track)| {
                track.motions.iter().any(|clip| {
                    clip.extruded
                        || !clip.path.is_empty()
                        || crate::motion::preset(&clip.preset).is_none_or(|preset| {
                            preset.category.starts_with("tx-")
                                || !preset.actors.is_empty()
                                || (preset.operations.contains(&crate::motion::Operation::Morph)
                                    && self
                                        .svg_layers
                                        .iter()
                                        .find(|layer| &layer.id == layer_id)
                                        .is_none_or(|layer| {
                                            !layer.vector_layer
                                                || self
                                                    .noncanonical_vector_sources
                                                    .contains(&layer.id)
                                                || layer.vector_objects.iter().any(|object| {
                                                    object.text.is_some()
                                                        || object.image_frame.is_some()
                                                })
                                        }))
                                || preset.operations.iter().any(|op| {
                                    !matches!(
                                        op,
                                        crate::motion::Operation::Fade
                                            | crate::motion::Operation::SlideX
                                            | crate::motion::Operation::SlideY
                                            | crate::motion::Operation::Travel
                                            | crate::motion::Operation::Orbit
                                            | crate::motion::Operation::Zigzag
                                            | crate::motion::Operation::Hop
                                            | crate::motion::Operation::Anticipate
                                            | crate::motion::Operation::Recoil
                                            | crate::motion::Operation::Shake
                                            | crate::motion::Operation::Scale
                                            | crate::motion::Operation::ScaleX
                                            | crate::motion::Operation::ScaleY
                                            | crate::motion::Operation::Pulse
                                            | crate::motion::Operation::Squash
                                            | crate::motion::Operation::Jelly
                                            | crate::motion::Operation::Spin
                                            | crate::motion::Operation::Swing
                                            | crate::motion::Operation::FlipX
                                            | crate::motion::Operation::FlipY
                                            | crate::motion::Operation::Iris
                                            | crate::motion::Operation::Wipe
                                            | crate::motion::Operation::Diagonal
                                            | crate::motion::Operation::Split
                                            | crate::motion::Operation::Blinds
                                            | crate::motion::Operation::Blocks
                                            | crate::motion::Operation::Dissolve
                                            | crate::motion::Operation::ClipWindow
                                            | crate::motion::Operation::Morph
                                            | crate::motion::Operation::Tilt
                                            | crate::motion::Operation::Skew
                                            | crate::motion::Operation::Parallax
                                            | crate::motion::Operation::Reorder
                                            | crate::motion::Operation::Gather
                                            | crate::motion::Operation::Scatter
                                            | crate::motion::Operation::Follow
                                            | crate::motion::Operation::Wave
                                            | crate::motion::Operation::Hold
                                            | crate::motion::Operation::Cut
                                            | crate::motion::Operation::Camera
                                            | crate::motion::Operation::Crane
                                    )
                                })
                        })
                }) || track.cel_library.values().any(|cel| {
                    cel.drawing
                        .strokes
                        .iter()
                        .any(|stroke| !rotation_safe_stroke(stroke))
                })
            })
            || !self.saved_pixel_selections.is_empty()
            || self.previous_pixel_selection.is_some()
            || self.selection.is_some()
            || self.quick_mask
            || self.path_editing.is_some()
            || self
                .layer_effects
                .values()
                .any(|effects| effects.enabled && effects.screentone.is_some())
            || !self.compound_shapes.is_empty()
            || !self.saved_paths.is_empty()
            || self.pixel_aspect_ratio != 1.0
        {
            return Err("Rotation is not yet available for some strokes, selections, screentones, paths, or motion clips / 一部の筆跡・選択範囲・スクリーントーン・パス・モーションクリップの回転は未対応です / 暂不支持旋转部分笔画、选区、网点、路径或动态片段".into());
        }
        let old = (self.width, self.height);
        let (width, height, matrix) = match turns {
            1 => (old.1, old.0, [0., 1., -1., 0., old.1 as f32, 0.]),
            2 => (old.0, old.1, [-1., 0., 0., -1., old.0 as f32, old.1 as f32]),
            _ => (old.1, old.0, [0., -1., 1., 0., 0., old.0 as f32]),
        };
        let mut next = self.clone();
        let mut previous = next.vector_history_state();
        previous.crop_dimensions = Some(old);
        if next.paper.enabled
            && next.paper.kind != crate::natural_media::PaperKind::Smooth
            && next.paper.depth != 0.
        {
            previous.paper = Some(next.paper);
            next.paper.transform_texture(matrix)?;
        }
        previous.strokes = Some(next.strokes.clone());
        next.width = width;
        next.height = height;
        for layer in &mut next.svg_layers {
            for object in &mut layer.vector_objects {
                object.transform = crate::image_frame::multiply(matrix, object.transform);
            }
            if layer.vector_layer && !self.noncanonical_vector_sources.contains(&layer.id) {
                layer.source = vector_svg(width, height, &layer.vector_objects);
            } else {
                layer.source = transform_image_source(&layer.source, old, (width, height), matrix)?;
            }
            validate_svg_layer(layer)?;
        }
        if let Some(source) = &mut next.paint_source {
            *source = transform_image_source(source, old, (width, height), matrix)?;
        }
        for effects in next.layer_effects.values_mut() {
            if let Some(mask) = &mut effects.mask {
                mask.transform_by(matrix)?;
            }
        }
        for guide in &mut next.guides.items {
            let position = guide.position;
            match (turns, guide.axis.as_deref()) {
                (1, Some("vertical")) => {
                    guide.axis = Some("horizontal".into());
                    guide.position = position;
                }
                (1, Some("horizontal")) => {
                    guide.axis = Some("vertical".into());
                    guide.position = old.1 as f32 - position;
                }
                (2, Some("vertical")) => guide.position = old.0 as f32 - position,
                (2, Some("horizontal")) => guide.position = old.1 as f32 - position,
                (3, Some("vertical")) => {
                    guide.axis = Some("horizontal".into());
                    guide.position = old.0 as f32 - position;
                }
                (3, Some("horizontal")) => {
                    guide.axis = Some("vertical".into());
                    guide.position = position;
                }
                _ => {}
            }
            for object in &mut guide.objects {
                object.transform = crate::image_frame::multiply(matrix, object.transform);
            }
        }
        let [ox, oy] = next.guides.origin;
        next.guides.origin = [
            matrix[0] * ox + matrix[2] * oy + matrix[4],
            matrix[1] * ox + matrix[3] * oy + matrix[5],
        ];
        next.guides.validate()?;
        for stroke in &mut next.strokes {
            transform_stroke_texture(stroke, matrix)?;
            for point in &mut stroke.points {
                let (x, y) = (point.x, point.y);
                point.x = matrix[0] * x + matrix[2] * y + matrix[4];
                point.y = matrix[1] * x + matrix[3] * y + matrix[5];
            }
        }
        let animation = std::sync::Arc::make_mut(&mut next.animation);
        for track in animation.tracks.values_mut() {
            track.refresh_exposures(animation.duration)?;
            for clip in &mut track.motions {
                clip.coordinate_turns = (clip.coordinate_turns + turns) % 4;
                let [x, y] = clip.pivot;
                clip.pivot = match turns {
                    1 => [1. - y, x],
                    2 => [1. - x, 1. - y],
                    _ => [y, 1. - x],
                };
            }
            if track.cel_library.values().any(|cel| {
                cel.drawing
                    .strokes
                    .iter()
                    .any(|stroke| !rotation_safe_stroke(stroke))
            }) {
                return Err("Rotation is not yet available for these animation strokes / アニメーションの一部の筆跡は回転できません / 暂不支持旋转动画中的部分笔画".into());
            }
            let [x, y] = track.anchor;
            track.anchor = [
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            ];
            let mut x_keys = track.channels.remove(&animation::Property::X);
            let mut y_keys = track.channels.remove(&animation::Property::Y);
            let negate = |keys: &mut Option<Vec<animation::Keyframe>>| {
                if let Some(keys) = keys {
                    for key in keys {
                        key.value = -key.value;
                    }
                }
            };
            match turns {
                1 => {
                    negate(&mut y_keys);
                    std::mem::swap(&mut x_keys, &mut y_keys);
                }
                2 => {
                    negate(&mut x_keys);
                    negate(&mut y_keys);
                }
                _ => {
                    negate(&mut x_keys);
                    std::mem::swap(&mut x_keys, &mut y_keys);
                }
            }
            if let Some(keys) = x_keys {
                track.channels.insert(animation::Property::X, keys);
            }
            if let Some(keys) = y_keys {
                track.channels.insert(animation::Property::Y, keys);
            }
            if turns != 2 {
                let sx = track.channels.remove(&animation::Property::ScaleX);
                let sy = track.channels.remove(&animation::Property::ScaleY);
                if let Some(keys) = sx {
                    track.channels.insert(animation::Property::ScaleY, keys);
                }
                if let Some(keys) = sy {
                    track.channels.insert(animation::Property::ScaleX, keys);
                }
            }
            for cel in track.cel_library.values_mut() {
                let drawing = std::sync::Arc::make_mut(&mut cel.drawing);
                if let Some(source) = &mut drawing.source {
                    *source = transform_image_source(source, old, (width, height), matrix)?;
                }
                for stroke in &mut drawing.strokes {
                    transform_stroke_texture(stroke, matrix)?;
                    for point in &mut stroke.points {
                        let (x, y) = (point.x, point.y);
                        point.x = matrix[0] * x + matrix[2] * y + matrix[4];
                        point.y = matrix[1] * x + matrix[3] * y + matrix[5];
                    }
                }
            }
            track.refresh_exposures(animation.duration)?;
        }
        animation.validate(width, height)?;
        next.deselect();
        next.record_vector_edit(previous);
        next.revision += 1;
        *self = next;
        Ok(true)
    }

    /// Canvas-size menu entry. Reject document data whose coordinates are not yet migrated.
    pub fn resize_canvas_checked(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<bool, String> {
        if !self.saved_pixel_selections.is_empty()
            || self.previous_pixel_selection.is_some()
            || self.selection.is_some()
            || self.quick_mask
            || self.path_editing.is_some()
        {
            return Err("Canvas resize is unavailable for pixel selections / ピクセル選択を含む文書のカンバスサイズ変更は未対応です / 暂不支持含像素选区的画布大小更改".into());
        }
        self.resize_canvas_at(x, y, width, height)
    }

    /// Resize the active canvas while keeping artwork outside its bounds editable.
    /// The origin is expressed in the old canvas coordinate system.
    pub fn resize_canvas_at(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<bool, String> {
        if width == 0
            || height == 0
            || width > MAX_DOCUMENT_DIMENSION
            || height > MAX_DOCUMENT_DIMENSION
            || x.unsigned_abs() > MAX_DOCUMENT_DIMENSION
            || y.unsigned_abs() > MAX_DOCUMENT_DIMENSION
        {
            return Err("Invalid canvas dimensions or origin".into());
        }
        if x == 0 && y == 0 && width == self.width && height == self.height {
            return Ok(false);
        }
        let x = x as f32;
        let y = y as f32;
        let mut next = self.clone();
        next.finish();
        let mut previous = next.vector_history_state();
        previous.crop_dimensions = Some((next.width, next.height));
        if next.paper.enabled
            && next.paper.kind != crate::natural_media::PaperKind::Smooth
            && next.paper.depth != 0.
        {
            previous.paper = Some(next.paper);
            next.paper.transform_texture([1., 0., 0., 1., -x, -y])?;
        }
        previous.strokes = Some(next.strokes.clone());
        let old = (next.width, next.height);
        next.width = width as u32;
        next.height = height as u32;
        let shift = |object: &mut VectorObject| {
            object.transform[4] -= x;
            object.transform[5] -= y;
        };
        for layer in &mut next.svg_layers {
            for object in &mut layer.vector_objects {
                shift(object);
            }
            if layer.vector_layer && !self.noncanonical_vector_sources.contains(&layer.id) {
                layer.source = vector_svg(next.width, next.height, &layer.vector_objects);
            } else {
                layer.source = crop_source(&layer.source, old, (next.width, next.height), x, y)?;
            }
            validate_svg_layer(layer)?;
        }
        if let Some(source) = &mut next.paint_source {
            *source = crop_source(source, old, (next.width, next.height), x, y)?;
        }
        // Masks use document-space transforms even when unlinked from their layer.
        for effects in next.layer_effects.values_mut() {
            if let Some(mask) = &mut effects.mask {
                mask.transform_by([1., 0., 0., 1., -x, -y])?;
            }
        }
        for stroke in &mut next.strokes {
            transform_stroke_texture(stroke, [1., 0., 0., 1., -x, -y])?;
            for point in &mut stroke.points {
                point.x -= x;
                point.y -= y;
            }
            stroke.selection = stroke.selection.as_ref().map(|s| s.translated(-x, -y));
        }
        // The animation library is canonical; rebuild playback references after editing it.
        let animation = std::sync::Arc::make_mut(&mut next.animation);
        for track in animation.tracks.values_mut() {
            track.refresh_exposures(animation.duration)?;
            track.anchor[0] -= x;
            track.anchor[1] -= y;
            for cel in track.cel_library.values_mut() {
                let drawing = std::sync::Arc::make_mut(&mut cel.drawing);
                if let Some(source) = &mut drawing.source {
                    *source = crop_source(source, old, (width, height), x, y)?;
                }
                for stroke in &mut drawing.strokes {
                    transform_stroke_texture(stroke, [1., 0., 0., 1., -x, -y])?;
                    for point in &mut stroke.points {
                        point.x -= x;
                        point.y -= y;
                    }
                    if let Some(selection) = &mut stroke.selection {
                        *selection = selection.transformed([1., 0., 0., 1., -x, -y])?;
                    }
                }
            }
            track.refresh_exposures(animation.duration)?;
        }
        animation.validate(width, height)?;
        for shape in &mut next.compound_shapes {
            for object in &mut shape.operands {
                shift(object);
            }
        }
        for path in &mut next.saved_paths {
            for object in &mut path.objects {
                shift(object);
            }
        }
        for guide in &mut next.guides.items {
            if let Some(axis) = &guide.axis {
                guide.position -= if axis == "vertical" { x } else { y };
            }
            for object in &mut guide.objects {
                shift(object);
            }
        }
        next.guides.origin[0] -= x;
        next.guides.origin[1] -= y;
        next.deselect();
        next.record_vector_edit(previous);
        next.revision += 1;
        *self = next;
        Ok(true)
    }
}
fn rotation_safe_stroke(stroke: &Stroke) -> bool {
    !stroke.clear && stroke.selection.is_none()
}
fn transform_stroke_texture(stroke: &mut Stroke, matrix: [f32; 6]) -> Result<(), String> {
    if !stroke.eraser {
        stroke.brush.material.transform_grain(matrix)?;
        if stroke.brush.material.enabled && stroke.brush.material.paper.enabled {
            stroke.brush.material.paper.transform_texture(matrix)?;
        }
    }
    Ok(())
}
fn transform_image_source(
    source: &str,
    old: (u32, u32),
    size: (u32, u32),
    matrix: [f32; 6],
) -> Result<String, String> {
    let content = unclipped_pixel_source(source, old.0, old.1)?;
    let xml = roxmltree::Document::parse(&content).map_err(|e| e.to_string())?;
    let content = &content[xml.root_element().range()];
    let [a, b, c, d, e, f] = matrix;
    let result = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><g transform="matrix({a} {b} {c} {d} {e} {f})">{content}</g></svg>"#,
        size.0, size.1
    );
    validate_svg_edit_source(&result)?;
    Ok(result)
}
fn crop_source(
    source: &str,
    old: (u32, u32),
    size: (u32, u32),
    x: f32,
    y: f32,
) -> Result<String, String> {
    let content = unclipped_pixel_source(source, old.0, old.1)?;
    let xml = roxmltree::Document::parse(&content).map_err(|e| e.to_string())?;
    let content = &content[xml.root_element().range()];
    let result = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><g transform="translate({} {})">{content}</g></svg>"#,
        size.0, size.1, -x, -y
    );
    validate_svg_edit_source(&result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropping_active_page_keeps_other_pages_and_history() {
        let mut doc = Document::default();
        doc.setup_pages(PageSetup {
            count: 3,
            facing: true,
            binding: PageBinding::LeftToRight,
        })
        .unwrap();
        let original = doc.pages_snapshot();
        doc.crop_canvas([10., 20., 200., 100.]).unwrap();
        let cropped = doc.pages_snapshot();
        assert_eq!(
            (cropped.pages[0].width, cropped.pages[0].height),
            (200, 100)
        );
        for i in 1..3 {
            assert_eq!(
                (cropped.pages[i].width, cropped.pages[i].height),
                (original.pages[i].width, original.pages[i].height)
            );
        }
        doc.undo();
        assert_eq!(doc.dimensions(), (960, 640));
        doc.redo();
        assert_eq!(doc.dimensions(), (200, 100));
    }

    #[test]
    fn crop_preserves_off_canvas_vector_and_imported_svg_content() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"crop-shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V80H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.import_svg("image".into(),r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect width="200" height="100" fill="red"/></svg>"#.into()).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.crop_canvas([30., 40., 200., 150.]).unwrap();
        assert_eq!(
            doc.svg_layers[0].vector_objects[0].transform[4..],
            [-20., -20.]
        );
        assert!(doc.svg_layers[1].source.contains("translate(-30 -40)"));
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        assert_eq!(loaded.svg_layers[0].vector_objects.len(), 1);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }

    #[test]
    fn crop_translates_pixels_and_vectors_atomically_and_undo_restores_dimensions() {
        let mut doc = Document::default();
        doc.begin(Point { x: 60., y: 80. }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 90., y: 100. }).unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.crop_canvas([20., 30., 200., 150.]).unwrap());
        assert_eq!(doc.dimensions(), (200, 150));
        assert_eq!(doc.strokes[0].points[0], Point { x: 40., y: 50. });
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(doc.crop_canvas([0., 0., 201., 150.]).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(!doc.crop_canvas([0., 0., 200., 150.]).unwrap());
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        assert_eq!(loaded.dimensions(), (200, 150));
    }
    #[test]
    fn canvas_resize_preserves_artwork_and_round_trips_history() {
        let mut doc = Document::default();
        doc.begin(Point { x: 60., y: 80. }, Brush::default())
            .unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.resize_canvas_at(-20, -30, 1000, 700).unwrap());
        assert_eq!(doc.dimensions(), (1000, 700));
        assert_eq!(doc.strokes[0].points[0], Point { x: 80., y: 110. });
        let after = serde_json::to_value(doc.document_state()).unwrap();
        assert_eq!(
            Document::from_document_state(doc.document_state())
                .unwrap()
                .dimensions(),
            (1000, 700)
        );
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(doc
            .resize_canvas_at(0, 0, MAX_DOCUMENT_DIMENSION + 1, 1)
            .is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(!doc.resize_canvas_at(0, 0, 1000, 700).unwrap());
    }
    #[test]
    fn resize_translates_linked_and_unlinked_masks_with_undo() {
        use crate::layer_mask::{LayerMask, MaskKind};
        let mut doc = Document::default();
        let other = doc.add_vector_layer().unwrap();
        for (id, linked) in [("layer-1", true), (other.as_str(), false)] {
            let mut mask = LayerMask::from_selection(MaskKind::Vector, None, 960, 640).unwrap();
            mask.linked = linked;
            let mut effects = doc.layer_effects(id);
            effects.mask = Some(mask);
            doc.set_layer_effects(id, effects).unwrap();
        }
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.resize_canvas_at(-20, -30, 1000, 700).unwrap();
        for id in ["layer-1", other.as_str()] {
            assert_eq!(
                doc.layer_effects(id).mask.unwrap().transform,
                [1., 0., 0., 1., 20., 30.]
            );
        }
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn checked_resize_rejects_pixel_selection_without_mutation() {
        let mut doc = Document::default();
        doc.selection = Some(Selection::new(
            SelectionShape::Rectangle,
            [2., 3., 20., 30.],
        ));
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.resize_canvas_checked(-10, -10, 980, 660).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }
    #[test]
    fn rotate_vector_and_imported_svg_round_trip() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"rotate-shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V80H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.import_svg("image".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect width="100" height="50" fill="red"/></svg>"#.into()).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(doc.dimensions(), (640, 960));
        assert_eq!(
            doc.svg_layers[0].vector_objects[0].transform,
            [0., 1., -1., 0., 620., 10.]
        );
        assert!(doc.svg_layers[1].source.contains("matrix(0 1 -1 0 640 0)"));
        let after = serde_json::to_value(doc.document_state()).unwrap();
        assert_eq!(
            Document::from_document_state(doc.document_state())
                .unwrap()
                .dimensions(),
            (640, 960)
        );
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn textured_strokes_preserve_grain_paper_and_history() {
        use crate::natural_media::{PaperKind, PaperSettings};
        for simulation in [BrushSimulation::Pencil, BrushSimulation::DryBrush] {
            for material_enabled in [false, true] {
                let mut doc = Document::default();
                doc.set_paper(PaperSettings {
                    enabled: true,
                    kind: PaperKind::Washi,
                    ..Default::default()
                })
                .unwrap();
                let brush = Brush {
                    simulation,
                    material: crate::natural_media::MaterialSettings {
                        enabled: material_enabled,
                        ..Default::default()
                    },
                    ..Brush::default()
                };
                doc.begin(Point { x: 10., y: 20. }, brush).unwrap();
                doc.finish();
                let before = serde_json::to_value(doc.document_state()).unwrap();
                let orig = doc.strokes[0].brush.material;
                doc.rotate_image_quarter_turns(1).unwrap();
                let material = doc.strokes[0].brush.material;
                assert_eq!(
                    material.grain_point(619.5, 10.5),
                    orig.grain_point(10.5, 20.5)
                );
                if material_enabled {
                    assert_eq!(
                        material.paper.texture_point(619.5, 10.5),
                        orig.paper.texture_point(10.5, 20.5)
                    );
                }
                let rotated = serde_json::to_value(doc.document_state()).unwrap();
                doc.undo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
                doc.redo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), rotated);
                let plan = doc.prepare_image_resample(doc.resolution * 2).unwrap();
                doc.commit_image_resample(&plan, vec![], vec![]).unwrap();
                let material = doc.strokes[0].brush.material;
                assert_eq!(
                    material.grain_point(1239., 21.),
                    orig.grain_point(10.5, 20.5)
                );
                if material_enabled {
                    assert_eq!(
                        material.paper.texture_point(1239., 21.),
                        orig.paper.texture_point(10.5, 20.5)
                    );
                }
                let scaled = serde_json::to_value(doc.document_state()).unwrap();
                assert_eq!(
                    serde_json::to_value(
                        Document::from_document_state(doc.document_state())
                            .unwrap()
                            .document_state()
                    )
                    .unwrap(),
                    scaled
                );
                doc.undo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), rotated);
                doc.redo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), scaled);
            }
        }
    }
    #[test]
    fn textured_eraser_is_transformed_as_an_editable_round_stroke() {
        let mut doc = Document::default();
        let brush = Brush {
            simulation: BrushSimulation::DryBrush,
            material: crate::natural_media::MaterialSettings {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        doc.begin_eraser(Point { x: 100., y: 80. }, brush, 1.)
            .unwrap();
        doc.extend(Point { x: 150., y: 100. }).unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        assert!(doc.strokes[0].eraser);
        assert_eq!(
            [doc.strokes[0].points[0].x, doc.strokes[0].points[0].y],
            [560., 100.]
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        let plan = doc.prepare_image_resample(doc.resolution * 2).unwrap();
        doc.commit_image_resample(&plan, vec![], vec![]).unwrap();
        assert!(doc.strokes[0].eraser);
        assert_eq!(doc.strokes[0].brush.size, 32.);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn rotation_preserves_uniform_paper_and_history() {
        use crate::natural_media::{PaperKind, PaperSettings};
        for (kind, depth) in [(PaperKind::Smooth, 0.8), (PaperKind::Washi, 0.)] {
            let mut doc = Document::default();
            let paper = PaperSettings {
                enabled: true,
                kind,
                depth,
                ..Default::default()
            };
            doc.set_paper(paper).unwrap();
            let before = serde_json::to_value(doc.document_state()).unwrap();
            let original = paper.color_at(35.5, 70.5);
            assert_eq!(original, paper.color_at(569.5, 35.5));
            doc.rotate_image_quarter_turns(1).unwrap();
            assert_eq!(doc.paper(), paper);
            let after = serde_json::to_value(doc.document_state()).unwrap();
            doc.undo();
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
            doc.redo();
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
            assert_eq!(
                Document::from_document_state(doc.document_state())
                    .unwrap()
                    .paper(),
                paper
            );
        }
        let mut textured = Document::default();
        textured
            .set_paper(PaperSettings {
                enabled: true,
                kind: PaperKind::Washi,
                depth: 0.8,
                ..Default::default()
            })
            .unwrap();
        let before = serde_json::to_value(textured.document_state()).unwrap();
        let original = textured.paper();
        textured.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(
            original.color_at(35.5, 70.5),
            textured.paper().color_at(569.5, 35.5)
        );
        let after = serde_json::to_value(textured.document_state()).unwrap();
        textured.undo();
        assert_eq!(
            serde_json::to_value(textured.document_state()).unwrap(),
            before
        );
        textured.redo();
        assert_eq!(
            serde_json::to_value(textured.document_state()).unwrap(),
            after
        );
        let plan = textured
            .prepare_image_resample(textured.resolution * 2)
            .unwrap();
        textured
            .commit_image_resample(&plan, vec![], vec![])
            .unwrap();
        assert_eq!(
            original.color_at(35.5, 70.5),
            textured.paper().color_at(1139., 71.)
        );
        textured.undo();
        assert_eq!(
            serde_json::to_value(textured.document_state()).unwrap(),
            after
        );
    }
    #[test]
    fn textured_paper_follows_all_rotations_and_round_trips() {
        use crate::natural_media::{PaperKind, PaperSettings};
        for kind in [
            PaperKind::ColdPress,
            PaperKind::Rough,
            PaperKind::Canvas,
            PaperKind::Washi,
        ] {
            for turns in 1..=3 {
                let mut doc = Document::default();
                doc.set_paper(PaperSettings {
                    enabled: true,
                    kind,
                    ..Default::default()
                })
                .unwrap();
                let original = doc.paper();
                let before = serde_json::to_value(doc.document_state()).unwrap();
                doc.rotate_image_quarter_turns(turns).unwrap();
                for (x, y) in [(35.5, 70.5), (101.5, 223.5), (400.5, 501.5)] {
                    let (rx, ry) = match turns {
                        1 => (640. - y, x),
                        2 => (960. - x, 640. - y),
                        _ => (y, 960. - x),
                    };
                    assert_eq!(original.color_at(x, y), doc.paper().color_at(rx, ry));
                }
                let after = serde_json::to_value(doc.document_state()).unwrap();
                assert_eq!(
                    Document::from_document_state(doc.document_state())
                        .unwrap()
                        .paper(),
                    doc.paper()
                );
                doc.undo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
                doc.redo();
                assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
            }
        }
    }
    #[test]
    fn rotation_accepts_ink_and_dry_envelopes() {
        let mut doc = Document::default();
        let brush = Brush {
            simulation: BrushSimulation::Ink,
            ..Default::default()
        };
        doc.begin(Point { x: 10., y: 20. }, brush).unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(
            [doc.strokes[0].points[0].x, doc.strokes[0].points[0].y],
            [620., 10.]
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        let mut dry = Document::default();
        let brush = Brush {
            envelope: BrushEnvelope {
                enabled: true,
                dryness: 0.5,
                ..Default::default()
            },
            ..Default::default()
        };
        dry.begin(Point { x: 10., y: 20. }, brush).unwrap();
        dry.finish();
        let before = serde_json::to_value(dry.document_state()).unwrap();
        dry.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(
            dry.strokes[0].brush.material.grain_point(620., 10.),
            (10., 20.)
        );
        dry.undo();
        assert_eq!(serde_json::to_value(dry.document_state()).unwrap(), before);
    }
    #[test]
    fn resolution_changes_metadata_with_history() {
        let mut doc = Document::default();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.set_image_resolution(300).unwrap());
        assert_eq!(doc.dimensions(), (960, 640));
        let after = serde_json::to_value(doc.document_state()).unwrap();
        assert_eq!(
            Document::from_document_state(doc.document_state())
                .unwrap()
                .snapshot()
                .resolution,
            300
        );
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(doc.set_image_resolution(0).is_err());
        assert!(!doc.set_image_resolution(300).unwrap());
    }
    #[test]
    fn resample_commits_dimensions_resolution_and_layers_in_one_history_step() {
        let mut doc = Document::default();
        let vector = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V50H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&vector, object).unwrap();
        let selection = Selection::new(SelectionShape::Rectangle, [10., 20., 50., 40.]);
        let mask = crate::layer_mask::LayerMask::from_selection(
            crate::layer_mask::MaskKind::Vector,
            Some(&selection),
            960,
            640,
        )
        .unwrap();
        doc.layer_effects.insert(
            vector.clone(),
            crate::layer_effects::LayerEffects {
                mask: Some(mask),
                ..Default::default()
            },
        );
        doc.import_svg("jpeg".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><image width=\"20\" height=\"20\" href=\"data:image/jpeg;base64,AAAA\"/></svg>".into()).unwrap();
        doc.begin(Point { x: 100., y: 80. }, Brush::default())
            .unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let target = doc.resolution * 2;
        let plan = doc.prepare_image_resample(target).unwrap();
        let replacement = plan
            .sources
            .iter()
            .map(|(id, _, _)| {
                (
                    id.clone(),
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1920\" height=\"1280\"/>"
                        .into(),
                )
            })
            .collect();
        doc.commit_image_resample(&plan, replacement, vec![])
            .unwrap();
        assert_eq!(doc.dimensions(), (1920, 1280));
        assert_eq!(doc.resolution, target);
        assert_eq!(
            [doc.strokes[0].points[0].x, doc.strokes[0].points[0].y],
            [200., 160.]
        );
        assert_eq!(doc.strokes[0].brush.size, 32.);
        let mask = doc.layer_effects[&vector].mask.as_ref().unwrap();
        assert_eq!(mask.transform, [2., 0., 0., 2., 0., 0.]);
        assert_eq!(mask.coverage([40., 60.]), 1.);
        assert_eq!(mask.coverage([150., 60.]), 0.);
        assert_eq!(
            doc.svg_layers
                .iter()
                .find(|l| l.id == vector)
                .unwrap()
                .vector_objects[0]
                .transform,
            [2., 0., 0., 2., 20., 40.]
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        Document::from_document_state(doc.document_state()).unwrap();
        assert!(doc.commit_image_resample(&plan, vec![], vec![]).is_err());
    }
    #[test]
    fn resample_pixel_mask_and_stroke_restore_together() {
        use crate::layer_mask::{LayerMask, MaskKind};
        let mut doc = Document::default();
        doc.set_paper(crate::natural_media::PaperSettings {
            enabled: true,
            kind: crate::natural_media::PaperKind::Smooth,
            ..Default::default()
        })
        .unwrap();
        doc.begin(Point { x: 100., y: 80. }, Brush::default())
            .unwrap();
        doc.finish();
        let mask = LayerMask::from_selection(
            MaskKind::Pixel,
            Some(&Selection::new(
                SelectionShape::Rectangle,
                [100., 80., 30., 40.],
            )),
            960,
            640,
        )
        .unwrap();
        doc.layer_effects.insert(
            "layer-1".into(),
            crate::layer_effects::LayerEffects {
                mask: Some(mask.clone()),
                ..Default::default()
            },
        );
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let plan = doc.prepare_image_resample(doc.resolution * 2).unwrap();
        assert_eq!(plan.pixel_masks.len(), 1);
        let mut scaled = mask;
        scaled.transform_by([2., 0., 0., 2., 0., 0.]).unwrap();
        doc.commit_image_resample(&plan, vec![], vec![("layer-1".into(), scaled)])
            .unwrap();
        assert_eq!(doc.strokes[0].brush.size, 32.);
        assert_eq!(
            doc.layer_effects["layer-1"]
                .mask
                .as_ref()
                .unwrap()
                .coverage([220., 180.]),
            1.
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        Document::from_document_state(doc.document_state()).unwrap();
    }
    #[test]
    fn rotate_round_strokes_preserves_history() {
        let mut doc = Document::default();
        doc.begin(Point { x: 10., y: 20. }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 30., y: 40. }).unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(
            doc.strokes[0].points,
            vec![Point { x: 620., y: 10. }, Point { x: 600., y: 30. }]
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn rotate_linked_and_unlinked_masks_preserves_coverage_and_history() {
        use crate::layer_mask::{LayerMask, MaskKind};
        let mut doc = Document::default();
        let other = doc.add_vector_layer().unwrap();
        for (id, linked, kind) in [
            ("layer-1", true, MaskKind::Pixel),
            (other.as_str(), false, MaskKind::Vector),
        ] {
            let mut mask = LayerMask::from_selection(
                kind,
                Some(&Selection::new(
                    SelectionShape::Rectangle,
                    [10., 20., 80., 60.],
                )),
                960,
                640,
            )
            .unwrap();
            mask.linked = linked;
            let mut effects = doc.layer_effects(id);
            effects.mask = Some(mask);
            doc.set_layer_effects(id, effects).unwrap();
        }
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        for id in ["layer-1", other.as_str()] {
            let mask = doc.layer_effects(id).mask.unwrap();
            assert_eq!(mask.transform, [0., 1., -1., 0., 640., 0.]);
            assert_eq!(mask.coverage([610., 20.]), 1.);
            assert_eq!(mask.coverage([20., 20.]), 0.);
        }
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn resize_translates_animation_cels_and_pivots_with_history() {
        let mut doc = Document::default();
        doc.begin(Point { x: 60., y: 80. }, Brush::default())
            .unwrap();
        doc.finish();
        doc.capture_animation_cel("layer-1", 0, false).unwrap();
        let id = doc.add_vector_layer().unwrap();
        doc.set_animation_key(
            &id,
            animation::Property::X,
            animation::Keyframe {
                frame: 0,
                value: 12.,
                interpolation: animation::Interpolation::Linear,
            },
        )
        .unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let anchor = doc.animation.tracks[&id].anchor;
        doc.resize_canvas_checked(-20, -30, 1000, 700).unwrap();
        let track = &doc.animation.tracks["layer-1"];
        assert_eq!(
            track.cel_library.values().next().unwrap().drawing.strokes[0].points[0],
            Point { x: 80., y: 110. }
        );
        assert_eq!(
            doc.animation.tracks[&id].anchor,
            [anchor[0] + 20., anchor[1] + 30.]
        );
        assert_eq!(
            doc.animation.tracks[&id].channels[&animation::Property::X][0].value,
            12.
        );
        doc.animation.validate(1000, 700).unwrap();
        let after = serde_json::to_value(doc.document_state()).unwrap();
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        loaded.animation.validate(1000, 700).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn rotation_maps_guides_and_ruler_origin_and_undoes() {
        let mut doc = Document::default();
        doc.guides.origin = [20., 30.];
        doc.guides.items = vec![
            Guide {
                id: "v".into(),
                axis: Some("vertical".into()),
                position: 100.,
                layer_id: None,
                objects: vec![],
            },
            Guide {
                id: "h".into(),
                axis: Some("horizontal".into()),
                position: 200.,
                layer_id: None,
                objects: vec![],
            },
        ];
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.rotate_image_quarter_turns(1).unwrap();
        assert_eq!(doc.guides.origin, [610., 20.]);
        assert_eq!(doc.guides.items[0].axis.as_deref(), Some("horizontal"));
        assert_eq!(doc.guides.items[0].position, 100.);
        assert_eq!(doc.guides.items[1].axis.as_deref(), Some("vertical"));
        assert_eq!(doc.guides.items[1].position, 440.);
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn rotate_animation_matches_transformed_evaluated_frames() {
        use animation::{Interpolation, Keyframe, Property};
        let mut doc = Document::default();
        let id = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"animated","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V80H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&id, object).unwrap();
        for (property, a, b) in [
            (Property::X, 10., 60.),
            (Property::Y, -20., 50.),
            (Property::ScaleX, 50., 150.),
            (Property::ScaleY, 120., 80.),
            (Property::Rotation, 10., 70.),
        ] {
            for (frame, value) in [(0, a), (10, b)] {
                doc.set_animation_key(
                    &id,
                    property,
                    Keyframe {
                        frame,
                        value,
                        interpolation: Interpolation::Linear,
                    },
                )
                .unwrap();
            }
        }
        doc.begin(Point { x: 100., y: 80. }, Brush::default())
            .unwrap();
        doc.finish();
        doc.capture_animation_cel("layer-1", 0, false).unwrap();
        doc.set_motion_clip(
            &id,
            crate::motion::Clip {
                id: "fade".into(),
                preset: "collapse-away".into(),
                duration: 10,
                pivot: [0.2, 0.7],
                ..Default::default()
            },
        )
        .unwrap();
        let original = serde_json::to_value(doc.document_state()).unwrap();
        for (turns, matrix) in [
            (1, [0., 1., -1., 0., 640., 0.]),
            (2, [-1., 0., 0., -1., 960., 640.]),
            (3, [0., -1., 1., 0., 0., 960.]),
        ] {
            let mut rotated = doc.clone();
            rotated.rotate_image_quarter_turns(turns).unwrap();
            for frame in [0, 5, 10] {
                let mut before = doc.clone_for_rendering();
                before.evaluate_animation(&doc, frame);
                let mut after = rotated.clone_for_rendering();
                after.evaluate_animation(&rotated, frame);
                let expected = crate::image_frame::multiply(
                    matrix,
                    before
                        .svg_layers
                        .iter()
                        .find(|l| l.id == id)
                        .unwrap()
                        .vector_objects[0]
                        .transform,
                );
                let actual = after
                    .svg_layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .vector_objects[0]
                    .transform;
                let before_opacity = before
                    .svg_layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .vector_objects[0]
                    .opacity;
                let after_opacity = after
                    .svg_layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .vector_objects[0]
                    .opacity;
                assert!((before_opacity - after_opacity).abs() < 0.0001);
                for (a, b) in actual.iter().zip(expected) {
                    assert!(
                        (a - b).abs() < 0.001,
                        "turn {turns}, frame {frame}: {actual:?} vs {expected:?}"
                    );
                }
                let point = after.strokes[0].points[0];
                assert_eq!(
                    [point.x, point.y],
                    [
                        matrix[0] * 100. + matrix[2] * 80. + matrix[4],
                        matrix[1] * 100. + matrix[3] * 80. + matrix[5]
                    ]
                );
            }
            let saved = serde_json::to_value(rotated.document_state()).unwrap();
            rotated.undo();
            assert_eq!(
                serde_json::to_value(rotated.document_state()).unwrap(),
                original
            );
            rotated.redo();
            assert_eq!(
                serde_json::to_value(rotated.document_state()).unwrap(),
                saved
            );
            Document::from_document_state(rotated.document_state()).unwrap();
        }
    }

    #[test]
    fn rotation_rejects_unsupported_motion_without_changes() {
        let mut doc = Document::default();
        let id = doc.add_vector_layer().unwrap();
        doc.set_motion_clip(
            &id,
            crate::motion::Clip {
                id: "slide".into(),
                preset: "chromatic-split".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.rotate_image_quarter_turns(1).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }

    #[test]
    fn rotate_translation_motion_presets_match_all_frames() {
        for preset in [
            "fade-slide",
            "snap-in",
            "linear-travel",
            "orbit",
            "zigzag-travel",
            "bounce-land",
            "after-shake",
            "collapse-away",
            "heavy-stop",
            "pop-scale",
            "axis-stretch",
            "impact-squash",
            "simple-turn",
            "tumble-in",
            "fold-in",
            "coin-spin",
            "gyroscope",
            "liquid-morph",
            "velocity-skew",
            "stagger-follow",
            "list-stagger",
            "grid-wave",
            "grid-shuffle",
            "parallax-layers",
            "cam-pushin",
        ] {
            let mut doc = Document::default();
            let id = doc.add_vector_layer().unwrap();
            let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V50H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
            let mut second = object.clone();
            second.id = "shape-two".into();
            second.transform[4] = 140.;
            second.transform[5] = 70.;
            doc.upsert_vector_object(&id, object).unwrap();
            doc.upsert_vector_object(&id, second).unwrap();
            doc.set_motion_clip(
                &id,
                crate::motion::Clip {
                    id: "motion".into(),
                    preset: preset.into(),
                    duration: 10,
                    pivot: [0.2, 0.7],
                    ..Default::default()
                },
            )
            .unwrap();
            for (turns, matrix) in [
                (1, [0., 1., -1., 0., 640., 0.]),
                (2, [-1., 0., 0., -1., 960., 640.]),
                (3, [0., -1., 1., 0., 0., 960.]),
            ] {
                let mut rotated = doc.clone();
                rotated.rotate_image_quarter_turns(turns).unwrap();
                for frame in 0..=10 {
                    let mut before = doc.clone_for_rendering();
                    before.evaluate_animation(&doc, frame);
                    let mut after = rotated.clone_for_rendering();
                    after.evaluate_animation(&rotated, frame);
                    let objects_a = &before
                        .svg_layers
                        .iter()
                        .find(|l| l.id == id)
                        .unwrap()
                        .vector_objects;
                    let objects_b = &after
                        .svg_layers
                        .iter()
                        .find(|l| l.id == id)
                        .unwrap()
                        .vector_objects;
                    assert_eq!(objects_a.len(), objects_b.len());
                    for (a, b) in objects_a.iter().zip(objects_b) {
                        let expected = crate::image_frame::multiply(matrix, a.transform);
                        assert!(
                            b.transform
                                .iter()
                                .zip(expected)
                                .all(|(x, y)| (x - y).abs() < 0.001),
                            "{preset} turn {turns} frame {frame}: actual {:?} expected {:?}",
                            b.transform,
                            expected
                        );
                        assert!((a.opacity - b.opacity).abs() < 0.0001);
                        assert_eq!(a.path.data, b.path.data, "{preset} path at frame {frame}");
                    }
                }
            }
        }
    }

    #[test]
    fn iris_motion_mask_rotates_with_the_artwork() {
        let mut doc = Document::default();
        let id = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V50H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&id, object).unwrap();
        doc.set_motion_clip(
            &id,
            crate::motion::Clip {
                id: "iris".into(),
                preset: "iris-open".into(),
                duration: 10,
                ..Default::default()
            },
        )
        .unwrap();
        let circle = |source: &str| {
            let xml = roxmltree::Document::parse(source).unwrap();
            let node = xml
                .descendants()
                .find(|n| n.has_tag_name("circle"))
                .unwrap();
            ["cx", "cy", "r"].map(|name| node.attribute(name).unwrap().parse::<f32>().unwrap())
        };
        for (turns, m) in [
            (1, [0., 1., -1., 0., 640., 0.]),
            (2, [-1., 0., 0., -1., 960., 640.]),
            (3, [0., -1., 1., 0., 0., 960.]),
        ] {
            let mut rotated = doc.clone();
            rotated.rotate_image_quarter_turns(turns).unwrap();
            for frame in [1, 5, 9] {
                let mut a = doc.clone_for_rendering();
                a.evaluate_animation(&doc, frame);
                let mut b = rotated.clone_for_rendering();
                b.evaluate_animation(&rotated, frame);
                let original = circle(&a.svg_layers.iter().find(|l| l.id == id).unwrap().source);
                let actual = circle(&b.svg_layers.iter().find(|l| l.id == id).unwrap().source);
                assert!(
                    (actual[0] - (m[0] * original[0] + m[2] * original[1] + m[4])).abs() < 0.001
                );
                assert!(
                    (actual[1] - (m[1] * original[0] + m[3] * original[1] + m[5])).abs() < 0.001
                );
                assert!((actual[2] - original[2]).abs() < 0.001);
            }
        }
    }
}
