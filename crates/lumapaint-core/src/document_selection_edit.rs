//! Interactive selection gestures, sampling and commit to document history.
use super::{
    Document, HistoryKind, Point, Selection, SelectionGesture, SelectionMode, SelectionOperation,
    SelectionRegion, SelectionShape, VectorHistoryEntry,
};

impl Document {
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    /// Start a fresh shape. Interactive editing should use begin_selection_edit.
    pub fn begin_selection(&mut self, point: Point, shape: SelectionShape) {
        self.finish();
        self.selection_anchor = None;
        if !point.valid() || !point.on_page(self.width, self.height) {
            return;
        }
        self.selection_anchor = Some(SelectionGesture {
            vector_layer: None,
            start: point,
            shape,
            mode: SelectionMode::Replace,
            moving: false,
            base: self.selection.clone(),
        });
    }
    pub fn begin_selection_edit(
        &mut self,
        point: Point,
        shape: SelectionShape,
        mode: SelectionMode,
    ) -> Result<(), String> {
        self.finish();
        self.selection_anchor = None;
        if !point.valid() || !point.on_page(self.width, self.height) {
            return Ok(());
        }
        let vector_layer = self
            .svg_layers
            .iter()
            .find(|layer| Some(&layer.id) == self.selected_layer.as_ref() && layer.vector_layer);
        if vector_layer.is_some_and(|layer| layer.locked || !layer.visible) {
            return Err("Select an unlocked visible layer / ロックされていない表示中のレイヤーを選択してください / 请选择未锁定且可见的图层".into());
        }
        let vector_layer = vector_layer.map(|layer| layer.id.clone());
        let moving = vector_layer.is_none()
            && mode == SelectionMode::Replace
            && self.selection.as_ref().is_some_and(|s| s.contains(point));
        if vector_layer.is_none() && mode != SelectionMode::Replace {
            if let Some(selection) = &self.selection {
                selection.ensure_capacity()?;
            }
        }
        self.selection_anchor = Some(SelectionGesture {
            vector_layer,
            start: point,
            shape,
            mode,
            moving,
            base: self.selection.clone(),
        });
        Ok(())
    }
    pub fn extend_selection(&mut self, point: Point, finish: bool) {
        if !point.valid() {
            return;
        }
        if let Some(SelectionGesture {
            vector_layer: Some(layer_id),
            start,
            shape,
            mode,
            ..
        }) = &self.selection_anchor
        {
            let bounds = [
                start.x.min(point.x),
                start.y.min(point.y),
                (start.x - point.x).abs(),
                (start.y - point.y).abs(),
            ];
            let has_area = bounds[2] >= 1. && bounds[3] >= 1.;
            self.selection = has_area.then(|| Selection::new(*shape, bounds));
            if finish {
                let mut hits = Vec::new();
                if self.selected_layer.as_ref() == Some(layer_id) && has_area {
                    if let Some(layer) = self
                        .svg_layers
                        .iter()
                        .find(|l| &l.id == layer_id && l.visible && !l.locked)
                    {
                        for object in &layer.vector_objects {
                            if object.visible
                                && !self.object_is_locked(&object.id)
                                && !object.control_points.is_empty()
                                && object
                                    .intersects_selection(bounds, *shape == SelectionShape::Ellipse)
                            {
                                hits.push(object.id.clone());
                            }
                        }
                    }
                }
                if self.selected_layer.as_ref() == Some(layer_id) {
                    let hits = self.expand_group_selection(&hits);
                    self.selected_vector_objects = match mode {
                        SelectionMode::Replace => hits,
                        SelectionMode::Add => {
                            let mut ids = self.selected_vector_objects.clone();
                            for id in hits {
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                            ids
                        }
                        SelectionMode::Intersect => self
                            .selected_vector_objects
                            .iter()
                            .filter(|id| hits.contains(id))
                            .cloned()
                            .collect(),
                        SelectionMode::Subtract => self
                            .selected_vector_objects
                            .iter()
                            .filter(|id| !hits.contains(id))
                            .cloned()
                            .collect(),
                    };
                }
                self.selection = self
                    .selection_anchor
                    .take()
                    .and_then(|gesture| gesture.base);
            }
            return;
        }
        if let Some(gesture) = &self.selection_anchor {
            let start = gesture.start;
            if gesture.moving {
                self.selection = gesture
                    .base
                    .as_ref()
                    .map(|selection| selection.translated(point.x - start.x, point.y - start.y));
            } else {
                let x = point.x.clamp(0.0, self.width as f32);
                let y = point.y.clamp(0.0, self.height as f32);
                let bounds = [
                    start.x.min(x),
                    start.y.min(y),
                    (start.x - x).abs(),
                    (start.y - y).abs(),
                ];
                if bounds[2] >= 1.0 && bounds[3] >= 1.0 {
                    let region = SelectionRegion {
                        paint: None,
                        points: Vec::new(),
                        radius: 0.,
                        shape: gesture.shape,
                        bounds,
                        operation: if gesture.mode == SelectionMode::Add {
                            SelectionOperation::Add
                        } else if gesture.mode == SelectionMode::Intersect {
                            SelectionOperation::Intersect
                        } else {
                            SelectionOperation::Subtract
                        },
                    };
                    self.selection = Some(match gesture.mode {
                        SelectionMode::Replace => Selection::new(gesture.shape, bounds),
                        SelectionMode::Add | SelectionMode::Intersect => match &gesture.base {
                            Some(base) => {
                                let mut result = base.clone();
                                result.regions.push(region);
                                result
                            }
                            None => Selection::new(gesture.shape, bounds),
                        },
                        SelectionMode::Subtract => {
                            let mut result = gesture.base.clone().unwrap_or_else(|| {
                                Selection::new(
                                    SelectionShape::Rectangle,
                                    [0.0, 0.0, self.width as f32, self.height as f32],
                                )
                            });
                            result.regions.push(region);
                            result
                        }
                    });
                } else {
                    self.selection = if gesture.mode == SelectionMode::Replace {
                        None
                    } else {
                        gesture.base.clone()
                    };
                }
            }
        }
        if finish {
            self.selection_anchor = None;
        }
    }
    pub fn cancel_selection_gesture(&mut self) -> bool {
        if let Some(gesture) = self.selection_anchor.take() {
            self.selection = gesture.base;
            true
        } else {
            false
        }
    }
    pub fn deselect(&mut self) {
        self.guides.selected.clear();
        self.finish();
        if self.selection.is_some() {
            self.previous_pixel_selection = self.selection.clone();
        }
        self.selection = None;
        self.selection_anchor = None;
    }
    pub fn select_all(&mut self) {
        self.deselect();
        self.selection = Some(Selection::new(
            SelectionShape::Rectangle,
            [0.0, 0.0, self.width as f32, self.height as f32],
        ));
    }
    pub fn invert_selection(&mut self) -> Result<(), String> {
        self.finish();
        self.selection_anchor = None;
        if let Some(selection) = self.selection.as_mut() {
            selection.invert(self.width, self.height)?;
        }
        Ok(())
    }

    pub fn set_path_selection(
        &mut self,
        points: Vec<Point>,
        radius: f32,
        mode: SelectionMode,
    ) -> Result<(), String> {
        if points.len() < if radius > 0. { 1 } else { 3 } {
            return Ok(());
        }
        let padding = radius;
        let minx = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - padding;
        let miny = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - padding;
        let maxx = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) + padding;
        let maxy = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) + padding;
        let mut region = SelectionRegion {
            paint: None,
            shape: if radius > 0. {
                SelectionShape::Stroke
            } else {
                SelectionShape::Polygon
            },
            bounds: [
                minx,
                miny,
                (maxx - minx).max(0.001),
                (maxy - miny).max(0.001),
            ],
            points,
            radius,
            operation: SelectionOperation::Replace,
        };
        let mut result = match mode {
            SelectionMode::Replace => Selection {
                raster: None,
                regions: Vec::new(),
            },
            SelectionMode::Add | SelectionMode::Intersect => {
                self.selection.clone().unwrap_or(Selection {
                    raster: None,
                    regions: Vec::new(),
                })
            }
            SelectionMode::Subtract => self.selection.clone().unwrap_or_else(|| {
                Selection::new(
                    SelectionShape::Rectangle,
                    [0., 0., self.width as f32, self.height as f32],
                )
            }),
        };
        result.ensure_capacity()?;
        region.operation = if result.regions.is_empty() && result.raster.is_none() {
            SelectionOperation::Replace
        } else if mode == SelectionMode::Intersect {
            SelectionOperation::Intersect
        } else if mode == SelectionMode::Subtract {
            SelectionOperation::Subtract
        } else {
            SelectionOperation::Add
        };
        result.regions.push(region);
        result.validate()?;
        self.selection = Some(result);
        Ok(())
    }

    pub fn commit_path_selection(&mut self, selection: Option<Selection>) -> Result<bool, String> {
        if let Some(s) = &selection {
            s.validate()?;
        }
        if self.selection == selection {
            return Ok(false);
        }
        self.finish();
        let previous = std::mem::replace(&mut self.selection, selection);
        if previous.is_some() {
            self.previous_pixel_selection = previous.clone();
        }
        self.vector_undo
            .push(VectorHistoryEntry::PixelSelection(previous));
        self.vector_redo.clear();
        self.redo.clear();
        self.redo_order.clear();
        self.undo_order.push(HistoryKind::Vector);
        self.revision += 1;
        Ok(true)
    }
    pub fn set_tool_pixel_selection(&mut self, selection: Option<Selection>) -> Result<(), String> {
        if let Some(s) = &selection {
            s.validate()?;
        }
        self.selection = selection;
        Ok(())
    }

    pub fn selection_sampling_document(&self, all_layers: bool) -> Self {
        let mut result = self.clone_for_rendering();
        result.finish();
        result.selection = None;
        if !all_layers {
            if let Some(id) = &self.selected_layer {
                result.paint_source = None;
                result.strokes.clear();
                result.point_count = 0;
                result.svg_layers.retain(|l| &l.id == id);
            } else {
                result.svg_layers.clear();
            }
        }
        result
    }
}
