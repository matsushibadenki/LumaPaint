//! Bounded PDF image decoder. Images remain Rust-owned and become embedded SVG assets.
use super::*;
use base64::Engine;
use std::io::Cursor;
fn integer(doc: &lopdf::Document, dict: &Dictionary, key: &[u8]) -> Result<usize, ImportError> {
    let value = num(resolve(doc, dict.get(key).map_err(|_| malformed())?)?)?;
    if value.fract() != 0. || value < 1. {
        return Err(malformed());
    }
    Ok(value as usize)
}
impl Interpreter<'_> {
    fn image_samples(
        &mut self,
        stream: &lopdf::Stream,
        resources: &Dictionary,
    ) -> Result<(usize, usize, usize, Vec<u8>), ImportError> {
        let (w, h) = (
            integer(self.doc, &stream.dict, b"Width")?,
            integer(self.doc, &stream.dict, b"Height")?,
        );
        if w > 8192 || h > 8192 || w * h > 4_194_304 {
            return Err(ImportError::LimitExceeded("pdf.image_dimensions"));
        }
        let mut space = resolve(
            self.doc,
            stream.dict.get(b"ColorSpace").map_err(|_| malformed())?,
        )?;
        if let Ok(name) = space.as_name() {
            if ![b"DeviceRGB".as_slice(), b"DeviceGray", b"DeviceCMYK"].contains(&name) {
                space = resource(self.doc, resources, b"ColorSpace", name)?;
            }
        }
        let channels = self.space(space)?;
        if ![1, 3].contains(&channels) || integer(self.doc, &stream.dict, b"BitsPerComponent")? != 8
        {
            return Err(ImportError::Unsupported("pdf.image_encoding"));
        }
        let pixels = w * h * 4;
        if pixels > self.image_remaining {
            return Err(ImportError::LimitExceeded("pdf.image_budget"));
        }
        self.image_remaining -= pixels;
        let filters = if stream.dict.get(b"Filter").is_ok() {
            stream.filters().map_err(|_| malformed())?
        } else {
            Vec::new()
        };
        let data = if filters == [b"DCTDecode".as_slice()] {
            let mut reader = image::ImageReader::with_format(
                Cursor::new(&stream.content),
                image::ImageFormat::Jpeg,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(w as u32);
            limits.max_image_height = Some(h as u32);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            let decoded = reader.decode().map_err(|_| malformed())?;
            if (decoded.width() as usize, decoded.height() as usize) != (w, h) {
                return Err(malformed());
            }
            if channels == 1 {
                decoded.to_luma8().into_raw()
            } else {
                decoded.to_rgb8().into_raw()
            }
        } else {
            if filters.iter().any(|f| {
                ![
                    b"FlateDecode".as_slice(),
                    b"ASCII85Decode",
                    b"ASCIIHexDecode",
                    b"LZWDecode",
                    b"RunLengthDecode",
                ]
                .contains(f)
            }) {
                return Err(ImportError::Unsupported("pdf.image_encoding"));
            }
            stream
                .get_plain_content_with_limit(w * h * channels)
                .map_err(|_| ImportError::LimitExceeded("pdf.image_stream"))?
        };
        if data.len() != w * h * channels {
            return Err(malformed());
        }
        Ok((w, h, channels, data))
    }
    fn decoded_range(&self, dict: &Dictionary, channels: usize) -> Result<Vec<f32>, ImportError> {
        if let Ok(value) = dict.get(b"Decode") {
            nums(
                resolve(self.doc, value)?
                    .as_array()
                    .map_err(|_| malformed())?,
                channels * 2,
            )
        } else {
            Ok((0..channels).flat_map(|_| [0., 1.]).collect())
        }
    }
    pub(super) fn image(
        &mut self,
        stream: &lopdf::Stream,
        resources: &Dictionary,
    ) -> Result<Option<String>, ImportError> {
        let key = (stream as *const _ as usize, resources as *const _ as usize);
        if let Some(id) = self.image_cache.get(&key) {
            return Ok(Some(id.clone()));
        }
        let result = self.image_asset(stream, resources);
        match result {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                Ok(None)
            }
            other => other,
        }
    }
    fn image_asset(
        &mut self,
        stream: &lopdf::Stream,
        resources: &Dictionary,
    ) -> Result<Option<String>, ImportError> {
        if stream
            .dict
            .get(b"ImageMask")
            .and_then(Object::as_bool)
            .unwrap_or(false)
        {
            return Err(ImportError::Unsupported("pdf.image_stencil"));
        }
        let (w, h, channels, samples) = self.image_samples(stream, resources)?;
        let decode = self.decoded_range(&stream.dict, channels)?;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for p in samples.chunks_exact(channels) {
            let mut color = [0; 3];
            for (c, n) in p.iter().enumerate() {
                color[c] =
                    ((decode[c * 2] + f32::from(*n) / 255. * (decode[c * 2 + 1] - decode[c * 2]))
                        .clamp(0., 1.)
                        * 255.)
                        .round() as u8;
            }
            if channels == 1 {
                rgba.extend_from_slice(&[color[0]; 3]);
            } else {
                rgba.extend_from_slice(&color);
            }
            rgba.push(255);
        }
        let mut has_soft_mask = false;
        if let Ok(mask) = stream.dict.get(b"SMask") {
            let mask = resolve(self.doc, mask)?;
            if mask.as_name().ok() != Some(b"None") {
                has_soft_mask = true;
                let mask = mask.as_stream().map_err(|_| malformed())?;
                if mask.dict.get(b"Matte").is_ok() {
                    return Err(ImportError::Unsupported("pdf.image_matte"));
                }
                let (mw, mh, mc, alpha) = self.image_samples(mask, resources)?;
                if (mw, mh, mc) != (w, h, 1) {
                    return Err(ImportError::Unsupported("pdf.image_mask_size"));
                }
                let range = self.decoded_range(&mask.dict, 1)?;
                for (p, a) in rgba.chunks_exact_mut(4).zip(alpha) {
                    p[3] = ((range[0] + f32::from(a) / 255. * (range[1] - range[0])).clamp(0., 1.)
                        * 255.)
                        .round() as u8;
                }
            }
        }
        // PDF soft masks take precedence over explicit masks and color keys.
        if let Ok(mask) = stream.dict.get(b"Mask").ok().filter(|_| !has_soft_mask).ok_or_else(malformed) {
            let mask = resolve(self.doc, mask)?;
            let Ok(array) = mask.as_array() else {
                return Err(ImportError::Unsupported("pdf.image_stencil"));
            };
            let ranges = nums(array, channels * 2)?;
            if ranges.chunks_exact(2).any(|r| r[0] < 0. || r[1] > 255. || r[0] > r[1] || r.iter().any(|n| n.fract() != 0.)) {
                return Err(malformed());
            }
            for (p, s) in rgba.chunks_exact_mut(4).zip(samples.chunks_exact(channels)) {
                if s.iter().enumerate().all(|(c, n)| {
                    f32::from(*n) >= ranges[c * 2] && f32::from(*n) <= ranges[c * 2 + 1]
                }) {
                    p[3] = 0;
                }
            }
        }
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, w as u32, h as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|_| malformed())?
                .write_image_data(&rgba)
                .map_err(|_| malformed())?;
        }
        if self.defs.len() + self.body.len() + png.len() * 4 / 3 > 4 * 1024 * 1024 {
            return Err(ImportError::LimitExceeded("pdf.svg_source"));
        }
        let id = format!("pdf-image-{}", self.serial);
        self.serial += 1;
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let interpolation = stream
            .dict
            .get(b"Interpolate")
            .and_then(Object::as_bool)
            .unwrap_or(false);
        let _=write!(self.defs,"<image id=\"{id}\" width=\"1\" height=\"1\" preserveAspectRatio=\"none\" image-rendering=\"{}\" href=\"data:image/png;base64,{data}\"/>",if interpolation{"auto"}else{"pixelated"});
        self.image_cache.insert(
            (stream as *const _ as usize, resources as *const _ as usize),
            id.clone(),
        );
        Ok(Some(id))
    }
}
