//! Rust-owned committed paint tiles; active strokes are applied temporarily and rolled back.
use super::*;
use std::collections::BTreeMap;

pub(crate) struct PaintCache {
    tiles: TiledRasterDocument,
    image: Option<String>,
    strokes: Vec<Stroke>,
    committed_key: Option<(u64, u64)>,
    pub compared_strokes: usize,
    active_coords: Vec<TileCoord>,
    active: Option<ActiveTiles>,
    pub active_tiles_rasterized: usize,
    uploaded: BTreeMap<TileCoord, Vec<u8>>,
    pub replayed: usize,
    paper: bool,
}

// At most 8 MiB of dab payload retained per paint cache. Large tips / long strokes
// bypass this optimization; sampling still uses the established whole-path algorithm.
const MAX_ACTIVE_DAB_REFS: usize = 262_144;

#[derive(PartialEq)]
struct ActiveStyle {
    eraser: bool,
    color: [u8; 3],
    selection: Option<lumapaint_core::selection::Selection>,
}

struct ActiveTiles {
    style: ActiveStyle,
    dabs: BTreeMap<TileCoord, Vec<RasterDab>>,
}

impl ActiveTiles {
    fn new(stroke: &Stroke, dabs: &[RasterDab], size: (u32, u32)) -> Option<Self> {
        let mut result = Self {
            style: ActiveStyle {
                eraser: stroke.eraser,
                color: stroke.brush.color,
                selection: stroke.selection.clone(),
            },
            dabs: BTreeMap::new(),
        };
        let mut count = 0;
        for dab in dabs {
            if dab.weight == 0.0 {
                continue;
            }
            let fringe = 2.0 * dab.texture_scale;
            let left = (dab.x - dab.radius - fringe).floor().max(0.) as u32;
            let top = (dab.y - dab.radius - fringe).floor().max(0.) as u32;
            let right = (dab.x + dab.radius + fringe)
                .ceil()
                .max(0.)
                .min(size.0 as f32) as u32;
            let bottom = (dab.y + dab.radius + fringe)
                .ceil()
                .max(0.)
                .min(size.1 as f32) as u32;
            if left >= right || top >= bottom {
                continue;
            }
            for y in top / TILE_SIZE..=(bottom - 1) / TILE_SIZE {
                for x in left / TILE_SIZE..=(right - 1) / TILE_SIZE {
                    count += 1;
                    if count > MAX_ACTIVE_DAB_REFS {
                        return None;
                    }
                    result
                        .dabs
                        .entry(TileCoord { x, y })
                        .or_default()
                        .push(*dab);
                }
            }
        }
        Some(result)
    }
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
            committed_key: None,
            compared_strokes: 0,
            active_coords: vec![],
            active: None,
            active_tiles_rasterized: 0,
            uploaded: BTreeMap::new(),
            replayed: 0,
            paper: false,
        })
    }

    pub fn raw_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.tiles.layers()[0].tiles.pixel(x, y).unwrap_or([0; 4])
    }
    fn paper_uploads(
        &self,
        document: &Document,
        coords: &BTreeSet<TileCoord>,
    ) -> Result<Vec<TileUpload>, String> {
        let (w, h) = document.dimensions();
        let mut effects = document.layer_effects("layer-1");
        let mask = effects.mask.take();
        let effects = effects.prepare();
        let opacity = if document.background_visible() {
            document.paint_layer_opacity()
        } else {
            0.
        };
        Ok(coords
            .iter()
            .filter(|c| c.x * TILE_SIZE < w && c.y * TILE_SIZE < h)
            .map(|coord| {
                let origin = [coord.x * TILE_SIZE, coord.y * TILE_SIZE];
                let extent = [TILE_SIZE.min(w - origin[0]), TILE_SIZE.min(h - origin[1])];
                let mut pixels = vec![0; (extent[0] * extent[1] * 4) as usize];
                for y in 0..extent[1] {
                    for x in 0..extent[0] {
                        let px = origin[0] + x;
                        let py = origin[1] + y;
                        let pixel = crate::root_paint::composite(
                            self.raw_pixel(px, py),
                            [px as f32 + 0.5, py as f32 + 0.5],
                            &effects,
                            mask.as_ref(),
                            true,
                        )
                        .map(|v| (v as f32 * opacity).round() as u8);
                        let at = ((y * extent[0] + x) * 4) as usize;
                        pixels[at..at + 4].copy_from_slice(&pixel);
                    }
                }
                TileUpload {
                    coord: *coord,
                    origin,
                    extent,
                    bytes_per_row: extent[0] * 4,
                    pixels,
                }
            })
            .collect())
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.tiles.dimensions()
    }

    pub fn prepare(&mut self, document: &Document, force: bool) -> Result<Vec<TileUpload>, String> {
        let key = (document.scene_journal().instance_id(), document.revision());
        let size_changed = self.dimensions() != document.dimensions();
        let unchanged = self.committed_key == Some(key) && !size_changed;
        let committed =
            (!unchanged).then(|| document.committed_paint_strokes().collect::<Vec<_>>());
        self.compared_strokes = 0;
        let prefix = unchanged
            || committed.as_ref().is_some_and(|committed| {
                self.image.as_deref() == document.paint_source()
                    && !size_changed
                    && self.strokes.len() <= committed.len()
                    && self.strokes.iter().zip(committed).all(|(old, new)| {
                        self.compared_strokes += 1;
                        old == *new
                    })
            });
        let mut dirty = BTreeSet::new();
        self.active_tiles_rasterized = 0;
        self.replayed = 0;
        if !prefix {
            // Undo, history replacement and document switches invalidate the retained prefix.
            dirty.extend(self.active_coords.iter().copied());
            dirty.extend(self.uploaded.keys().copied());
            let old_uploads = std::mem::take(&mut self.uploaded);
            *self = Self::new(document.dimensions())?;
            if !size_changed {
                self.uploaded = old_uploads;
            } else {
                dirty.clear();
            }
        }
        if !unchanged && self.image.as_deref() != document.paint_source() {
            if let Some(source) = document.paint_source() {
                if let Some(changes) = seed_paint_image(&mut self.tiles, "paint", source, 1)? {
                    dirty.extend(changes.coords);
                }
                self.tiles.discard_history();
            }
            self.image = document.paint_source().map(str::to_owned);
        }
        for stroke in committed.iter().flatten().skip(self.strokes.len()) {
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
            dirty.extend(self.active_coords.iter().copied());
        }
        if let Some(changes) = self
            .tiles
            .set_layer_effects("paint", document.layer_effects("layer-1"))?
        {
            dirty.extend(changes.coords);
            dirty.extend(self.active_coords.iter().copied());
        }
        self.tiles.discard_history();
        let active = document.active_paint_stroke();
        // Keep the committed tiles untouched between calls. Only changed active tiles
        // are composited temporarily; unchanged active pixels already live in the display.
        let sampled = active
            .filter(|stroke| !stroke.clear)
            .map(sampled_raster_dabs)
            .transpose()?;
        let next = active
            .zip(sampled.as_deref())
            .and_then(|(stroke, dabs)| ActiveTiles::new(stroke, dabs, document.dimensions()));
        if let Some(next) = &next {
            let compatible = self.active.as_ref().filter(|old| old.style == next.style);
            for (coord, dabs) in &next.dabs {
                if compatible.and_then(|old| old.dabs.get(coord)) != Some(dabs) {
                    dirty.insert(*coord);
                }
            }
            for coord in &self.active_coords {
                if !next.dabs.contains_key(coord) {
                    dirty.insert(*coord);
                }
            }
            if force {
                dirty.extend(next.dabs.keys().copied());
            }
        } else {
            // Clear operations and unusually large dab maps use the original full path.
            dirty.extend(self.active_coords.iter().copied());
        }
        if force || size_changed {
            dirty.extend(self.tiles.layers()[0].tiles.allocated_coords());
        }
        let changes = if let (Some(stroke), Some(dabs), Some(next)) =
            (active, sampled.as_deref(), next.as_ref())
        {
            self.active_tiles_rasterized = next
                .dabs
                .keys()
                .filter(|coord| dirty.contains(coord))
                .count();
            if dirty.is_empty() {
                None
            } else if stroke.eraser {
                self.tiles.erase_dabs_clipped_in_tiles(
                    "paint",
                    dabs,
                    stroke.selection.as_ref(),
                    Some(&dirty),
                )?
            } else {
                self.tiles.paint_dabs_clipped_in_tiles(
                    "paint",
                    dabs,
                    stroke.brush.color,
                    stroke.selection.as_ref(),
                    Some(&dirty),
                )?
            }
        } else if let (Some(stroke), Some(dabs)) = (active, sampled.as_deref()) {
            if stroke.eraser {
                self.tiles
                    .erase_dabs_clipped("paint", dabs, stroke.selection.as_ref())?
            } else {
                self.tiles.paint_dabs_clipped(
                    "paint",
                    dabs,
                    stroke.brush.color,
                    stroke.selection.as_ref(),
                )?
            }
        } else {
            active
                .map(|stroke| paint_stroke_into_tiles(&mut self.tiles, "paint", stroke))
                .transpose()?
                .flatten()
        };
        let active_coords = next
            .as_ref()
            .map(|next| next.dabs.keys().copied().collect())
            .unwrap_or_else(|| {
                changes
                    .as_ref()
                    .map_or_else(Vec::new, |changes| changes.coords.clone())
            });
        if next.is_none() {
            dirty.extend(active_coords.iter().copied());
        }
        let paper = crate::root_paint::masked_paper(document);
        if self.paper != paper || (paper && (!unchanged || force)) {
            dirty.extend(self.uploaded.keys().copied());
            if paper {
                let (w, h) = document.dimensions();
                for y in 0..h.div_ceil(TILE_SIZE) {
                    for x in 0..w.div_ceil(TILE_SIZE) {
                        dirty.insert(TileCoord { x, y });
                    }
                }
            }
        }
        self.paper = paper;
        let uploads = if paper {
            self.paper_uploads(document, &dirty)
        } else {
            self.tiles.prepare_uploads(&TileInvalidation {
                layer_id: "paint".into(),
                coords: dirty.into_iter().collect(),
            })
        };
        // Roll back only the transient stroke, retaining the committed tile allocation.
        if changes.is_some() {
            self.tiles.undo()?;
        }
        self.tiles.discard_history();
        let mut uploads = uploads?;
        self.committed_key = Some(key);
        self.active = next;
        self.active_coords = active_coords;
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
    #[ignore = "manual Release timing; compares targeted updates with forced whole-stroke redraw"]
    fn active_tile_update_timing() {
        let mut state = Document::default().document_state();
        state.width = 4096;
        state.height = 512;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.begin(
            Point { x: 20., y: 200. },
            Brush {
                size: 20.,
                hardness: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
        let mut incremental = PaintCache::new(doc.dimensions()).unwrap();
        let mut full = PaintCache::new(doc.dimensions()).unwrap();
        let mut partial_times = Vec::new();
        let mut full_times = Vec::new();
        let mut partial_tiles = 0;
        let mut full_tiles = 0;
        for i in 0..64 {
            doc.extend_with_pressure(
                Point {
                    x: 20. + i as f32 * 60.,
                    y: 200. + (i as f32 * 0.2).sin() * 20.,
                },
                0.6,
            )
            .unwrap();
            let now = std::time::Instant::now();
            incremental.prepare(&doc, false).unwrap();
            partial_times.push(now.elapsed().as_secs_f64() * 1000.);
            partial_tiles += incremental.active_tiles_rasterized;
            let now = std::time::Instant::now();
            full.prepare(&doc, true).unwrap();
            full_times.push(now.elapsed().as_secs_f64() * 1000.);
            full_tiles += full.active_tiles_rasterized;
            assert_eq!(incremental.uploaded, full.uploaded);
        }
        partial_times.sort_by(f64::total_cmp);
        full_times.sort_by(f64::total_cmp);
        eprintln!("4096x512, 64 updates, full median/p95 {:.3}/{:.3} ms, partial {:.3}/{:.3} ms, tile rasterizations {full_tiles}->{partial_tiles}", full_times[32], full_times[60], partial_times[32], partial_times[60]);
    }

    #[test]
    fn active_tile_deltas_match_full_replay_and_skip_stable_tiles() {
        use lumapaint_core::document::BrushSimulation;
        for simulation in [
            BrushSimulation::default(),
            BrushSimulation::Ink,
            BrushSimulation::Pencil,
            BrushSimulation::DryBrush,
        ] {
            let mut state = Document::default().document_state();
            state.width = 2048;
            state.height = 256;
            let mut doc = Document::from_document_state(state).unwrap();
            let mut cache = PaintCache::new(doc.dimensions()).unwrap();
            doc.begin(
                Point { x: 20., y: 80. },
                Brush {
                    size: 12.,
                    hardness: 0.4,
                    simulation,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut skipped = false;
            for i in 0..16 {
                doc.extend_with_pressure(
                    Point {
                        x: 20. + i as f32 * 120.,
                        y: 80. + (i as f32 * 0.5).sin() * 20.,
                    },
                    0.3 + (i % 4) as f32 * 0.2,
                )
                .unwrap();
                cache.prepare(&doc, false).unwrap();
                skipped |= cache.active_tiles_rasterized < cache.active_coords.len();
                let mut full = PaintCache::new(doc.dimensions()).unwrap();
                full.prepare(&doc, true).unwrap();
                assert_eq!(
                    cache.uploaded, full.uploaded,
                    "simulation={simulation:?}, update={i}"
                );
            }
            assert!(skipped);
            assert!(cache.prepare(&doc, false).unwrap().is_empty());
            assert_eq!(cache.active_tiles_rasterized, 0);
            doc.finish();
            cache.prepare(&doc, false).unwrap();
            doc.undo();
            cache.prepare(&doc, false).unwrap();
            assert!(cache.uploaded.is_empty());
            doc.redo();
            cache.prepare(&doc, false).unwrap();
            doc.begin_eraser(
                Point { x: 40., y: 80. },
                Brush {
                    size: 30.,
                    hardness: 0.3,
                    ..Default::default()
                },
                0.7,
            )
            .unwrap();
            for i in 0..6 {
                doc.extend_with_pressure(
                    Point {
                        x: 40. + i as f32 * 200.,
                        y: 80.,
                    },
                    0.7,
                )
                .unwrap();
                cache.prepare(&doc, false).unwrap();
                let mut full = PaintCache::new(doc.dimensions()).unwrap();
                full.prepare(&doc, true).unwrap();
                assert_eq!(cache.uploaded, full.uploaded, "eraser update {i}");
            }
        }
    }
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
            no_color: false,
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
        assert_eq!(cache.compared_strokes, 0);
        document
            .begin(Point { x: 50., y: 60. }, Brush::default())
            .unwrap();
        cache.prepare(&document, false).unwrap();
        assert_eq!(cache.compared_strokes, 0);
        document.extend(Point { x: 60., y: 65. }).unwrap();
        cache.prepare(&document, false).unwrap();
        assert_eq!(cache.compared_strokes, 0);
        document.finish();
        cache.prepare(&document, false).unwrap();
        assert!(cache.compared_strokes > 0);
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
