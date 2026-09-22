//! Sparse RGBA8 raster tiles. A fully transparent tile has no allocation.
use std::collections::BTreeMap;

pub const TILE_SIZE: u32 = 256;
const TILE_BYTES: usize = TILE_SIZE as usize * TILE_SIZE as usize * 4;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TileCoord {
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileChange {
    pub coord: TileCoord,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SparseTiles {
    width: u32,
    height: u32,
    tiles: BTreeMap<TileCoord, Vec<u8>>,
}

impl SparseTiles {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid tile canvas dimensions".into());
        }
        Ok(Self {
            width,
            height,
            tiles: BTreeMap::new(),
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn allocated_tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tile(&self, coord: TileCoord) -> Option<&[u8]> {
        self.tiles.get(&coord).map(Vec::as_slice)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let coord = TileCoord {
            x: x / TILE_SIZE,
            y: y / TILE_SIZE,
        };
        let Some(tile) = self.tiles.get(&coord) else {
            return Some([0; 4]);
        };
        let index = (((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4) as usize;
        Some(tile[index..index + 4].try_into().unwrap())
    }

    /// Replace a rectangle of straight-alpha RGBA8 pixels; return only changed tiles.
    /// The result can be applied backward for Undo and forward for Redo.
    pub fn write_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<TileChange>, String> {
        let end_x = x.checked_add(width).ok_or("Tile rectangle overflow")?;
        let end_y = y.checked_add(height).ok_or("Tile rectangle overflow")?;
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|count| count.checked_mul(4))
            .ok_or("Tile rectangle overflow")?;
        if width == 0 || height == 0 || end_x > self.width || end_y > self.height {
            return Err("Tile rectangle outside canvas".into());
        }
        if pixels.len() != expected {
            return Err("Incorrect RGBA rectangle length".into());
        }
        let mut changes = Vec::new();
        for tile_y in y / TILE_SIZE..=(end_y - 1) / TILE_SIZE {
            for tile_x in x / TILE_SIZE..=(end_x - 1) / TILE_SIZE {
                let coord = TileCoord {
                    x: tile_x,
                    y: tile_y,
                };
                let before = self.tiles.get(&coord).cloned();
                let mut after = before.clone().unwrap_or_else(|| vec![0; TILE_BYTES]);
                let left = x.max(tile_x * TILE_SIZE);
                let top = y.max(tile_y * TILE_SIZE);
                let right = end_x.min((tile_x + 1) * TILE_SIZE);
                let bottom = end_y.min((tile_y + 1) * TILE_SIZE);
                for row in top..bottom {
                    let source = (((row - y) * width + left - x) * 4) as usize;
                    let target = (((row % TILE_SIZE) * TILE_SIZE + left % TILE_SIZE) * 4) as usize;
                    let length = ((right - left) * 4) as usize;
                    after[target..target + length]
                        .copy_from_slice(&pixels[source..source + length]);
                }
                // RGB in transparent pixels has no visible or stored meaning.
                for pixel in after.as_chunks_mut::<4>().0 {
                    if pixel[3] == 0 {
                        pixel[..3].fill(0);
                    }
                }
                let after = after.iter().any(|byte| *byte != 0).then_some(after);
                if before == after {
                    continue;
                }
                match &after {
                    Some(bytes) => {
                        self.tiles.insert(coord, bytes.clone());
                    }
                    None => {
                        self.tiles.remove(&coord);
                    }
                }
                changes.push(TileChange {
                    coord,
                    before,
                    after,
                });
            }
        }
        Ok(changes)
    }

    pub fn apply_changes(&mut self, changes: &[TileChange], undo: bool) -> Result<(), String> {
        // Validate the entire batch before changing any tile.
        let mut previous = None;
        for change in changes {
            let current = if undo { &change.after } else { &change.before };
            if change.coord.x >= self.width.div_ceil(TILE_SIZE)
                || change.coord.y >= self.height.div_ceil(TILE_SIZE)
                || previous.is_some_and(|coord| coord >= (change.coord.y, change.coord.x))
                || [change.before.as_ref(), change.after.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|bytes| !valid_tile_payload(change.coord, bytes, self.width, self.height))
                || self.tiles.get(&change.coord) != current.as_ref()
            {
                return Err("Invalid tile change".into());
            }
            previous = Some((change.coord.y, change.coord.x));
        }
        for change in changes {
            let value = if undo { &change.before } else { &change.after };
            match value {
                Some(bytes) => {
                    self.tiles.insert(change.coord, bytes.clone());
                }
                None => {
                    self.tiles.remove(&change.coord);
                }
            }
        }
        Ok(())
    }
}

fn valid_tile_payload(coord: TileCoord, bytes: &[u8], width: u32, height: u32) -> bool {
    if bytes.len() != TILE_BYTES {
        return false;
    }
    let mut painted = false;
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let x = coord.x * TILE_SIZE + (index as u32 % TILE_SIZE);
        let y = coord.y * TILE_SIZE + (index as u32 / TILE_SIZE);
        if x >= width || y >= height || pixel[3] == 0 {
            if *pixel != [0, 0, 0, 0] {
                return false;
            }
        } else {
            painted = true;
        }
    }
    painted
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterLayer {
    pub id: String,
    pub name: String,
    pub tiles: SparseTiles,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RasterEdit {
    layer_id: String,
    changes: Vec<TileChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileInvalidation {
    pub layer_id: String,
    pub coords: Vec<TileCoord>,
}

/// Tile-backed document model for the future raster pipeline. This does not
/// participate in the current v1 stroke/SVG project format or renderer yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TiledRasterDocument {
    width: u32,
    height: u32,
    layers: Vec<RasterLayer>,
    undo: Vec<RasterEdit>,
    redo: Vec<RasterEdit>,
    revision: u64,
}

impl TiledRasterDocument {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        SparseTiles::new(width, height)?;
        Ok(Self {
            width,
            height,
            layers: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        })
    }

    pub fn layers(&self) -> &[RasterLayer] {
        &self.layers
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_layer(&mut self, id: String, name: String) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 64
            || name.trim().is_empty()
            || name.chars().count() > 255
            || self.layers.len() >= 16
            || self.layers.iter().any(|layer| layer.id == id)
        {
            return Err("Invalid or duplicate raster layer".into());
        }
        self.layers.push(RasterLayer {
            id,
            name,
            tiles: SparseTiles::new(self.width, self.height)?,
        });
        self.revision += 1;
        Ok(())
    }

    pub fn write_rect(
        &mut self,
        layer_id: &str,
        rect: [u32; 4],
        pixels: &[u8],
    ) -> Result<Option<TileInvalidation>, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        let changes = layer
            .tiles
            .write_rect(rect[0], rect[1], rect[2], rect[3], pixels)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let changed_coords = changes.iter().map(|change| change.coord).collect();
        self.undo.push(RasterEdit {
            layer_id: layer_id.into(),
            changes,
        });
        self.redo.clear();
        self.revision += 1;
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords: changed_coords,
        }))
    }

    pub fn undo(&mut self) -> Result<Option<TileInvalidation>, String> {
        self.apply_history(true)
    }

    pub fn redo(&mut self) -> Result<Option<TileInvalidation>, String> {
        self.apply_history(false)
    }

    fn apply_history(&mut self, undo: bool) -> Result<Option<TileInvalidation>, String> {
        let source = if undo { &self.undo } else { &self.redo };
        let Some(edit) = source.last() else {
            return Ok(None);
        };
        let invalidation = TileInvalidation {
            layer_id: edit.layer_id.clone(),
            coords: edit.changes.iter().map(|change| change.coord).collect(),
        };
        self.layers
            .iter_mut()
            .find(|layer| layer.id == edit.layer_id)
            .ok_or("Raster history layer is missing")?
            .tiles
            .apply_changes(&edit.changes, undo)?;
        let edit = if undo {
            self.undo.pop().unwrap()
        } else {
            self.redo.pop().unwrap()
        };
        if undo {
            self.redo.push(edit);
        } else {
            self.undo.push(edit);
        }
        self.revision += 1;
        Ok(Some(invalidation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_touched_tiles_and_restores_each_side_of_boundary() {
        let mut tiles = SparseTiles::new(512, 300).unwrap();
        let changes = tiles
            .write_rect(255, 255, 2, 2, &[10, 20, 30, 255].repeat(4))
            .unwrap();
        assert_eq!(changes.len(), 4);
        assert_eq!(tiles.allocated_tile_count(), 4);
        assert_eq!(tiles.pixel(256, 256), Some([10, 20, 30, 255]));
        assert_eq!(tiles.pixel(0, 0), Some([0; 4]));
        tiles.apply_changes(&changes, true).unwrap();
        assert_eq!(tiles.allocated_tile_count(), 0);
        tiles.apply_changes(&changes, false).unwrap();
        assert_eq!(tiles.pixel(255, 255), Some([10, 20, 30, 255]));
    }

    #[test]
    fn transparent_write_releases_tile_and_noop_has_no_history() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        assert!(tiles
            .write_rect(0, 0, 1, 1, &[0, 0, 0, 0])
            .unwrap()
            .is_empty());
        tiles.write_rect(0, 0, 1, 1, &[255, 0, 0, 255]).unwrap();
        let changes = tiles.write_rect(0, 0, 1, 1, &[200, 1, 2, 0]).unwrap();
        assert_eq!(tiles.allocated_tile_count(), 0);
        tiles.apply_changes(&changes, true).unwrap();
        assert_eq!(tiles.pixel(0, 0), Some([255, 0, 0, 255]));
    }

    #[test]
    fn invalid_rect_and_delta_leave_all_tiles_unchanged() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        let baseline = tiles.clone();
        assert!(tiles.write_rect(255, 0, 2, 1, &[0; 8]).is_err());
        assert!(tiles.write_rect(0, 0, 1, 1, &[0; 3]).is_err());
        assert_eq!(tiles, baseline);
        let invalid = [TileChange {
            coord: TileCoord { x: 0, y: 0 },
            before: None,
            after: Some(vec![0; 3]),
        }];
        assert!(tiles.apply_changes(&invalid, false).is_err());
        assert_eq!(tiles, baseline);
    }

    #[test]
    fn stale_delta_cannot_replace_a_newer_edit() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        let old = tiles.write_rect(0, 0, 1, 1, &[1, 2, 3, 255]).unwrap();
        tiles.write_rect(0, 0, 1, 1, &[4, 5, 6, 255]).unwrap();
        let baseline = tiles.clone();
        assert!(tiles.apply_changes(&old, true).is_err());
        assert_eq!(tiles, baseline);
    }

    #[test]
    fn raster_document_undoes_only_the_last_edited_layer() {
        let mut document = TiledRasterDocument::new(512, 256).unwrap();
        document.add_layer("lower".into(), "Lower".into()).unwrap();
        document.add_layer("upper".into(), "Upper".into()).unwrap();
        let changed = document
            .write_rect("lower", [255, 0, 2, 1], &[255, 0, 0, 255].repeat(2))
            .unwrap();
        assert_eq!(
            changed.unwrap().coords,
            [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
        );
        document
            .write_rect("upper", [255, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap();
        let revision = document.revision();
        assert_eq!(document.undo().unwrap().unwrap().layer_id, "upper");
        assert_eq!(document.revision(), revision + 1);
        assert_eq!(
            document.layers()[0].tiles.pixel(256, 0),
            Some([255, 0, 0, 255])
        );
        assert_eq!(document.layers()[1].tiles.pixel(255, 0), Some([0; 4]));
        assert_eq!(
            document.redo().unwrap().unwrap().coords,
            [TileCoord { x: 0, y: 0 }]
        );
        assert_eq!(
            document.layers()[1].tiles.pixel(255, 0),
            Some([0, 0, 255, 255])
        );
        assert!(document
            .write_rect("upper", [255, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap()
            .is_none());
        assert!(document.undo().unwrap().is_some());
        document
            .write_rect("upper", [0, 0, 1, 1], &[0, 255, 0, 255])
            .unwrap();
        assert!(document.redo().unwrap().is_none());
    }
}
