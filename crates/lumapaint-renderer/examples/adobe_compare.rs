//! Reproducible local Adobe export comparison; no Adobe automation or network access.
//! Generate: cargo run -p lumapaint-renderer --example adobe_compare -- generate /tmp/adobe-check
//! Compare: ... -- compare /tmp/adobe-check/reference.png /tmp/adobe-check/adobe.png
use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("generate") if args.len() == 3 => {
            let dir = Path::new(&args[2]);
            fs::create_dir_all(dir)?;
            // Points and a 72 ppi export make the Adobe artboard exactly 256 pixels.
            // No fonts or external resources: this first fixture isolates geometry/compositing.
            let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="256pt" height="256pt" viewBox="0 0 256 256">
<defs><linearGradient id="g"><stop stop-color="#ff8000"/><stop offset="1" stop-color="#0080ff"/></linearGradient><clipPath id="c"><circle cx="192" cy="192" r="42"/></clipPath></defs>
<rect width="256" height="256" fill="white"/>
<rect x="16" y="16" width="80" height="80" fill="#2080c0"/>
<path d="M144 24 L224 88" fill="none" stroke="#602080" stroke-width="8" stroke-linecap="round"/>
<path d="M16 208 C16 112 96 112 96 208 Z" fill="#30a060"/>
<rect x="144" y="144" width="96" height="96" fill="url(#g)" clip-path="url(#c)"/>
<rect x="64" y="64" width="64" height="64" fill="#e02040" opacity="0.5"/>
</svg>"##;
            fs::write(dir.join("fixture.svg"), source)?;
            let raster = lumapaint_renderer::vector::rasterize_svg(source, 256, 256)?;
            let backend = format!("{:?}", raster.backend);
            let mut pixels = raster.pixels;
            for p in pixels.as_chunks_mut::<4>().0 {
                if p[3] != 0 {
                    for c in 0..3 {
                        p[c] = ((u32::from(p[c]) * 255 + u32::from(p[3]) / 2) / u32::from(p[3]))
                            .min(255) as u8;
                    }
                }
            }
            image::save_buffer(
                dir.join("reference.png"),
                &pixels,
                256,
                256,
                image::ColorType::Rgba8,
            )?;
            fs::write(dir.join("reference-backend.txt"), &backend)?;
            println!("Generated 256 × 256 reference using {backend}. Export fixture.svg from Adobe at 72 ppi, RGB, artboard bounds.");
        }
        Some("compare") if args.len() == 4 => {
            let a = image::open(&args[2])?.to_rgba8();
            let b = image::open(&args[3])?.to_rgba8();
            if a.dimensions() != b.dimensions() {
                return Err(format!(
                    "Dimensions differ: {:?} vs {:?}; no resampling performed",
                    a.dimensions(),
                    b.dimensions()
                )
                .into());
            }
            let mut changed_pixels = 0_u64;
            let mut sum = 0_u64;
            let mut max = 0_u8;
            let mut diff = image::RgbaImage::new(a.width(), a.height());
            for ((pa, pb), pd) in a.pixels().zip(b.pixels()).zip(diff.pixels_mut()) {
                let mut changed = false;
                for c in 0..4 {
                    // Compare premultiplied channels to ignore undefined transparent RGB.
                    let va = if c == 3 {
                        pa[c]
                    } else {
                        ((u16::from(pa[c]) * u16::from(pa[3]) + 127) / 255) as u8
                    };
                    let vb = if c == 3 {
                        pb[c]
                    } else {
                        ((u16::from(pb[c]) * u16::from(pb[3]) + 127) / 255) as u8
                    };
                    let d = va.abs_diff(vb);
                    sum += u64::from(d);
                    max = max.max(d);
                    changed |= d != 0;
                    if c < 3 {
                        pd[c] = d.saturating_mul(8);
                    }
                }
                pd[3] = 255;
                changed_pixels += u64::from(changed);
            }
            let out = Path::new(&args[3]).with_extension("diff.png");
            diff.save(&out)?;
            println!("pixels={} changed_pixels={changed_pixels} max_channel_difference={max} mean_channel_difference={:.6} diff={}", a.width() as u64 * a.height() as u64, sum as f64 / a.as_raw().len() as f64, out.display());
        }
        _ => {
            return Err(
                "Usage: adobe_compare generate DIRECTORY | compare REFERENCE.png ADOBE.png".into(),
            )
        }
    }
    Ok(())
}
