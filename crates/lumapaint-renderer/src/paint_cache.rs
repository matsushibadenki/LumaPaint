//! Rust-owned committed paint tiles; active strokes are applied temporarily and rolled back.
use super::*;
use std::collections::BTreeMap;

pub(crate) struct PaintCache {
    tiles: TiledRasterDocument,
    image: Option<String>,
    strokes: Vec<Stroke>,
    active_coords: Vec<TileCoord>,
    uploaded: BTreeMap<TileCoord, Vec<u8>>,
    pub replayed: usize,
}

impl PaintCache {
    pub fn new(dimensions: (u32, u32)) -> Result<Self, String> {
        let mut tiles = TiledRasterDocument::new(dimensions.0, dimensions.1)?;
        tiles.add_layer("paint".into(), "Paint".into())?;
        tiles.discard_history();
        Ok(Self {
            tiles,
            image: None,
            strokes: vec![],
            active_coords: vec![],
            uploaded: BTreeMap::new(),
            replayed: 0,
        })
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.tiles.layers()[0]
            .effects
            .apply(self.tiles.layers()[0].tiles.pixel(x, y).unwrap_or([0; 4]))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.tiles.dimensions()
    }

    pub fn prepare(&mut self, document: &Document, force: bool) -> Result<Vec<TileUpload>, String> {
        let committed: Vec<_> = document.committed_paint_strokes().collect();
        let size_changed = self.dimensions() != document.dimensions();
        let prefix = self.image.as_deref() == document.paint_source()
            && !size_changed
            && self.strokes.len() <= committed.len()
            && self
                .strokes
                .iter()
                .zip(&committed)
                .all(|(old, new)| old == *new);
        let mut dirty: BTreeSet<_> = self.active_coords.iter().copied().collect();
        self.replayed = 0;
        if !prefix {
            // Undo, history replacement and document switches invalidate the retained prefix.
            dirty.extend(self.uploaded.keys().copied());
            let old_uploads = std::mem::take(&mut self.uploaded);
            *self = Self::new(document.dimensions())?;
            if !size_changed {
                self.uploaded = old_uploads;
            } else {
                dirty.clear();
            }
        }
        if self.image.as_deref() != document.paint_source() {
            if let Some(source) = document.paint_source() {
                if let Some(changes) = seed_paint_image(&mut self.tiles, "paint", source, 1)? {
                    dirty.extend(changes.coords);
                }
                self.tiles.discard_history();
            }
            self.image = document.paint_source().map(str::to_owned);
        }
        for stroke in committed.iter().skip(self.strokes.len()) {
            if let Some(changes) = paint_stroke_into_tiles(&mut self.tiles, "paint", stroke)? {
                dirty.extend(changes.coords);
            }
            self.tiles.discard_history();
            self.strokes.push((*stroke).clone());
            self.replayed += 1;
        }
        if let Some(changes) = self.tiles.set_layer_appearance(
            "paint",
            document.background_visible(),
            document.paint_layer_opacity(),
        )? {
            dirty.extend(changes.coords);
        }
        if let Some(changes) = self
            .tiles
            .set_layer_effects("paint", document.layer_effects("layer-1"))?
        {
            dirty.extend(changes.coords);
        }
        self.tiles.discard_history();
        let active = if document.has_active_stroke() {
            document.visible_strokes().last()
        } else {
            None
        };
        let changes = active
            .map(|stroke| paint_stroke_into_tiles(&mut self.tiles, "paint", stroke))
            .transpose()?
            .flatten();
        self.active_coords = changes
            .as_ref()
            .map_or_else(Vec::new, |changes| changes.coords.clone());
        dirty.extend(self.active_coords.iter().copied());
        if force || size_changed {
            dirty.extend(self.tiles.layers()[0].tiles.allocated_coords());
        }
        let uploads = self.tiles.prepare_uploads(&TileInvalidation {
            layer_id: "paint".into(),
            coords: dirty.into_iter().collect(),
        });
        // Roll back only the transient stroke, retaining the committed tile allocation.
        if changes.is_some() {
            self.tiles.undo()?;
        }
        self.tiles.discard_history();
        let mut uploads = uploads?;
        uploads.retain(|upload| {
            force
                || size_changed
                || self.uploaded.get(&upload.coord).map_or_else(
                    || upload.pixels.iter().any(|b| *b != 0),
                    |old| old != &upload.pixels,
                )
        });
        for upload in &uploads {
            if upload.pixels.iter().any(|b| *b != 0) {
                self.uploaded.insert(upload.coord, upload.pixels.clone());
            } else {
                self.uploaded.remove(&upload.coord);
            }
        }
        Ok(uploads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::Brush;
    #[test]
    fn backing_image_is_rendered_erased_and_rebuilt_after_undo() {
        let mut doc = Document::default();
        doc.replace_moved_pixels("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect x=\"10\" y=\"10\" width=\"30\" height=\"30\" fill=\"red\"/></svg>".into(),None).unwrap();
        let mut cache = PaintCache::new(doc.dimensions()).unwrap();
        cache.prepare(&doc, true).unwrap();
        assert_eq!(
            cache.tiles.layers()[0].tiles.pixel(20, 20).unwrap(),
            [255, 0, 0, 255]
        );
        doc.begin_eraser(
            Point { x: 20., y: 20. },
            Brush {
                size: 20.,
                hardness: 1.,
                ..Default::default()
            },
            1.,
        )
        .unwrap();
        doc.finish();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(cache.tiles.layers()[0].tiles.pixel(20, 20).unwrap()[3], 0);
        doc.undo();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(
            cache.tiles.layers()[0].tiles.pixel(20, 20).unwrap(),
            [255, 0, 0, 255]
        );
        doc.undo();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(
            cache.tiles.layers()[0]
                .tiles
                .pixel(20, 20)
                .unwrap_or([0; 4]),
            [0; 4]
        );
    }

    fn stroke(document: &mut Document, x: f32, erase: bool) {
        let brush = Brush {
            simulation: Default::default(),
            envelope: Default::default(),
            size: 20.,
            hardness: 0.5,
            color: [20, 40, 60],
        };
        if erase {
            document
                .begin_eraser(Point { x, y: 30. }, brush, 1.)
                .unwrap();
        } else {
            document.begin(Point { x, y: 30. }, brush).unwrap();
        }
        document
            .extend_with_pressure(Point { x: x + 30., y: 40. }, 1.)
            .unwrap();
        document.finish();
    }
    fn assert_matches(cache: &mut PaintCache, document: &Document) {
        let uploads = cache.prepare(document, true).unwrap();
        let mut expected =
            TiledRasterDocument::new(document.dimensions().0, document.dimensions().1).unwrap();
        expected.add_layer("paint".into(), "Paint".into()).unwrap();
        for stroke in document.visible_strokes() {
            paint_stroke_into_tiles(&mut expected, "paint", stroke).unwrap();
        }
        expected
            .set_layer_appearance("paint", true, document.paint_layer_opacity())
            .unwrap();
        for upload in uploads {
            let expected = expected
                .prepare_uploads(&TileInvalidation {
                    layer_id: "paint".into(),
                    coords: vec![upload.coord],
                })
                .unwrap();
            assert_eq!(upload.pixels, expected[0].pixels);
        }
    }
    #[test]
    fn unchanged_frames_and_append_do_not_replay_history() {
        let mut document = Document::default();
        for i in 0..20 {
            stroke(&mut document, 20. + i as f32 * 5., false);
        }
        stroke(&mut document, 40., true);
        let mut cache = PaintCache::new(document.dimensions()).unwrap();
        assert_matches(&mut cache, &document);
        assert_eq!(cache.replayed, 21);
        assert!(cache.prepare(&document, false).unwrap().is_empty());
        assert_eq!(cache.replayed, 0);
        stroke(&mut document, 80., false);
        cache.prepare(&document, false).unwrap();
        assert_eq!(cache.replayed, 1);
        assert_matches(&mut cache, &document);
    }
    #[test]
    fn active_curve_undo_cut_and_same_size_document_switch_match_full_replay() {
        let mut document = Document::default();
        stroke(&mut document, 30., false);
        stroke(&mut document, 40., true);
        let mut cache = PaintCache::new(document.dimensions()).unwrap();
        assert_matches(&mut cache, &document);
        document
            .begin_eraser(Point { x: 45., y: 30. }, Brush::default(), 1.)
            .unwrap();
        assert_matches(&mut cache, &document);
        assert_eq!(cache.replayed, 0);
        document
            .extend_with_pressure(Point { x: 130., y: 50. }, 1.)
            .unwrap();
        assert_matches(&mut cache, &document);
        assert_eq!(cache.replayed, 0);
        document.finish();
        assert_matches(&mut cache, &document);
        assert_eq!(cache.replayed, 1);
        document.undo();
        assert_matches(&mut cache, &document);
        document.redo();
        assert_matches(&mut cache, &document);
        document.select_all();
        document.cut_paint_selection().unwrap();
        assert_matches(&mut cache, &document);
        let mut other = Document::default();
        stroke(&mut other, 300., false);
        stroke(&mut other, 310., true);
        assert_matches(&mut cache, &other);
        assert_eq!(cache.replayed, 2);
    }
}
