use super::*;
use crate::image_frame::{FrameFit, FrameImage, ImageFrame};
impl Document {
    pub fn image_frame_object(&self, id: &str) -> Result<(&SvgLayer, &VectorObject), String> {
        self.svg_layers
            .iter()
            .find_map(|l| {
                l.vector_objects
                    .iter()
                    .find(|o| o.id == id && o.image_frame.is_some())
                    .map(|o| (l, o))
            })
            .ok_or_else(|| {
                "Image frame not found / 画像フレームが見つかりません / 找不到图像框架".into()
            })
    }
    pub fn edit_image_frame(&mut self, id: &str, frame: ImageFrame) -> Result<(), String> {
        frame.validate()?;
        let (layer, object) = self.image_frame_object(id)?;
        if layer.locked || !layer.visible || !object.visible || self.object_is_locked(id) {
            return Err(
                "Frame is locked or hidden / フレームがロックまたは非表示です / 框架已锁定或隐藏"
                    .into(),
            );
        }
        let layer = layer.id.clone();
        let mut object = object.clone();
        object.image_frame = Some(frame);
        self.upsert_vector_object(&layer, object)
    }
    pub fn fit_image_frame(&mut self, id: &str, fitting: FrameFit) -> Result<(), String> {
        let (_, object) = self.image_frame_object(id)?;
        let mut frame = object.image_frame.clone().unwrap();
        frame.fit(
            crate::stroke::path_bounds(&object.path.data).ok_or("Invalid frame path")?,
            fitting,
        )?;
        self.edit_image_frame(id, frame)
    }
    pub fn move_frame_content(&mut self, id: &str, offset: [f32; 2]) -> Result<(), String> {
        if offset.iter().any(|v| !v.is_finite() || v.abs() > 100_000.) {
            return Err("Invalid image offset".into());
        }
        let (_, object) = self.image_frame_object(id)?;
        let mut frame = object.image_frame.clone().unwrap();
        let mut matrix = frame.content_transform.ok_or("Empty frame")?;
        matrix[4] += offset[0];
        matrix[5] += offset[1];
        frame.content_transform = Some(matrix);
        self.edit_image_frame(id, frame)
    }
    pub fn place_frame_image(
        &mut self,
        target: Option<&str>,
        image: FrameImage,
        new_id: &str,
    ) -> Result<String, String> {
        if let Some(id) = target {
            let (_, object) = self.image_frame_object(id)?;
            let mut frame = object.image_frame.clone().unwrap();
            let fitting = frame.fitting;
            frame.image = Some(image);
            frame.fit(
                crate::stroke::path_bounds(&object.path.data).ok_or("Invalid frame path")?,
                fitting,
            )?;
            self.edit_image_frame(id, frame)?;
            return Ok(id.into());
        }
        let scale = (self.width as f32 / image.width as f32)
            .min(self.height as f32 / image.height as f32)
            .min(1.);
        let w = image.width as f32 * scale;
        let h = image.height as f32 * scale;
        let mut frame = ImageFrame {
            image: Some(image),
            ..Default::default()
        };
        frame.fit([0., 0., w, h], FrameFit::Contain)?;
        let object:VectorObject=serde_json::from_value(serde_json::json!({"id":new_id,"name":frame.image.as_ref().unwrap().name.chars().take(120).collect::<String>(),"path":{"data":format!("M0 0H{w}V{h}H0Z"),"fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":null,"stroke":null,"strokeWidth":0,"visible":true,"kind":"rectangle","controlPoints":[[0,0],[w,h]],"imageFrame":frame})).map_err(|e|e.to_string())?;
        self.insert_image_frame(object)?;
        Ok(new_id.into())
    }

    /// All link changes commit together, preserving placement and one Undo entry.
    pub fn manage_frame_images(
        &mut self,
        updates: Vec<(String, Option<FrameImage>)>,
    ) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut layers = self.svg_layers.clone();
        for (id, image) in updates {
            if !seen.insert(id.clone()) {
                return Err("Duplicate link target".into());
            }
            let (layer, old) = self.image_frame_object(&id)?;
            if layer.locked || !layer.visible || !old.visible || self.object_is_locked(&id) {
                return Err("Frame is locked or hidden / フレームがロックまたは非表示です / 框架已锁定或隐藏".into());
            }
            let object = layers
                .iter_mut()
                .flat_map(|l| l.vector_objects.iter_mut())
                .find(|o| o.id == id)
                .ok_or("Frame not found")?;
            let frame = object.image_frame.as_mut().ok_or("Not an image frame")?;
            let old = frame.image.as_ref().ok_or("Empty frame")?;
            if let Some(image) = image {
                if let Some(matrix) = frame.content_transform {
                    frame.content_transform = Some(crate::image_frame::multiply(
                        matrix,
                        [
                            old.width as f32 / image.width as f32,
                            0.,
                            0.,
                            old.height as f32 / image.height as f32,
                            0.,
                            0.,
                        ],
                    ));
                }
                frame.image = Some(image);
            } else {
                frame.image.as_mut().unwrap().source_path = None;
            }
            object.validate()?;
        }
        for layer in &mut layers {
            if layer.vector_objects.iter().any(|o| seen.contains(&o.id)) {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much SVG data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn insert_image_frame(&mut self, object: VectorObject) -> Result<(), String> {
        if object.image_frame.is_none() {
            return Err("Expected image frame".into());
        }
        let mut draft = self.clone();
        let selected = draft
            .selected_layer
            .as_ref()
            .and_then(|id| draft.svg_layers.iter().find(|l| &l.id == id));
        if selected.is_some_and(|l| l.locked || !l.visible) {
            return Err("Selected layer is locked or hidden / 選択レイヤーがロックまたは非表示です / 所选图层已锁定或隐藏".into());
        }
        let target = selected
            .filter(|l| l.vector_layer)
            .map(|l| l.id.clone())
            .or_else(|| draft.editable_vector_layer_id());
        let layer = match target {
            Some(id) => id,
            None => draft.add_vector_layer()?,
        };
        let id = object.id.clone();
        draft.select_layer(layer.clone())?;
        draft.upsert_vector_object(&layer, object)?;
        draft.select_vector_objects(vec![id])?;
        self.finish();
        let before = self.vector_history_state();
        self.restore_vector_history(draft.vector_history_state());
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> FrameImage {
        FrameImage {
            source_path: Some("/image.png".into()),
            fingerprint: "original".into(),
            name: "Image".into(),
            width: 200,
            height: 100,
            encoded_width: 200,
            encoded_height: 100,
            orientation_transform: [1., 0., 0., 1., 0., 0.],
            data_uri: "data:image/png;base64,AA==".into(),
        }
    }
    #[test]
    fn placement_is_one_edit_and_cached_image_survives_save() {
        let mut d = Document::default();
        let before = d.svg_layers().count();
        d.place_frame_image(None, image(), "frame").unwrap();
        assert!(d.image_frame_object("frame").is_ok());
        let loaded = Document::decode(&d.encode().unwrap()).unwrap();
        assert_eq!(
            loaded
                .image_frame_object("frame")
                .unwrap()
                .1
                .image_frame
                .as_ref()
                .unwrap()
                .image
                .as_ref()
                .unwrap()
                .source_path
                .as_deref(),
            Some("/image.png")
        );
        assert!(!serde_json::to_string(&d.snapshot())
            .unwrap()
            .contains("data:image"));
        d.undo();
        assert_eq!(d.svg_layers().count(), before);
        d.redo();
        assert!(d.image_frame_object("frame").is_ok());
    }
    #[test]
    fn replacement_fitting_and_crop_preserve_frame() {
        let mut d = Document::default();
        d.place_frame_image(None, image(), "frame").unwrap();
        let path = d.image_frame_object("frame").unwrap().1.path.clone();
        d.fit_image_frame("frame", FrameFit::Cover).unwrap();
        d.move_frame_content("frame", [10., 20.]).unwrap();
        assert_eq!(d.image_frame_object("frame").unwrap().1.path, path);
        let mut i = image();
        i.width = 100;
        i.height = 200;
        i.fingerprint = "updated".into();
        d.place_frame_image(Some("frame"), i, "").unwrap();
        assert_eq!(d.image_frame_object("frame").unwrap().1.path, path);
        assert!(d.svg_layers().next().unwrap().source.contains("clipPath"));
        d.undo();
        assert_eq!(
            d.image_frame_object("frame")
                .unwrap()
                .1
                .image_frame
                .as_ref()
                .unwrap()
                .image
                .as_ref()
                .unwrap()
                .fingerprint,
            "original"
        );
    }
    #[test]
    fn resizing_crop_preserves_image_but_scaling_transforms_both() {
        let mut d = Document::default();
        d.place_frame_image(None, image(), "frame").unwrap();
        let world = |d: &Document| {
            let o = d.image_frame_object("frame").unwrap().1;
            crate::image_frame::multiply(
                o.transform,
                o.image_frame.as_ref().unwrap().content_transform.unwrap(),
            )
        };
        let initial = world(&d);
        d.resize_selected_image_frames([2., 0., 0., 3., 10., 20.], false)
            .unwrap();
        assert_eq!(world(&d), initial);
        d.undo();
        d.resize_selected_image_frames([2., 0., 0., 3., 10., 20.], true)
            .unwrap();
        assert_eq!(
            world(&d),
            crate::image_frame::multiply([2., 0., 0., 3., 10., 20.], initial)
        );
        d.undo();

        assert!(d.move_frame_content("frame", [f32::NAN, 1.]).is_err());
    }
    #[test]
    fn placing_with_background_selected_creates_one_undoable_frame_layer() {
        let mut d = Document::default();
        let background = d.snapshot().layers.last().unwrap().id.clone();
        d.select_layer(background).unwrap();
        d.place_frame_image(None, image(), "frame").unwrap();
        assert!(d.image_frame_object("frame").is_ok());
        d.undo();
        assert!(d.image_frame_object("frame").is_err());
    }
    #[test]
    fn batch_links_are_atomic_preserve_crop_and_undo_once() {
        let mut d = Document::default();
        d.place_frame_image(None, image(), "a").unwrap();
        d.place_frame_image(None, image(), "b").unwrap();
        d.move_frame_content("a", [12., 23.]).unwrap();
        let before = d.encode().unwrap();
        let original = d
            .image_frame_object("a")
            .unwrap()
            .1
            .image_frame
            .as_ref()
            .unwrap()
            .content_transform
            .unwrap();
        let mut next = image();
        next.width = 400;
        next.height = 200;
        next.fingerprint = "new".into();
        assert!(d
            .manage_frame_images(vec![
                ("a".into(), Some(next.clone())),
                ("missing".into(), Some(next.clone()))
            ])
            .is_err());
        assert_eq!(d.encode().unwrap(), before);
        d.manage_frame_images(vec![("a".into(), Some(next)), ("b".into(), None)])
            .unwrap();
        let frame = d
            .image_frame_object("a")
            .unwrap()
            .1
            .image_frame
            .as_ref()
            .unwrap();
        assert_eq!(frame.content_transform.unwrap()[4..], original[4..]);
        assert_eq!(frame.content_transform.unwrap()[0], original[0] * 0.5);
        assert!(d
            .image_frame_object("b")
            .unwrap()
            .1
            .image_frame
            .as_ref()
            .unwrap()
            .image
            .as_ref()
            .unwrap()
            .source_path
            .is_none());
        d.undo();
        assert_eq!(d.encode().unwrap(), before);
        d.redo();
        assert_eq!(
            d.image_frame_object("a")
                .unwrap()
                .1
                .image_frame
                .as_ref()
                .unwrap()
                .image
                .as_ref()
                .unwrap()
                .fingerprint,
            "new"
        );
    }
    #[test]
    fn batch_link_management_rejects_locked_targets_and_duplicate_ids() {
        let mut d = Document::default();
        d.place_frame_image(None, image(), "a").unwrap();
        d.place_frame_image(None, image(), "b").unwrap();
        d.lock_objects(ObjectLockAction::Selection).unwrap();
        let before = d.encode().unwrap();
        assert!(d
            .manage_frame_images(vec![("a".into(), None), ("b".into(), None)])
            .is_err());
        assert_eq!(d.encode().unwrap(), before);
        assert!(d
            .manage_frame_images(vec![("a".into(), None), ("a".into(), None)])
            .is_err());
        assert_eq!(d.encode().unwrap(), before);
    }
}
