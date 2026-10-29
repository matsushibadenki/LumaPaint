//! Prepare and commit on the document thread; keep raster work off the UI thread.
use super::*;
use base64::Engine;
use lumapaint_core::document::ImageResamplePlan;

pub(crate) struct Job {
    id: u64,
    revision: u64,
    plan: ImageResamplePlan,
    unchanged: bool,
}
pub(crate) struct Completed {
    job: Job,
    layers: Vec<(String, String)>,
    pixel_masks: Vec<(String, lumapaint_core::layer_mask::LayerMask)>,
}

pub(crate) fn prepare(id: u64, revision: u64, resolution: u32) -> Result<Job, String> {
    ensure_document_open()?;
    if paint_gesture_active()
        || ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some())
        || ACTIVE_DOCUMENT_ID.with(|d| d.get()) != id
        || DOCUMENT.with(|d| d.borrow().revision()) != revision
    {
        return Err(
            "Document changed or unsupported / 文書が変更されたか未対応です / 文档已更改或不受支持"
                .into(),
        );
    }
    let plan = DOCUMENT.with(|d| d.borrow().prepare_image_resample(resolution))?;
    let unchanged = DOCUMENT.with(|d| d.borrow().snapshot().resolution == resolution);
    Ok(Job {
        id,
        revision,
        plan,
        unchanged,
    })
}
pub(crate) fn evaluate(job: Job) -> Result<Completed, String> {
    let mut layers = Vec::new();
    if job.unchanged {
        return Ok(Completed {
            job,
            layers,
            pixel_masks: Vec::new(),
        });
    }
    for (id, source, canonical_png) in &job.plan.sources {
        if *canonical_png {
            let encoded = source
                .split("data:image/png;base64,")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .ok_or("Invalid PNG source")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| e.to_string())?;
            if bytes.len() < 24
                || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
                || u32::from_be_bytes(bytes[16..20].try_into().unwrap()) != job.plan.old.0
                || u32::from_be_bytes(bytes[20..24].try_into().unwrap()) != job.plan.old.1
            {
                return Err("Invalid PNG dimensions".into());
            }
            let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?
                .to_rgba8();
            if image
                .pixels()
                .any(|p| p[3] == 0 && (p[0] != 0 || p[1] != 0 || p[2] != 0))
            {
                return Err("Cannot resample hidden transparent RGB / 透明画素のRGBを保持したリサンプリングは未対応です / 暂不支持保留透明像素RGB的重采样".into());
            }
        }
        let raster =
            lumapaint_renderer::vector::rasterize_svg(source, job.plan.old.0, job.plan.old.1)?;
        if !*canonical_png && !raster.fully_contained {
            return Err("Pixel content outside the canvas cannot be resampled yet / カンバス外の画像を含むレイヤーはリサンプリング未対応です / 暂不支持重采样画布外有内容的图层".into());
        }
        let pixels = lumapaint_renderer::resample::rgba8_triangle(
            raster.pixels,
            job.plan.old,
            job.plan.new,
        )?;
        let png = lumapaint_renderer::vector::document_png(job.plan.new.0, job.plan.new.1, pixels)?;
        layers.push((
            id.clone(),
            clipboard::image_svg(job.plan.new.0, job.plan.new.1, &png),
        ));
    }
    let mut pixel_masks = Vec::new();
    for (id, mask) in &job.plan.pixel_masks {
        pixel_masks.push((id.clone(), resample_mask(mask, job.plan.old, job.plan.new)?));
    }
    Ok(Completed {
        job,
        layers,
        pixel_masks,
    })
}

fn resample_mask(
    mask: &lumapaint_core::layer_mask::LayerMask,
    old: (u32, u32),
    new: (u32, u32),
) -> Result<lumapaint_core::layer_mask::LayerMask, String> {
    use lumapaint_core::layer_mask::MaskContent;
    mask.validate()?;
    let Some(MaskContent::Pixel {
        width,
        height,
        runs,
        gray,
    }) = &mask.content
    else {
        return Err("Invalid pixel mask".into());
    };
    let nw = (f64::from(*width) * f64::from(new.0) / f64::from(old.0)).round() as u32;
    let nh = (f64::from(*height) * f64::from(new.1) / f64::from(old.1)).round() as u32;
    if *width == 0
        || *height == 0
        || nw == 0
        || nh == 0
        || u64::from(*width) * u64::from(*height) > 16_777_216
        || u64::from(nw) * u64::from(nh) > 16_777_216
        || nw > 8192
        || nh > 8192
    {
        return Err("Pixel mask is too large or small to resample / マスクのサイズが対応範囲外です / 蒙版尺寸超出重采样范围".into());
    }
    let mut pixels = vec![0u8; *width as usize * *height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    for [start, len] in runs {
        for pixel in pixels[*start as usize * 4..(*start + *len) as usize * 4].chunks_exact_mut(4) {
            pixel[..3].fill(255);
        }
    }
    for [start, len, value] in gray {
        for pixel in pixels[*start as usize * 4..(*start + *len) as usize * 4].chunks_exact_mut(4) {
            pixel[..3].fill(*value as u8);
        }
    }
    let out = lumapaint_renderer::resample::rgba8_triangle(pixels, (*width, *height), (nw, nh))?;
    let mut white = Vec::new();
    let mut partial = Vec::new();
    for y in 0..nh {
        let mut x = 0;
        while x < nw {
            let start = x;
            let value = out[((y * nw + x) * 4) as usize];
            x += 1;
            while x < nw && out[((y * nw + x) * 4) as usize] == value {
                x += 1;
            }
            if value == 255 {
                white.push([y * nw + start, x - start]);
            } else if value != 0 {
                partial.push([y * nw + start, x - start, value as u32]);
            }
            if white.len() > 1_048_576 || partial.len() > 1_048_576 {
                return Err("Resampled mask exceeds complexity limit / マスクが複雑すぎます / 重采样后的蒙版过于复杂".into());
            }
        }
    }
    let mut result = mask.clone();
    let global = [
        new.0 as f32 / old.0 as f32,
        0.,
        0.,
        new.1 as f32 / old.1 as f32,
        0.,
        0.,
    ];
    let local_inverse = [
        *width as f32 / nw as f32,
        0.,
        0.,
        *height as f32 / nh as f32,
        0.,
        0.,
    ];
    result.transform = lumapaint_core::image_frame::multiply(
        global,
        lumapaint_core::image_frame::multiply(mask.transform, local_inverse),
    );
    result.content = Some(MaskContent::Pixel {
        width: nw,
        height: nh,
        runs: white,
        gray: partial,
    });
    result.validate()?;
    Ok(result)
}
pub(crate) fn commit(done: Completed) -> Result<DocumentSnapshot, String> {
    if paint_gesture_active()
        || ACTIVE_DOCUMENT_ID.with(|d| d.get()) != done.job.id
        || DOCUMENT.with(|d| d.borrow().revision()) != done.job.revision
    {
        return Err("Document changed during resampling / リサンプリング中に文書が変更されました / 重采样期间文档已更改".into());
    }
    if done.job.unchanged {
        return Ok(DOCUMENT.with(|d| d.borrow().snapshot()));
    }
    DOCUMENT.with(|d| {
        d.borrow_mut()
            .commit_image_resample(&done.job.plan, done.layers, done.pixel_masks)
    })?;
    timeline::refresh_history_preview();
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::layer_mask::{LayerMask, MaskContent, MaskKind};
    #[test]
    fn pixel_mask_resampling_filters_coverage_and_preserves_transform() {
        let mask = LayerMask {
            linked: false,
            transform: [1., 0., 0., 1., 3., 4.],
            kind: MaskKind::Pixel,
            enabled: true,
            inverted: false,
            density: 1.,
            content: Some(MaskContent::Pixel {
                width: 2,
                height: 2,
                runs: vec![[0, 1], [3, 1]],
                gray: vec![],
            }),
        };
        let result = resample_mask(&mask, (4, 4), (2, 2)).unwrap();
        assert_eq!(result.transform, [1., 0., 0., 1., 1.5, 2.]);
        let Some(MaskContent::Pixel {
            width,
            height,
            runs,
            gray,
        }) = result.content
        else {
            panic!("pixel mask missing")
        };
        assert_eq!((width, height), (1, 1));
        assert!(runs.is_empty());
        assert_eq!(gray.len(), 1);
        assert!((120..=135).contains(&gray[0][2]));
    }
}
