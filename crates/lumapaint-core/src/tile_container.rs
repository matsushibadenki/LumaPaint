//! Bounded binary container for authoritative tiled raster state.
//! This is independent from the legacy `.lumapaint` v1 JSON format.
use crate::tiles::{
    MaskTileState, RasterLayerState, RasterTileState, TileCoord, TiledRasterDocument,
    TiledRasterState, TILE_SIZE,
};
use crc32fast::hash;
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 8] = b"LPTILE2\0";
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 28;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_CONTAINER_BYTES: usize = 512 * 1024 * 1024;
const PIXEL_TILE_BYTES: usize = TILE_SIZE as usize * TILE_SIZE as usize * 4;
const MASK_TILE_BYTES: usize = TILE_SIZE as usize * TILE_SIZE as usize;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    width: u32,
    height: u32,
    layers: Vec<LayerManifest>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayerManifest {
    id: String,
    name: String,
    visible: bool,
    opacity: f32,
    locked: bool,
    alpha_locked: bool,
    mask_enabled: bool,
    mask_inverted: bool,
    mask_density: f32,
    tiles: Vec<TileReference>,
    mask_tiles: Vec<TileReference>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TileReference {
    coord: TileCoord,
    offset: u64,
    length: u32,
    checksum: u32,
}

pub fn encode(state: &TiledRasterState) -> Result<Vec<u8>, String> {
    TiledRasterDocument::from_state(state.clone())?;
    let mut payload = Vec::new();
    let mut layers = Vec::with_capacity(state.layers.len());
    for layer in &state.layers {
        let tiles = references(&layer.tiles, &mut payload, |tile| {
            (&tile.coord, &tile.pixels)
        })?;
        let mask_tiles = references(&layer.mask_tiles, &mut payload, |tile| {
            (&tile.coord, &tile.values)
        })?;
        layers.push(LayerManifest {
            id: layer.id.clone(),
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            locked: layer.locked,
            alpha_locked: layer.alpha_locked,
            mask_enabled: layer.mask_enabled,
            mask_inverted: layer.mask_inverted,
            mask_density: layer.mask_density,
            tiles,
            mask_tiles,
        });
    }
    let manifest = serde_json::to_vec(&Manifest {
        width: state.width,
        height: state.height,
        layers,
    })
    .map_err(|error| error.to_string())?;
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err("Tile manifest is too large".into());
    }
    let total = HEADER_BYTES
        .checked_add(manifest.len())
        .and_then(|length| length.checked_add(payload.len()))
        .ok_or("Tile container size overflow")?;
    if total > MAX_CONTAINER_BYTES {
        return Err("Tile container is too large".into());
    }
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_le_bytes());
    output.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    output.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    output.extend_from_slice(&hash(&manifest).to_le_bytes());
    output.extend_from_slice(&manifest);
    output.extend_from_slice(&payload);
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> Result<TiledRasterState, String> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_CONTAINER_BYTES || &bytes[..8] != MAGIC {
        return Err("Invalid tile container header".into());
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let manifest_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let payload_len = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let manifest_checksum = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    if version != VERSION || manifest_len > MAX_MANIFEST_BYTES {
        return Err("Unsupported tile container".into());
    }
    let payload_len = usize::try_from(payload_len).map_err(|_| "Tile payload is too large")?;
    let manifest_end = HEADER_BYTES
        .checked_add(manifest_len)
        .ok_or("Tile container size overflow")?;
    let expected = manifest_end
        .checked_add(payload_len)
        .ok_or("Tile container size overflow")?;
    if expected != bytes.len() {
        return Err("Tile container length mismatch".into());
    }
    let manifest_bytes = &bytes[HEADER_BYTES..manifest_end];
    if hash(manifest_bytes) != manifest_checksum {
        return Err("Tile manifest checksum mismatch".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(manifest_bytes).map_err(|error| error.to_string())?;
    let payload = &bytes[manifest_end..];
    let mut cursor = 0usize;
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for layer in manifest.layers {
        let tiles = read_tiles(&layer.tiles, payload, PIXEL_TILE_BYTES, &mut cursor)?;
        let mask_tiles = read_masks(&layer.mask_tiles, payload, MASK_TILE_BYTES, &mut cursor)?;
        layers.push(RasterLayerState {
            id: layer.id,
            name: layer.name,
            visible: layer.visible,
            opacity: layer.opacity,
            locked: layer.locked,
            alpha_locked: layer.alpha_locked,
            mask_enabled: layer.mask_enabled,
            mask_inverted: layer.mask_inverted,
            mask_density: layer.mask_density,
            tiles,
            mask_tiles,
        });
    }
    if cursor != payload.len() {
        return Err("Tile payload contains unreferenced bytes".into());
    }
    let state = TiledRasterState {
        width: manifest.width,
        height: manifest.height,
        layers,
    };
    TiledRasterDocument::from_state(state.clone())?;
    Ok(state)
}

fn references<T>(
    tiles: &[T],
    payload: &mut Vec<u8>,
    value: impl Fn(&T) -> (&TileCoord, &[u8]),
) -> Result<Vec<TileReference>, String> {
    tiles
        .iter()
        .map(|tile| {
            let (coord, bytes) = value(tile);
            let offset = u64::try_from(payload.len()).map_err(|_| "Tile offset overflow")?;
            let length = u32::try_from(bytes.len()).map_err(|_| "Tile length overflow")?;
            payload.extend_from_slice(bytes);
            Ok(TileReference {
                coord: *coord,
                offset,
                length,
                checksum: hash(bytes),
            })
        })
        .collect()
}

fn checked_bytes<'a>(
    reference: &TileReference,
    payload: &'a [u8],
    expected_len: usize,
    cursor: &mut usize,
) -> Result<&'a [u8], String> {
    let offset = usize::try_from(reference.offset).map_err(|_| "Tile offset overflow")?;
    let length = reference.length as usize;
    if offset != *cursor || length != expected_len {
        return Err("Invalid tile payload reference".into());
    }
    let end = offset
        .checked_add(length)
        .ok_or("Tile reference overflow")?;
    let bytes = payload
        .get(offset..end)
        .ok_or("Tile reference outside payload")?;
    if hash(bytes) != reference.checksum {
        return Err("Tile payload checksum mismatch".into());
    }
    *cursor = end;
    Ok(bytes)
}

fn read_tiles(
    references: &[TileReference],
    payload: &[u8],
    expected_len: usize,
    cursor: &mut usize,
) -> Result<Vec<RasterTileState>, String> {
    references
        .iter()
        .map(|reference| {
            Ok(RasterTileState {
                coord: reference.coord,
                pixels: checked_bytes(reference, payload, expected_len, cursor)?.to_vec(),
            })
        })
        .collect()
}

fn read_masks(
    references: &[TileReference],
    payload: &[u8],
    expected_len: usize,
    cursor: &mut usize,
) -> Result<Vec<MaskTileState>, String> {
    references
        .iter()
        .map(|reference| {
            Ok(MaskTileState {
                coord: reference.coord,
                values: checked_bytes(reference, payload, expected_len, cursor)?.to_vec(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> TiledRasterState {
        let mut document = TiledRasterDocument::new(257, 17).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect(
                "paint",
                [255, 4, 2, 1],
                &[200, 40, 10, 255, 10, 80, 160, 128],
            )
            .unwrap();
        document.set_layer_mask("paint", true, false, 0.8).unwrap();
        document
            .write_mask_rect("paint", [255, 4, 2, 1], &[0, 128])
            .unwrap();
        document.state()
    }

    #[test]
    fn binary_container_round_trips_pixels_masks_and_manifest() {
        let state = sample_state();
        let encoded = encode(&state).unwrap();
        assert_eq!(&encoded[..8], MAGIC);
        assert_eq!(decode(&encoded).unwrap(), state);
    }

    #[test]
    fn binary_container_rejects_truncation_and_checksum_corruption() {
        let encoded = encode(&sample_state()).unwrap();
        assert!(decode(&encoded[..encoded.len() - 1]).is_err());

        let mut manifest_damage = encoded.clone();
        manifest_damage[HEADER_BYTES] ^= 1;
        assert!(decode(&manifest_damage).is_err());

        let mut payload_damage = encoded;
        *payload_damage.last_mut().unwrap() ^= 1;
        assert!(decode(&payload_damage).is_err());
    }

    #[test]
    fn binary_container_rejects_gaps_and_unreferenced_payload() {
        let encoded = encode(&sample_state()).unwrap();
        let manifest_len = u32::from_le_bytes(encoded[12..16].try_into().unwrap()) as usize;
        let manifest_end = HEADER_BYTES + manifest_len;
        let mut manifest: Manifest =
            serde_json::from_slice(&encoded[HEADER_BYTES..manifest_end]).unwrap();
        manifest.layers[0].tiles[0].offset = 1;
        let replacement = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(replacement.len(), manifest_len);
        let mut invalid = encoded;
        invalid[24..28].copy_from_slice(&hash(&replacement).to_le_bytes());
        invalid[HEADER_BYTES..manifest_end].copy_from_slice(&replacement);
        assert!(decode(&invalid).is_err());
    }
}
