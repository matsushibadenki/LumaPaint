use lumapaint_core::tiles::{
    composite_raster_layers_tile, MaskTileState, RasterBlendMode, TileCoord, TiledRasterDocument,
};
use std::{hint::black_box, time::Instant};
fn main() {
    for (label, alpha, masked) in [
        ("opaque", 255, false),
        ("alpha", 160, false),
        ("mask", 160, true),
    ] {
        let mut d = TiledRasterDocument::new(256, 256).unwrap();
        for i in 0..8 {
            let id = format!("L{i}");
            d.add_layer(id.clone(), id.clone()).unwrap();
            let mut pix = vec![0; 256 * 256 * 4];
            for (n, p) in pix.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                p.copy_from_slice(&[
                    ((n % 256) as u8).wrapping_add(i * 7),
                    ((n / 256) as u8).wrapping_add(i * 11),
                    i * 30,
                    alpha,
                ]);
            }
            d.write_rect(&id, [0, 0, 256, 256], &pix).unwrap();
            if i > 0 {
                d.set_layer_blend_mode(&id, RasterBlendMode::Overlay)
                    .unwrap();
            }
        }
        let mut state = d.state();
        if masked {
            for l in &mut state.layers {
                l.mask_enabled = true;
                l.mask_tiles = vec![MaskTileState {
                    coord: TileCoord { x: 0, y: 0 },
                    values: vec![128; 256 * 256],
                }];
            }
        }
        for _ in 0..2 {
            black_box(composite_raster_layers_tile(
                &state.layers,
                TileCoord { x: 0, y: 0 },
            ));
        }
        let mut times = vec![];
        for _ in 0..9 {
            let t = Instant::now();
            black_box(composite_raster_layers_tile(
                &state.layers,
                TileCoord { x: 0, y: 0 },
            ));
            times.push(t.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        println!("{label}: median={:.3}ms samples={times:?}", times[4]);
    }
}
