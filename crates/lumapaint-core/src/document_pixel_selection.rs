use super::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedPixelSelection {
    pub name: String,
    pub selection: Arc<Selection>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PixelSelectionRequest {
    pub action: String,
    pub kind: Option<String>,
    pub amount: Option<f32>,
    pub smooth: Option<f32>,
    pub feather: Option<f32>,
    pub contrast: Option<f32>,
    pub shift: Option<f32>,
    pub color: Option<[u8; 3]>,
    pub inverse: Option<bool>,
    pub all_layers: Option<bool>,
    pub output_mask: Option<bool>,
    pub name: Option<String>,
    pub new_name: Option<String>,
    pub mode: Option<SelectionMode>,
    pub matrix: Option<[f32; 6]>,
}
pub(super) fn validate_saved(saved: &[SavedPixelSelection]) -> Result<(), String> {
    if saved.len() > 64
        || serde_json::to_vec(saved).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024
    {
        return Err(
            "Selection library exceeds capacity / 保存選択の容量を超えました / 已保存选区超出容量"
                .into(),
        );
    }
    let mut names = std::collections::BTreeSet::new();
    for item in saved {
        valid_name(&item.name)?;
        item.selection.validate()?;
        if !names.insert(&item.name) {
            return Err("Duplicate selection name / 選択名が重複しています / 选区名称重复".into());
        }
    }
    Ok(())
}
fn valid_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.trim() != name
        || name.chars().count() > 100
        || name.chars().any(char::is_control)
    {
        Err("Invalid selection name / 選択名が不正です / 选区名称无效".into())
    } else {
        Ok(())
    }
}
impl Document {
    pub fn pixel_selection_action(&mut self, request: PixelSelectionRequest) -> Result<(), String> {
        match request.action.as_str() {
            "quickMask" => {
                self.cancel_quick_mask_stroke();
                self.quick_mask = !self.quick_mask;
            }
            "all" => {
                self.commit_path_selection(Some(Selection::new(
                    SelectionShape::Rectangle,
                    [0., 0., self.width as f32, self.height as f32],
                )))?;
            }
            "deselect" => {
                self.commit_path_selection(None)?;
            }
            "reselect" => {
                let s=self.previous_pixel_selection.clone().ok_or("No previous selection / 再選択できる選択範囲がありません / 没有可重新选择的选区")?;
                self.commit_path_selection(Some(s))?;
            }
            "inverse" => {
                let mut s = self
                    .selection
                    .clone()
                    .ok_or("Select an area first / 範囲を選択してください / 请先选择区域")?;
                s.invert(self.width, self.height)?;
                self.commit_path_selection(Some(s))?;
            }
            "load" => {
                let saved = self
                    .saved_pixel_selections
                    .iter()
                    .find(|s| Some(s.name.as_str()) == request.name.as_deref())
                    .ok_or(
                        "Saved selection not found / 保存選択が見つかりません / 找不到已保存的选区",
                    )?;
                let mut result = saved.selection.as_ref().clone();
                if !matches!(request.mode, None | Some(SelectionMode::Replace)) {
                    if let Some(base) = &self.selection {
                        let max = self.width.max(self.height) as f32;
                        let scale = (1024. / max).min(1.);
                        let size = [
                            (self.width as f32 * scale).ceil() as u32,
                            (self.height as f32 * scale).ceil() as u32,
                        ];
                        let sx = self.width as f32 / size[0] as f32;
                        let sy = self.height as f32 / size[1] as f32;
                        let pixels = (0..size[0] * size[1])
                            .map(|i| {
                                let point = Point {
                                    x: (i % size[0]) as f32 * sx + sx * 0.5,
                                    y: (i / size[0]) as f32 * sy + sy * 0.5,
                                };
                                let a = base.coverage(point);
                                let b = result.coverage(point);
                                let value = match request.mode.unwrap() {
                                    SelectionMode::Add => a.max(b),
                                    SelectionMode::Subtract => a * (1. - b),
                                    SelectionMode::Intersect => a.min(b),
                                    _ => b,
                                };
                                (value * 255.).round() as u8
                            })
                            .collect::<Vec<_>>();
                        result = Selection::from_raster(
                            crate::selection_raster::SelectionRaster::from_pixels(
                                size,
                                [sx, 0., 0., sy, 0., 0.],
                                &pixels,
                            )?,
                        )?;
                    } else if matches!(
                        request.mode,
                        Some(SelectionMode::Subtract | SelectionMode::Intersect)
                    ) {
                        return Err(
                            "Select an area first / 範囲を選択してください / 请先选择区域".into(),
                        );
                    }
                }
                self.commit_path_selection(Some(result))?;
            }
            "transform" => {
                let mut result = self
                    .selection
                    .clone()
                    .ok_or("Select an area first / 範囲を選択してください / 请先选择区域")?;
                let [a, b, c, d, e, f] = request.matrix.ok_or("Transform missing")?;
                if [a, b, c, d, e, f]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 32768.)
                    || (a * d - b * c).abs() < 1e-6
                {
                    return Err(
                        "Invalid selection transform / 選択範囲の変形値が不正です / 选区变换无效"
                            .into(),
                    );
                }
                if result.regions.iter().any(|r| {
                    r.shape == SelectionShape::Stroke || r.operation == SelectionOperation::Invert
                }) {
                    result = result.rasterized(self.dimensions(), 1024)?;
                }
                if let Some(mask) = &result.raster {
                    result.raster = Some(Arc::new(mask.transformed([a, b, c, d, e, f])?));
                }
                for region in &mut result.regions {
                    let [x, y, w, h] = region.bounds;
                    let points=match region.shape {
                        SelectionShape::Rectangle=>vec![Point{x,y},Point{x:x+w,y},Point{x:x+w,y:y+h},Point{x,y:y+h}],
                        SelectionShape::Ellipse=>(0..128).map(|i|{let t=i as f32*std::f32::consts::TAU/128.;Point{x:x+w*(0.5+0.5*t.cos()),y:y+h*(0.5+0.5*t.sin())}}).collect(),
                        SelectionShape::Polygon=>region.points.clone(),
                        SelectionShape::Stroke=>return Err("Convert brush selections before transforming / ブラシ選択は輪郭に変換してから変形してください / 请先将画笔选区转换为轮廓".into()),
                    };
                    region.points = points
                        .into_iter()
                        .map(|p| Point {
                            x: a * p.x + c * p.y + e,
                            y: b * p.x + d * p.y + f,
                        })
                        .collect();
                    let minx = region
                        .points
                        .iter()
                        .map(|p| p.x)
                        .fold(f32::INFINITY, f32::min);
                    let maxx = region
                        .points
                        .iter()
                        .map(|p| p.x)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let miny = region
                        .points
                        .iter()
                        .map(|p| p.y)
                        .fold(f32::INFINITY, f32::min);
                    let maxy = region
                        .points
                        .iter()
                        .map(|p| p.y)
                        .fold(f32::NEG_INFINITY, f32::max);
                    region.bounds = [minx, miny, (maxx - minx).max(1.), (maxy - miny).max(1.)];
                    region.shape = SelectionShape::Polygon;
                    region.radius = 0.;
                    if region.operation == SelectionOperation::Invert {
                        return Err("Rasterize inverse selections before transforming / 反転選択は輪郭に変換してから変形してください / 请先转换反选区域".into());
                    }
                }
                result.validate()?;
                self.commit_path_selection(Some(result))?;
            }
            "save" | "update" | "rename" | "delete" => {
                let name = request.name.as_deref().unwrap_or("");
                valid_name(name)?;
                let mut next = self.saved_pixel_selections.as_ref().clone();
                let index = next.iter().position(|s| s.name == name);
                match request.action.as_str() {
                    "save" => {
                        if index.is_some() {
                            return Err(
                                "Selection name exists / 同じ選択名が存在します / 选区名称已存在"
                                    .into(),
                            );
                        }
                        next.push(SavedPixelSelection {
                            name: name.into(),
                            selection: Arc::new(self.selection.clone().ok_or(
                                "Select an area first / 範囲を選択してください / 请先选择区域",
                            )?),
                        });
                    }
                    "update" => {
                        next[index.ok_or("Saved selection missing")?].selection =
                            Arc::new(self.selection.clone().ok_or("Select an area first")?)
                    }
                    "rename" => {
                        let new_name = request.new_name.as_deref().unwrap_or("");
                        valid_name(new_name)?;
                        if new_name != name && next.iter().any(|s| s.name == new_name) {
                            return Err(
                                "Selection name exists / 同じ選択名が存在します / 选区名称已存在"
                                    .into(),
                            );
                        }
                        next[index.ok_or("Saved selection missing")?].name = new_name.into();
                    }
                    "delete" => {
                        next.remove(index.ok_or("Saved selection missing")?);
                    }
                    _ => unreachable!(),
                }
                validate_saved(&next)?;
                if next.as_slice() == self.saved_pixel_selections.as_slice() {
                    return Ok(());
                }
                self.finish();
                let old = std::mem::replace(&mut self.saved_pixel_selections, Arc::new(next));
                self.vector_undo
                    .push(VectorHistoryEntry::SavedPixelSelections(old));
                self.undo_order.push(HistoryKind::Vector);
                self.vector_redo.clear();
                self.redo_order.clear();
                self.revision += 1;
            }
            _ => {
                return Err(
                    "Unknown pixel selection action / 不明な選択操作です / 未知选区操作".into(),
                )
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(action: &str, name: Option<&str>) -> PixelSelectionRequest {
        PixelSelectionRequest {
            action: action.into(),
            name: name.map(str::to_owned),
            ..Default::default()
        }
    }
    #[test]
    fn pixel_selection_menu_roundtrip_reselect_history_and_atomic_errors() {
        let mut d = Document::default();
        d.set_tool_pixel_selection(Some(Selection::new(
            SelectionShape::Rectangle,
            [10., 10., 20., 30.],
        )))
        .unwrap();
        let original = d.selection().cloned();
        d.pixel_selection_action(request("save", Some("Subject")))
            .unwrap();
        d.pixel_selection_action(request("deselect", None)).unwrap();
        assert!(d.selection().is_none());
        d.pixel_selection_action(request("reselect", None)).unwrap();
        assert_eq!(d.selection(), original.as_ref());
        d.pixel_selection_action(request("all", None)).unwrap();
        d.pixel_selection_action(request("load", Some("Subject")))
            .unwrap();
        assert_eq!(d.selection(), original.as_ref());
        let state = d.document_state();
        let mut loaded = Document::from_document_state(
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap(),
        )
        .unwrap();
        loaded
            .pixel_selection_action(request("load", Some("Subject")))
            .unwrap();
        assert_eq!(loaded.selection(), original.as_ref());
        let before = serde_json::to_vec(&d.document_state()).unwrap();
        assert!(d
            .pixel_selection_action(request("save", Some("Subject")))
            .is_err());
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        d.pixel_selection_action(request("delete", Some("Subject")))
            .unwrap();
        d.undo();
        assert_eq!(d.saved_pixel_selections.len(), 1);
        d.redo();
        assert!(d.saved_pixel_selections.is_empty());
    }
    #[test]
    fn pixel_selection_transform_preserves_artwork() {
        let mut d = Document::default();
        d.set_tool_pixel_selection(Some(Selection::new(
            SelectionShape::Ellipse,
            [10., 10., 20., 30.],
        )))
        .unwrap();
        let original = d.selection().cloned();
        d.pixel_selection_action(PixelSelectionRequest {
            matrix: Some([1., 0., 0., 1., 20., 0.]),
            ..request("transform", None)
        })
        .unwrap();
        assert!(d.selection().unwrap().contains(Point { x: 40., y: 25. }));
        assert!(!d.selection().unwrap().contains(Point { x: 20., y: 25. }));
        assert_eq!(d.strokes.len(), 0);
        d.undo();
        assert_eq!(d.selection(), original.as_ref());
    }
}

#[derive(Clone)]
pub(super) struct QuickMaskGesture {
    base: Option<Selection>,
    points: Vec<Point>,
    radius: f32,
    value: f32,
    hardness: f32,
}
impl Document {
    pub fn quick_mask_active(&self) -> bool {
        self.quick_mask
    }
    pub fn cancel_quick_mask_stroke(&mut self) -> bool {
        if let Some(gesture) = self.quick_mask_stroke.take() {
            self.selection = gesture.base;
            true
        } else {
            false
        }
    }
    pub fn quick_mask_pointer(
        &mut self,
        point: Point,
        phase: u8,
        radius: f32,
        value: f32,
        hardness: f32,
    ) -> Result<(), String> {
        if !self.quick_mask {
            return Err("Quick mask is inactive".into());
        }
        if !point.x.is_finite()
            || !point.y.is_finite()
            || !radius.is_finite()
            || !(0. ..=1.).contains(&value)
            || !(0. ..=1.).contains(&hardness)
            || !(0.5..=512.).contains(&radius)
        {
            return Err("Invalid quick mask stroke".into());
        }
        if phase == 0 {
            self.cancel_quick_mask_stroke();
            self.quick_mask_stroke = Some(QuickMaskGesture {
                base: self.selection.clone(),
                points: vec![point],
                radius,
                value,
                hardness,
            });
        }
        let Some(mut gesture) = self.quick_mask_stroke.take() else {
            return Ok(());
        };
        if gesture.points.last() != Some(&point) {
            gesture.points.push(point);
        }
        if gesture.points.len() > 32768 {
            self.selection = gesture.base;
            return Err("Quick mask stroke is too long".into());
        }
        self.selection = Some(gesture.base.clone().unwrap_or_else(|| {
            Selection::new(
                SelectionShape::Rectangle,
                [0., 0., self.width as f32, self.height as f32],
            )
        }));
        let result =
            self.set_path_selection(gesture.points.clone(), gesture.radius, SelectionMode::Add);
        if let Err(e) = result {
            self.selection = gesture.base;
            return Err(e);
        }
        if let Some(region) = self.selection.as_mut().and_then(|s| s.regions.last_mut()) {
            region.paint = Some([gesture.value, gesture.hardness]);
        }
        if phase == 2 {
            let result = self.selection.clone();
            self.selection = gesture.base;
            self.commit_path_selection(result)?;
        } else {
            self.quick_mask_stroke = Some(gesture);
        }
        Ok(())
    }
}
#[cfg(test)]
mod quick_tests {
    use super::*;
    #[test]
    fn quick_mask_paints_selection_only_and_one_gesture_is_undoable() {
        let mut d = Document::default();
        let request = PixelSelectionRequest {
            action: "quickMask".into(),
            ..Default::default()
        };
        d.pixel_selection_action(request.clone()).unwrap();
        assert!(d.quick_mask_active());
        let revision = d.revision();
        d.quick_mask_pointer(Point { x: 20., y: 20. }, 0, 5., 0., 1.)
            .unwrap();
        d.quick_mask_pointer(Point { x: 30., y: 20. }, 2, 5., 0., 1.)
            .unwrap();
        assert_eq!(d.revision(), revision + 1);
        assert!(!d.selection().unwrap().contains(Point { x: 25., y: 20. }));
        assert!(d.selection().unwrap().contains(Point { x: 50., y: 50. }));
        assert!(d.strokes.is_empty());
        d.undo();
        assert!(d.selection().is_none());
        d.redo();
        assert!(d.selection().is_some());
        let before = d.selection().cloned();
        d.quick_mask_pointer(Point { x: 50., y: 50. }, 0, 4., 0., 1.)
            .unwrap();
        assert!(d.cancel_quick_mask_stroke());
        assert_eq!(d.selection(), before.as_ref());
        d.pixel_selection_action(request).unwrap();
        assert!(!d.quick_mask_active());
    }
}

#[cfg(test)]
mod gray_tests {
    use super::*;
    #[test]
    fn selection_quick_mask_gray_soft_brush_and_saved_state() {
        let mut d = Document::default();
        d.pixel_selection_action(PixelSelectionRequest {
            action: "quickMask".into(),
            ..Default::default()
        })
        .unwrap();
        d.quick_mask_pointer(Point { x: 30., y: 30. }, 0, 10., 0.25, 0.)
            .unwrap();
        d.quick_mask_pointer(Point { x: 30., y: 30. }, 2, 10., 0.25, 0.)
            .unwrap();
        let selection = d.selection().unwrap();
        assert!((selection.coverage(Point { x: 30., y: 30. }) - 0.25).abs() < 0.001);
        assert!(
            selection.coverage(Point { x: 35., y: 30. }) > 0.25
                && selection.coverage(Point { x: 35., y: 30. }) < 1.
        );
        assert_eq!(selection.coverage(Point { x: 50., y: 50. }), 1.);
        let loaded = Document::from_document_state(d.document_state()).unwrap();
        assert!(!loaded.quick_mask_active());
        assert_eq!(loaded.selection(), d.selection());
    }
}
