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
    sampled: Option<(Stroke, Vec<RasterDab>)>,
    pub active_tiles_rasterized: usize,
    uploaded: BTreeMap<TileCoord, Vec<u8>>,
    pub replayed: usize,
    pub paper_tiles_rasterized: usize,
    paper: bool,
    paper_settings: Option<lumapaint_core::natural_media::PaperSettings>,
}

// At most 8 MiB of dab payload retained per paint cache. Large tips / long strokes
// bypass this optimization; sampling still uses the established whole-path algorithm.
const MAX_ACTIVE_DAB_REFS: usize = 262_144;

#[derive(PartialEq)]
struct ActiveStyle {
    eraser: bool,
    brush: lumapaint_core::document::Brush,
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
                brush: stroke.brush,
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

// The interval follows the same inverse transforms as coverage(). Expand after
// each affine evaluation to include f32 multiply/add rounding, even for shear.
fn affine_interval(bounds: [f64; 4], matrix: [f64; 6]) -> [f64; 4] {
    let [a, b, c, d, e, f] = matrix;
    let mut result = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
        [bounds[0], bounds[3]],
    ] {
        let point = [a * x + c * y + e, b * x + d * y + f];
        for axis in 0..2 {
            result[axis] = result[axis].min(point[axis]);
            result[axis + 2] = result[axis + 2].max(point[axis]);
        }
    }
    result
}
fn coverage_error(bounds: [f64; 4], matrix: [f32; 6]) -> [f64; 2] {
    let [a, b, c, d, e, f] = matrix.map(f64::from);
    let x = bounds[0].abs().max(bounds[2].abs());
    let y = bounds[1].abs().max(bounds[3].abs());
    [
        a.abs() * x + c.abs() * y + e.abs() + 1.,
        b.abs() * x + d.abs() * y + f.abs() + 1.,
    ]
    .map(|magnitude| magnitude * 8. * f64::from(f32::EPSILON))
}
fn expand_interval(mut bounds: [f64; 4], error: [f64; 2]) -> [f64; 4] {
    for axis in 0..2 {
        bounds[axis] -= error[axis];
        bounds[axis + 2] += error[axis];
    }
    bounds
}

fn pixel_support_bounds(bounds: [f64; 4]) -> [f64; 4] {
    // Pixel masks snap near-integer coordinates before flooring. Include that
    // footprint separately from the affine arithmetic error above.
    let x = bounds[0].abs().max(bounds[2].abs());
    let y = bounds[1].abs().max(bounds[3].abs());
    expand_interval(
        bounds,
        [(x * 0.0000004).max(0.00002), (y * 0.0000004).max(0.00002)],
    )
}

struct MaskChangeRegion {
    to_local: [f32; 6],
    to_geometry: Option<[f32; 6]>,
    bounds: [f64; 4],
}
impl MaskChangeRegion {
    fn intersects(&self, tile: [f64; 4]) -> bool {
        let local_error = coverage_error(tile, self.to_local);
        let local = expand_interval(
            affine_interval(tile, self.to_local.map(f64::from)),
            local_error,
        );
        let area = if let Some(matrix) = self.to_geometry {
            // Compose exact affine coefficients before bounding: rotating an AABB
            // twice would lose x/y correlation and inflate even an identity path.
            let [a, b, c, d, e, f] = self.to_local.map(f64::from);
            let [x, y, z, w, u, v] = matrix.map(f64::from);
            let combined = [
                x * a + z * b,
                y * a + w * b,
                x * c + z * d,
                y * c + w * d,
                x * e + z * f + u,
                y * e + w * f + v,
            ];
            let roundoff = coverage_error(local, matrix);
            let error = [
                x.abs() * local_error[0] + z.abs() * local_error[1] + roundoff[0],
                y.abs() * local_error[0] + w.abs() * local_error[1] + roundoff[1],
            ];
            expand_interval(affine_interval(tile, combined), error)
        } else {
            local
        };
        // Uncertain arithmetic keeps the candidate instead of discarding pixels.
        if area.iter().any(|v| !v.is_finite()) {
            return true;
        }
        area[0] <= self.bounds[2]
            && area[2] >= self.bounds[0]
            && area[1] <= self.bounds[3]
            && area[3] >= self.bounds[1]
    }
}

fn selection_change_region(
    selection: &lumapaint_core::selection::Selection,
    to_local: [f32; 6],
    to_geometry: Option<[f32; 6]>,
) -> Option<Option<MaskChangeRegion>> {
    // Raster selections have another sampling transform. Keep the existing scan
    // until their interpolation footprint has an independently verified bound.
    if selection.raster.is_some() {
        return None;
    }
    let Some(bounds) = selection.coverage_bounds() else {
        return Some(None);
    };
    let mut bounds = bounds.map(f64::from);
    if bounds.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let magnitude = bounds.iter().map(|v| v.abs()).fold(1., f64::max);
    let pad = 2. + magnitude * 16. * f64::from(f32::EPSILON);
    bounds[0] -= pad;
    bounds[1] -= pad;
    bounds[2] += pad;
    bounds[3] += pad;
    Some(Some(MaskChangeRegion {
        to_local,
        to_geometry,
        bounds,
    }))
}

fn mask_pixel_boundaries(
    ar: &[[u32; 2]],
    br: &[[u32; 2]],
    ag: &[[u32; 3]],
    bg: &[[u32; 3]],
    total: u32,
) -> Option<Vec<u32>> {
    let count = ar
        .len()
        .saturating_add(br.len())
        .saturating_add(ag.len())
        .saturating_add(bg.len())
        .saturating_mul(2)
        .saturating_add(2);
    if count > 131_072 {
        return None;
    }
    let mut boundaries = Vec::with_capacity(count);
    boundaries.extend([0, total]);
    for run in ar.iter().chain(br.iter()) {
        boundaries.extend([run[0], run[0] + run[1]]);
    }
    for run in ag.iter().chain(bg.iter()) {
        boundaries.extend([run[0], run[0] + run[1]]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    Some(boundaries)
}

fn mask_change_regions(
    previous: &lumapaint_core::layer_mask::LayerMask,
    next: &lumapaint_core::layer_mask::LayerMask,
) -> Option<Vec<MaskChangeRegion>> {
    use lumapaint_core::layer_mask::MaskContent;
    if previous.enabled != next.enabled
        || previous.inverted != next.inverted
        || previous.density != next.density
    {
        return None;
    }
    let old_inverse = lumapaint_core::image_frame::inverse(previous.transform).ok()?;
    let new_inverse = lumapaint_core::image_frame::inverse(next.transform).ok()?;
    let mut regions = Vec::new();
    match (&previous.content, &next.content) {
        (
            Some(MaskContent::Vector {
                selection: old_selection,
                edits: old_edits,
            }),
            Some(MaskContent::Vector {
                selection: new_selection,
                edits: new_edits,
            }),
        ) if previous.transform == next.transform && old_selection == new_selection => {
            let prefix = old_edits
                .iter()
                .zip(new_edits)
                .take_while(|(a, b)| a == b)
                .count();
            if old_edits
                .len()
                .saturating_sub(prefix)
                .saturating_add(new_edits.len().saturating_sub(prefix))
                > 512
            {
                return None;
            }
            for edit in old_edits[prefix..].iter().chain(&new_edits[prefix..]) {
                if let Some(region) =
                    selection_change_region(&edit.selection, old_inverse, Some(edit.to_geometry))?
                {
                    regions.push(region);
                }
            }
            return Some(regions);
        }
        (
            Some(MaskContent::Pixel {
                width: aw,
                height: ah,
                runs: ar,
                gray: ag,
            }),
            Some(MaskContent::Pixel {
                width: bw,
                height: bh,
                runs: br,
                gray: bg,
            }),
        ) if previous.transform == next.transform && aw == bw && ah == bh => {
            let boundaries = mask_pixel_boundaries(ar, br, ag, bg, aw.checked_mul(*ah)?)?;
            crate::performance::count("mask_diff_local_mask_clones_avoided", 2);
            crate::performance::count(
                "mask_diff_local_payload_bytes_avoided",
                ((ar.len() + br.len()) * std::mem::size_of::<[u32; 2]>()
                    + (ag.len() + bg.len()) * std::mem::size_of::<[u32; 3]>())
                    as u64,
            );
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for pair in boundaries.windows(2) {
                let (start, end) = (pair[0], pair[1]);
                if start >= end {
                    continue;
                }
                let point = [(start % aw) as f32 + 0.5, (start / aw) as f32 + 0.5];
                if previous.coverage_in_mask_space(point) == next.coverage_in_mask_space(point) {
                    continue;
                }
                let top = start / aw;
                let bottom = (end - 1) / aw;
                let (left, right) = if top == bottom {
                    (start % aw, (end - 1) % aw + 1)
                } else {
                    (0, *aw)
                };
                bounds[0] = bounds[0].min(f64::from(left));
                bounds[1] = bounds[1].min(f64::from(top));
                bounds[2] = bounds[2].max(f64::from(right));
                bounds[3] = bounds[3].max(f64::from(bottom + 1));
            }
            if bounds[0].is_finite() {
                regions.push(MaskChangeRegion {
                    to_local: old_inverse,
                    to_geometry: None,
                    bounds: pixel_support_bounds(bounds),
                });
            }
            return Some(regions);
        }
        _ => {}
    }
    // A transform or base-shape replacement can affect old and new support.
    // Flags above ensure coverage outside both supports remains the same.
    for (mask, to_local) in [(previous, old_inverse), (next, new_inverse)] {
        match mask.content.as_ref()? {
            MaskContent::Pixel { width, height, .. } => regions.push(MaskChangeRegion {
                to_local,
                to_geometry: None,
                bounds: pixel_support_bounds([0., 0., f64::from(*width), f64::from(*height)]),
            }),
            MaskContent::Vector { selection, edits } => {
                if edits.len() > 512 {
                    return None;
                }
                if let Some(region) = selection_change_region(selection, to_local, None)? {
                    regions.push(region);
                }
                for edit in edits.iter().filter(|edit| edit.reveal) {
                    if let Some(region) =
                        selection_change_region(&edit.selection, to_local, Some(edit.to_geometry))?
                    {
                        regions.push(region);
                    }
                }
            }
        }
    }
    Some(regions)
}

// Compare final coverage at the exact document pixel centers used by paper_uploads.
// Large canvases retain the full invalidation path to bound this CPU scan.
fn changed_mask_tiles(
    previous: &lumapaint_core::layer_mask::LayerMask,
    next: &lumapaint_core::layer_mask::LayerMask,
    size: (u32, u32),
) -> Option<BTreeSet<TileCoord>> {
    const MAX_COVERAGE_SAMPLES: usize = 262_144;
    let (w, h) = size;
    if u64::from(w) * u64::from(h) > 16_777_216 {
        return None;
    }
    let mut changed = BTreeSet::new();
    use lumapaint_core::layer_mask::MaskContent;
    if previous.transform == [1., 0., 0., 1., 0., 0.] && next.transform == previous.transform {
        if let (
            Some(MaskContent::Pixel {
                width: aw,
                height: ah,
                runs: ar,
                gray: ag,
            }),
            Some(MaskContent::Pixel {
                width: bw,
                height: bh,
                runs: br,
                gray: bg,
            }),
        ) = (&previous.content, &next.content)
        {
            if (*aw, *ah) == size && (*bw, *bh) == size {
                let boundaries = mask_pixel_boundaries(ar, br, ag, bg, w * h)?;
                for pair in boundaries.windows(2) {
                    let mut start = pair[0];
                    let end = pair[1].min(w * h);
                    if start >= end {
                        continue;
                    }
                    let p = [(start % w) as f32 + 0.5, (start / w) as f32 + 0.5];
                    if previous.coverage(p) == next.coverage(p) {
                        continue;
                    }
                    while start < end {
                        let y = start / w;
                        let row_end = end.min((y + 1) * w);
                        for x in (start % w) / TILE_SIZE..=((row_end - 1) % w) / TILE_SIZE {
                            changed.insert(TileCoord {
                                x,
                                y: y / TILE_SIZE,
                            });
                        }
                        start = row_end;
                    }
                }
                return Some(changed);
            }
        }
    }
    let regions = mask_change_regions(previous, next);
    let mut samples_left = MAX_COVERAGE_SAMPLES;
    for ty in 0..h.div_ceil(TILE_SIZE) {
        for tx in 0..w.div_ceil(TILE_SIZE) {
            let origin = [tx * TILE_SIZE, ty * TILE_SIZE];
            let tile_bounds = [
                f64::from(origin[0] as f32 + 0.5),
                f64::from(origin[1] as f32 + 0.5),
                f64::from(((origin[0] + TILE_SIZE).min(w) - 1) as f32 + 0.5),
                f64::from(((origin[1] + TILE_SIZE).min(h) - 1) as f32 + 0.5),
            ];
            if regions
                .as_ref()
                .is_some_and(|regions| !regions.iter().any(|region| region.intersects(tile_bounds)))
            {
                continue;
            }
            let mut exhausted = false;
            let differs = (origin[1]..(origin[1] + TILE_SIZE).min(h)).any(|y| {
                let row_bounds = [
                    tile_bounds[0],
                    f64::from(y as f32 + 0.5),
                    tile_bounds[2],
                    f64::from(y as f32 + 0.5),
                ];
                if regions.as_ref().is_some_and(|regions| {
                    !regions.iter().any(|region| region.intersects(row_bounds))
                }) {
                    return false;
                }
                (origin[0]..(origin[0] + TILE_SIZE).min(w)).any(|x| {
                    if samples_left == 0 {
                        exhausted = true;
                        return true;
                    }
                    samples_left -= 1;
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    previous.coverage(p) != next.coverage(p)
                })
            });
            if exhausted {
                return None;
            }
            if differs {
                changed.insert(TileCoord { x: tx, y: ty });
            }
        }
    }
    Some(changed)
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
            sampled: None,
            active_tiles_rasterized: 0,
            uploaded: BTreeMap::new(),
            replayed: 0,
            paper_tiles_rasterized: 0,
            paper: false,
            paper_settings: None,
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
                let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
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
                            document.paper(),
                            [1., 1.],
                        )
                        .map(|v| (v as f32 * opacity).round() as u8);
                        let at = ((y * TILE_SIZE + x) * 4) as usize;
                        pixels[at..at + 4].copy_from_slice(&pixel);
                    }
                }
                TileUpload {
                    coord: *coord,
                    origin,
                    extent,
                    bytes_per_row: TILE_SIZE * 4,
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
        self.paper_tiles_rasterized = 0;
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
        let mut appearance_changed = false;
        if let Some(changes) = self.tiles.set_layer_appearance(
            "paint",
            document.background_visible(),
            document.paint_layer_opacity(),
        )? {
            appearance_changed = true;
            dirty.extend(changes.coords);
            dirty.extend(self.active_coords.iter().copied());
        }
        let mut effects = document.layer_effects("layer-1");
        let mask_tiles = if self.paper
            && crate::root_paint::masked_paper(document)
            && self.tiles.layers()[0].effects != effects
        {
            let mut previous = self.tiles.layers()[0].effects.clone();
            let old_mask = previous.mask.take();
            let new_mask = effects.mask.take();
            let changes = if previous == effects && old_mask != new_mask {
                old_mask
                    .as_ref()
                    .zip(new_mask.as_ref())
                    .and_then(|(old, new)| changed_mask_tiles(old, new, document.dimensions()))
            } else {
                None
            };
            effects.mask = new_mask;
            changes
        } else {
            None
        };
        if let Some(changes) = self.tiles.set_layer_effects("paint", effects)? {
            if let Some(coords) = mask_tiles {
                dirty.extend(coords);
            } else {
                appearance_changed = true;
                dirty.extend(changes.coords);
            }
            dirty.extend(self.active_coords.iter().copied());
        }
        self.tiles.discard_history();
        let active = document.active_paint_stroke();
        // Keep the committed tiles untouched between calls. Only changed active tiles
        // are composited temporarily; unchanged active pixels already live in the display.
        let sample_stroke = active.filter(|stroke| !stroke.clear);
        let sample_hit = sample_stroke
            .zip(self.sampled.as_ref())
            .is_some_and(|(stroke, (old, _))| stroke == old);
        if unchanged && sample_hit && !force {
            // Cursor/overlay redraws keep the already uploaded active tiles.
            crate::performance::count("paint_active_unchanged_frames", 1);
            return Ok(Vec::new());
        }
        let new_samples = if sample_hit {
            crate::performance::count("paint_active_sample_reuses", 1);
            None
        } else {
            sample_stroke
                .map(|stroke| {
                    crate::performance::count("paint_active_sample_builds", 1);
                    sampled_raster_dabs(stroke)
                })
                .transpose()?
        };
        let sampled = if sample_hit {
            self.sampled.as_ref().map(|(_, dabs)| dabs.as_slice())
        } else {
            new_samples.as_deref()
        };
        let next = active
            .zip(sampled)
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
        let changes =
            if let (Some(stroke), Some(dabs), Some(next)) = (active, sampled, next.as_ref()) {
                self.active_tiles_rasterized = next
                    .dabs
                    .keys()
                    .filter(|coord| dirty.contains(coord))
                    .count();
                if dirty.is_empty() {
                    None
                } else {
                    self.tiles.paint_brush_dabs_clipped_in_tiles(
                        "paint",
                        dabs,
                        compositing_brush(stroke),
                        stroke.selection.as_ref(),
                        Some(&dirty),
                    )?
                }
            } else if let (Some(stroke), Some(dabs)) = (active, sampled) {
                self.tiles.paint_brush_dabs_clipped_in_tiles(
                    "paint",
                    dabs,
                    compositing_brush(stroke),
                    stroke.selection.as_ref(),
                    None,
                )?
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
        if self.paper != paper
            || (paper
                && (appearance_changed
                    || self.paper_settings != Some(document.paper())
                    || !prefix
                    || force))
        {
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
        self.paper_settings = Some(document.paper());
        let uploads = if paper {
            self.paper_tiles_rasterized = dirty.len();
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
        if !sample_hit {
            // Bounded, exact input snapshots: length/pointer/last-point keys are not
            // sufficient when a cancelled stroke is replaced by another stroke.
            self.sampled = sample_stroke.zip(new_samples).and_then(|(stroke, dabs)| {
                let bytes = dabs
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RasterDab>())
                    .saturating_add(
                        stroke
                            .points
                            .len()
                            .saturating_mul(std::mem::size_of::<Point>()),
                    )
                    .saturating_add(
                        stroke
                            .pressures
                            .len()
                            .saturating_mul(std::mem::size_of::<f32>()),
                    );
                (stroke.selection.is_none() && bytes <= 8 * 1024 * 1024)
                    .then(|| (stroke.clone(), dabs))
            });
        }
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

    fn all_changed_mask_tiles(
        old: &lumapaint_core::layer_mask::LayerMask,
        next: &lumapaint_core::layer_mask::LayerMask,
        size: (u32, u32),
    ) -> BTreeSet<TileCoord> {
        let mut tiles = BTreeSet::new();
        for y in 0..size.1 {
            for x in 0..size.0 {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                if old.coverage(p) != next.coverage(p) {
                    tiles.insert(TileCoord {
                        x: x / TILE_SIZE,
                        y: y / TILE_SIZE,
                    });
                }
            }
        }
        tiles
    }

    #[test]
    fn snapped_pixel_boundary_is_retained_after_large_scale() {
        use lumapaint_core::layer_mask::{LayerMask, MaskContent, MaskKind};
        let size = (513, 259);
        let mut old = LayerMask::from_selection(MaskKind::Pixel, None, size.0, size.1).unwrap();
        old.transform = [10000., 0., 0., 1., 255.59, 0.];
        let mut next = old.clone();
        if let Some(MaskContent::Pixel { runs, .. }) = next.content.as_mut() {
            runs[0] = [1, 512];
        }
        let actual = changed_mask_tiles(&old, &next, size).unwrap();
        assert!(!actual.is_empty());
        assert_eq!(actual, all_changed_mask_tiles(&old, &next, size));
    }

    #[test]
    fn transformed_mask_edit_bounds_match_full_pixels_and_undo_redo() {
        use lumapaint_core::{
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        for kind in [MaskKind::Pixel, MaskKind::Vector] {
            for matrix in [
                [1., 0., 0., 1., 37., -19.],
                [0.94, 0.2, -0.15, 1.05, 60., -20.],
                [-1., 0.1, 0.2, 1., 750., -5.],
            ] {
                let mut doc = Document::default();
                let size = doc.dimensions();
                let mut old = LayerMask::from_selection(kind, None, size.0, size.1).unwrap();
                old.transform = matrix;
                old.density = 0.65;
                old.inverted = true;
                let mut next = old.clone();
                next.paint_selection(
                    &Selection::new(SelectionShape::Ellipse, [310., 270., 30., 25.]),
                    Some(&Selection::new(
                        SelectionShape::Rectangle,
                        [300., 260., 35., 35.],
                    )),
                    true,
                )
                .unwrap();
                let actual = changed_mask_tiles(&old, &next, size)
                    .expect("local edit stays within comparison budget");
                assert!(!actual.is_empty());
                assert_eq!(actual, all_changed_mask_tiles(&old, &next, size));
                assert_eq!(changed_mask_tiles(&next, &old, size).unwrap(), actual);
                let mut effects = doc.layer_effects("layer-1");
                effects.mask = Some(old);
                doc.set_layer_effects("layer-1", effects).unwrap();
                let mut cache = PaintCache::new(size).unwrap();
                cache.prepare(&doc, false).unwrap();
                let mut effects = doc.layer_effects("layer-1");
                effects.mask = Some(next);
                doc.set_layer_effects("layer-1", effects).unwrap();
                for operation in 0..3 {
                    if operation == 1 {
                        doc.undo();
                    }
                    if operation == 2 {
                        doc.redo();
                    }
                    cache.prepare(&doc, false).unwrap();
                    let mut fresh = PaintCache::new(size).unwrap();
                    fresh.prepare(&doc, true).unwrap();
                    assert_eq!(cache.uploaded, fresh.uploaded);
                    assert!(
                        cache.paper_tiles_rasterized
                            < (size.0.div_ceil(TILE_SIZE) * size.1.div_ceil(TILE_SIZE)) as usize
                    );
                }
            }
        }
    }

    #[test]
    fn moved_vector_support_and_replaced_shapes_preserve_all_changed_tiles() {
        use lumapaint_core::{
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        let size = (800, 600);
        let selection = Selection::new(SelectionShape::Ellipse, [60., 70., 40., 30.]);
        let old =
            LayerMask::from_selection(MaskKind::Vector, Some(&selection), size.0, size.1).unwrap();
        let mut next = old.clone();
        next.transform = [0.9, 0.2, -0.1, 1., 290., 220.];
        assert_eq!(
            changed_mask_tiles(&old, &next, size).unwrap(),
            all_changed_mask_tiles(&old, &next, size)
        );
        let replaced = LayerMask::from_selection(
            MaskKind::Vector,
            Some(&Selection::new(
                SelectionShape::Rectangle,
                [450., 350., 20., 20.],
            )),
            size.0,
            size.1,
        )
        .unwrap();
        assert_eq!(
            changed_mask_tiles(&next, &replaced, size).unwrap(),
            all_changed_mask_tiles(&next, &replaced, size)
        );
    }

    #[test]
    fn complex_mask_diff_falls_back_to_exact_full_rebuild() {
        use lumapaint_core::layer_mask::{LayerMask, MaskContent, MaskKind};
        let mut doc = Document::default();
        let (w, h) = doc.dimensions();
        let old = LayerMask::from_selection(MaskKind::Pixel, None, w, h).unwrap();
        let mut effects = doc.layer_effects("layer-1");
        effects.mask = Some(old.clone());
        doc.set_layer_effects("layer-1", effects).unwrap();
        let mut cache = PaintCache::new((w, h)).unwrap();
        cache.prepare(&doc, false).unwrap();
        let full_count = cache.paper_tiles_rasterized;
        let mut next = old.clone();
        if let Some(MaskContent::Pixel { runs, .. }) = next.content.as_mut() {
            *runs = (0..65_536).map(|index| [index * 2, 1]).collect();
        }
        assert!(changed_mask_tiles(&old, &next, (w, h)).is_none());
        let mut effects = doc.layer_effects("layer-1");
        effects.mask = Some(next);
        doc.set_layer_effects("layer-1", effects).unwrap();
        for operation in [0, 1, 2] {
            if operation == 1 {
                doc.undo();
            }
            if operation == 2 {
                doc.redo();
            }
            cache.prepare(&doc, false).unwrap();
            assert_eq!(cache.paper_tiles_rasterized, full_count);
            let mut fresh = PaintCache::new((w, h)).unwrap();
            fresh.prepare(&doc, true).unwrap();
            assert_eq!(cache.uploaded, fresh.uploaded);
        }
        let old = LayerMask::from_selection(MaskKind::Vector, None, 1024, 1024).unwrap();
        let mut next = old.clone();
        next.linked = !old.linked;
        assert!(changed_mask_tiles(&old, &next, (1024, 1024))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn mask_interval_diff_matches_coverage_across_rows_and_tiles() {
        use lumapaint_core::layer_mask::{LayerMask, MaskContent, MaskKind};
        let size = (513, 259);
        let old = LayerMask::from_selection(MaskKind::Pixel, None, size.0, size.1).unwrap();
        let mut next = old.clone();
        if let Some(MaskContent::Pixel { gray, .. }) = next.content.as_mut() {
            *gray = vec![[255, 4, 128], [1024, 513, 30]];
        }
        for inverted in [false, true] {
            next.inverted = inverted;
            for density in [0., 0.5, 1.] {
                next.density = density;
                for matrix in [[1., 0., 0., 1., 0., 0.], [0., -1., 1., 0., 0., 513.]] {
                    next.transform = matrix;
                    let actual = changed_mask_tiles(&old, &next, size).unwrap();
                    let mut expected = BTreeSet::new();
                    for y in 0..size.1 {
                        for x in 0..size.0 {
                            let point = [x as f32 + 0.5, y as f32 + 0.5];
                            if old.coverage(point) != next.coverage(point) {
                                expected.insert(TileCoord {
                                    x: x / TILE_SIZE,
                                    y: y / TILE_SIZE,
                                });
                            }
                        }
                    }
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn masked_paper_local_paint_matches_full_rebuild() {
        let mut doc = Document::default();
        let (w, h) = doc.dimensions();
        doc.set_layer_effects(
            "layer-1",
            lumapaint_core::layer_effects::LayerEffects {
                mask: Some(
                    lumapaint_core::layer_mask::LayerMask::from_selection(
                        lumapaint_core::layer_mask::MaskKind::Pixel,
                        None,
                        w,
                        h,
                    )
                    .unwrap(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
        let mut cache = PaintCache::new((w, h)).unwrap();
        cache.prepare(&doc, false).unwrap();
        let full_count = cache.paper_tiles_rasterized;
        stroke(&mut doc, 40., false);
        cache.prepare(&doc, false).unwrap();
        assert!(cache.paper_tiles_rasterized > 0);
        assert!(cache.paper_tiles_rasterized < full_count);
        let mut fresh = PaintCache::new((w, h)).unwrap();
        fresh.prepare(&doc, true).unwrap();
        assert_eq!(cache.uploaded, fresh.uploaded);
        cache.prepare(&doc, false).unwrap();
        assert_eq!(cache.paper_tiles_rasterized, 0);
        for redo in [false, true] {
            if redo {
                doc.redo();
            } else {
                doc.undo();
            }
            cache.prepare(&doc, false).unwrap();
            let mut fresh = PaintCache::new((w, h)).unwrap();
            fresh.prepare(&doc, true).unwrap();
            assert_eq!(cache.uploaded, fresh.uploaded);
        }
        let mut effects = doc.layer_effects("layer-1");
        effects
            .mask
            .as_mut()
            .unwrap()
            .paint_selection(
                &lumapaint_core::selection::Selection::new(
                    lumapaint_core::selection::SelectionShape::Rectangle,
                    [10., 10., 30., 30.],
                ),
                None,
                false,
            )
            .unwrap();
        doc.set_layer_effects("layer-1", effects).unwrap();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(cache.paper_tiles_rasterized, 1);
        let mut fresh = PaintCache::new((w, h)).unwrap();
        fresh.prepare(&doc, true).unwrap();
        assert_eq!(cache.uploaded, fresh.uploaded);
        for redo in [false, true] {
            if redo {
                doc.redo();
            } else {
                doc.undo();
            }
            cache.prepare(&doc, false).unwrap();
            assert_eq!(cache.paper_tiles_rasterized, 1);
            let mut fresh = PaintCache::new((w, h)).unwrap();
            fresh.prepare(&doc, true).unwrap();
            assert_eq!(cache.uploaded, fresh.uploaded);
        }
        let mut effects = doc.layer_effects("layer-1");
        effects.mask.as_mut().unwrap().inverted = true;
        doc.set_layer_effects("layer-1", effects).unwrap();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(cache.paper_tiles_rasterized, full_count);
        let mut fresh = PaintCache::new((w, h)).unwrap();
        fresh.prepare(&doc, true).unwrap();
        assert_eq!(cache.uploaded, fresh.uploaded);
    }

    #[test]
    fn masked_paper_edge_tiles_match_upload_contract() {
        let mut document = Document::default();
        document
            .set_layer_effects(
                "layer-1",
                lumapaint_core::layer_effects::LayerEffects {
                    mask: Some(
                        lumapaint_core::layer_mask::LayerMask::from_selection(
                            lumapaint_core::layer_mask::MaskKind::Pixel,
                            None,
                            document.dimensions().0,
                            document.dimensions().1,
                        )
                        .unwrap(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();
        let (w, h) = document.dimensions();
        let coord = TileCoord {
            x: (w - 1) / TILE_SIZE,
            y: (h - 1) / TILE_SIZE,
        };
        let cache = PaintCache::new((w, h)).unwrap();
        let uploads = cache
            .paper_uploads(&document, &BTreeSet::from([coord]))
            .unwrap();
        validate_tile_uploads(w, h, &uploads).unwrap();
        let upload = &uploads[0];
        let mut effects = document.layer_effects("layer-1");
        let mask = effects.mask.take();
        let effects = effects.prepare();
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let offset = ((y * TILE_SIZE + x) * 4) as usize;
                let expected = if x < upload.extent[0] && y < upload.extent[1] {
                    crate::root_paint::composite(
                        [0; 4],
                        [
                            (upload.origin[0] + x) as f32 + 0.5,
                            (upload.origin[1] + y) as f32 + 0.5,
                        ],
                        &effects,
                        mask.as_ref(),
                        true,
                        document.paper(),
                        [1., 1.],
                    )
                } else {
                    [0; 4]
                };
                assert_eq!(&upload.pixels[offset..offset + 4], &expected);
            }
        }
    }

    #[test]
    fn unchanged_active_stroke_reuses_samples_but_edits_force_and_history_stay_exact() {
        let mut doc = Document::default();
        doc.begin_with_pressure(Point { x: 30., y: 40. }, Brush::default(), 0.3)
            .unwrap();
        doc.extend_with_pressure(Point { x: 180., y: 110. }, 0.8)
            .unwrap();
        let mut cache = PaintCache::new(doc.dimensions()).unwrap();
        cache.prepare(&doc, false).unwrap();
        let data = cache.sampled.as_ref().unwrap().1.as_ptr();
        for _ in 0..5 {
            assert!(cache.prepare(&doc, false).unwrap().is_empty());
            assert_eq!(cache.active_tiles_rasterized, 0);
            assert_eq!(data, cache.sampled.as_ref().unwrap().1.as_ptr());
        }
        assert!(!cache.prepare(&doc, true).unwrap().is_empty());
        assert_eq!(data, cache.sampled.as_ref().unwrap().1.as_ptr());
        doc.extend_with_pressure(Point { x: 210., y: 130. }, 0.2)
            .unwrap();
        cache.prepare(&doc, false).unwrap();
        assert_eq!(
            &cache.sampled.as_ref().unwrap().0,
            doc.active_paint_stroke().unwrap()
        );
        let mut fresh = PaintCache::new(doc.dimensions()).unwrap();
        fresh.prepare(&doc, true).unwrap();
        assert_eq!(cache.uploaded, fresh.uploaded);
        doc.finish();
        cache.prepare(&doc, false).unwrap();
        assert!(cache.sampled.is_none());
        for redo in [false, true] {
            if redo {
                doc.redo();
            } else {
                doc.undo();
            }
            cache.prepare(&doc, false).unwrap();
            let mut fresh = PaintCache::new(doc.dimensions()).unwrap();
            fresh.prepare(&doc, true).unwrap();
            assert_eq!(cache.uploaded, fresh.uploaded);
        }
    }

    #[test]
    #[ignore = "manual active stroke redraw benchmark"]
    fn unchanged_active_redraw_timing() {
        let mut doc = Document::default();
        doc.begin(Point { x: 20., y: 40. }, Brush::default())
            .unwrap();
        for i in 0..300 {
            doc.extend(Point {
                x: 20. + i as f32 * 2.,
                y: 180. + (i as f32 * 0.2).sin() * 100.,
            })
            .unwrap();
        }
        let mut cache = PaintCache::new(doc.dimensions()).unwrap();
        cache.prepare(&doc, false).unwrap();
        let mut baseline = Vec::new();
        let mut retained = Vec::new();
        for i in 0..21 {
            for reuse in [false, true] {
                if !reuse {
                    cache.sampled = None;
                }
                let start = std::time::Instant::now();
                assert!(cache.prepare(&doc, false).unwrap().is_empty());
                if i > 0 {
                    if reuse { &mut retained } else { &mut baseline }
                        .push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
        }
        baseline.sort_by(f64::total_cmp);
        retained.sort_by(f64::total_cmp);
        eprintln!(
            "active redraw benchmark: cache_off_ms={:.3} retained_ms={:.3}",
            baseline[10], retained[10]
        );
    }

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
            ..Default::default()
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
