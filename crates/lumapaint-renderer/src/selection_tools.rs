use lumapaint_core::{
    document::{Document, Point},
    selection_tools::Settings,
};
pub struct EdgeMap {
    size: (u32, u32),
    scale: f32,
    pixels: Vec<u8>,
}
impl EdgeMap {
    pub fn from_document(doc: &Document) -> Result<Self, String> {
        let png = crate::thumbnails::page_preview(doc, 1024)?;
        let image = image::load_from_memory(&png)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let size = image.dimensions();
        Ok(Self {
            size,
            scale: size.0 as f32 / doc.dimensions().0 as f32,
            pixels: image.into_raw(),
        })
    }
    fn channel(&self, x: i32, y: i32, c: usize) -> f32 {
        let x = x.clamp(0, self.size.0 as i32 - 1);
        let y = y.clamp(0, self.size.1 as i32 - 1);
        let i = ((y as u32 * self.size.0 + x as u32) * 4) as usize;
        self.pixels[i + c] as f32
    }
    fn edge(&self, x: i32, y: i32) -> f32 {
        (0..4)
            .map(|c| {
                (self.channel(x + 1, y, c) - self.channel(x - 1, y, c))
                    .hypot(self.channel(x, y + 1, c) - self.channel(x, y - 1, c))
            })
            .fold(0., f32::max)
    }
    pub fn snap(&self, point: Point, settings: Settings, screen_zoom: f32) -> Point {
        let radius = (settings.magnetic_width / screen_zoom.max(0.001) * self.scale).clamp(1., 40.);
        let cx = (point.x * self.scale).round() as i32;
        let cy = (point.y * self.scale).round() as i32;
        let mut best = 0.;
        let mut hit = point;
        for dy in -(radius.ceil() as i32)..=radius.ceil() as i32 {
            for dx in -(radius.ceil() as i32)..=radius.ceil() as i32 {
                let distance = (dx as f32).hypot(dy as f32);
                if distance > radius {
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                if x < 1 || y < 1 || x + 1 >= self.size.0 as i32 || y + 1 >= self.size.1 as i32 {
                    continue;
                }
                let edge = self.edge(x, y);
                if edge < settings.magnetic_contrast * 2.55 {
                    continue;
                }
                let score = edge * (1. - distance / (radius + 1.));
                if score > best {
                    best = score;
                    hit = Point {
                        x: x as f32 / self.scale,
                        y: y as f32 / self.scale,
                    };
                }
            }
        }
        hit
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn magnetic_snap_follows_color_or_alpha_edges_and_rejects_flat_regions() {
        let mut pixels = vec![0; 32 * 16 * 4];
        for y in 0..16 {
            for x in 16..32 {
                pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                    .copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let map = EdgeMap {
            size: (32, 16),
            scale: 1.,
            pixels,
        };
        let point = map.snap(
            Point { x: 13., y: 8. },
            Settings {
                magnetic_width: 6.,
                ..Default::default()
            },
            1.,
        );
        assert!((15. ..=16.).contains(&point.x));
        let flat = EdgeMap {
            size: (32, 16),
            scale: 1.,
            pixels: vec![255; 32 * 16 * 4],
        };
        assert_eq!(
            flat.snap(Point { x: 13., y: 8. }, Default::default(), 1.),
            Point { x: 13., y: 8. }
        );
    }
    #[test]
    fn polygon_selection_clips_paint_bucket_and_survives_native_reload() {
        use lumapaint_core::{document::Brush, selection::SelectionMode};
        use lumapaint_formats::native::NativeDocumentCodec;
        let mut doc = Document::default();
        doc.set_path_selection(
            vec![
                Point { x: 0., y: 0. },
                Point { x: 5., y: 0. },
                Point { x: 0., y: 5. },
            ],
            0.,
            SelectionMode::Replace,
        )
        .unwrap();
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.selection(), doc.selection());
        let result = crate::paint_bucket::fill(
            (8, 8),
            vec![0; 256],
            &[0; 256],
            Point { x: 1., y: 1. },
            Brush::default().color,
            Default::default(),
            loaded.selection(),
        )
        .unwrap();
        assert!(result.pixels[9 * 4 + 3] > 0);
        assert_eq!(result.pixels[(6 * 8 + 6) * 4 + 3], 0);
    }
}
