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
                color[c] = ((decode[c * 2]
                    + f32::from(*n) / 255. * (decode[c * 2 + 1] - decode[c * 2]))
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
                for (p, a) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
                    p[3] = ((range[0] + f32::from(a) / 255. * (range[1] - range[0])).clamp(0., 1.)
                        * 255.)
                        .round() as u8;
                }
            }
        }
        // PDF soft masks take precedence over explicit masks and color keys.
        if let Some(mask) = stream.dict.get(b"Mask").ok().filter(|_| !has_soft_mask) {
            let mask = resolve(self.doc, mask)?;
            let Ok(array) = mask.as_array() else {
                return Err(ImportError::Unsupported("pdf.image_stencil"));
            };
            let ranges = nums(array, channels * 2)?;
            if ranges.as_chunks::<2>().0.iter().any(|r| {
                r[0] < 0. || r[1] > 255. || r[0] > r[1] || r.iter().any(|n| n.fract() != 0.)
            }) {
                return Err(malformed());
            }
            for (p, s) in rgba
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(samples.chunks_exact(channels))
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    fn parser(doc: &lopdf::Document) -> Interpreter<'_> {
        Interpreter {
            doc,
            body: String::new(),
            defs: String::new(),
            issues: vec![],
            remaining: LIMIT,
            operations: 0,
            serial: 0,
            allow_lossy: false,
            page_bounds: [0., 0., 10., 10.],
            font_cache: Default::default(),
            text_glyphs: 0,
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
        }
    }
    fn rgb(data: Vec<u8>) -> Stream {
        Stream::new(
            dictionary! { "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 },
            data,
        )
    }
    fn pixels(parser: &Interpreter<'_>) -> Vec<u8> {
        let data = parser
            .defs
            .split("base64,")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(frame.buffer_size());
        pixels
    }
    #[test]
    fn decode_color_keys_use_original_samples_and_cached_assets_decode_once() {
        let doc = lopdf::Document::new();
        let resources = Dictionary::new();
        let mut parser = parser(&doc);
        let mut image = rgb(vec![10, 20, 30]);
        image.dict.set(
            "Decode",
            vec![1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into()],
        );
        image.dict.set(
            "Mask",
            vec![
                10.into(),
                10.into(),
                20.into(),
                20.into(),
                30.into(),
                30.into(),
            ],
        );
        let first = parser.image(&image, &resources).unwrap();
        assert_eq!(pixels(&parser), [245, 235, 225, 0]);
        let budget = parser.image_remaining;
        assert_eq!(parser.image(&image, &resources).unwrap(), first);
        assert_eq!(parser.image_remaining, budget);
        assert_eq!(parser.defs.matches("<image").count(), 1);
    }
    #[test]
    fn soft_mask_decode_overrides_explicit_mask() {
        let mut doc = lopdf::Document::new();
        let mut alpha = Stream::new(
            dictionary! { "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
            vec![64],
        );
        alpha.dict.set("Decode", vec![1.into(), 0.into()]);
        let id = doc.add_object(alpha);
        let mut image = rgb(vec![255, 0, 0]);
        image.dict.set("SMask", id);
        image.dict.set(
            "Mask",
            vec![
                255.into(),
                255.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
        );
        let mut parser = parser(&doc);
        parser.image(&image, &Dictionary::new()).unwrap();
        assert_eq!(pixels(&parser), [255, 0, 0, 191]);
    }
    #[test]
    fn jpeg_is_decoded_with_declared_dimensions() {
        let doc = lopdf::Document::new();
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 100)
            .encode(&[180, 80, 40], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut image = rgb(data);
        image.dict.set("Filter", "DCTDecode");
        let mut parser = parser(&doc);
        parser.image(&image, &Dictionary::new()).unwrap();
        let decoded = pixels(&parser);
        assert!(decoded
            .iter()
            .zip([180, 80, 40, 255])
            .all(|(a, b)| a.abs_diff(b) <= 3));
        image.dict.set("Width", 2);
        parser.image_cache.clear();
        assert!(parser.image(&image, &Dictionary::new()).is_err());
    }
    #[test]
    fn malformed_filters_samples_and_masks_are_rejected() {
        let doc = lopdf::Document::new();
        let mut image = rgb(vec![0, 0, 0]);
        image.dict.set("Filter", 42);
        assert!(matches!(
            parser(&doc).image(&image, &Dictionary::new()),
            Err(ImportError::Malformed(_))
        ));
        image.dict.remove(b"Filter");
        image.content.pop();
        assert!(parser(&doc).image(&image, &Dictionary::new()).is_err());
        image.content.push(0);
        image.dict.set(
            "Mask",
            vec![
                256.into(),
                257.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
        );
        assert!(parser(&doc).image(&image, &Dictionary::new()).is_err());
    }
    #[test]
    fn capacity_limits_and_unsupported_encodings_never_silently_drop_images() {
        let doc = lopdf::Document::new();
        let resources = Dictionary::new();
        let mut image = rgb(vec![0, 0, 0]);
        let mut limited = parser(&doc);
        limited.image_remaining = 3;
        assert!(matches!(
            limited.image(&image, &resources),
            Err(ImportError::LimitExceeded("pdf.image_budget"))
        ));
        image.dict.set("Width", 8193);
        assert!(matches!(
            parser(&doc).image(&image, &resources),
            Err(ImportError::LimitExceeded("pdf.image_dimensions"))
        ));
        image.dict.set("Width", 1);
        image.dict.set("BitsPerComponent", 16);
        assert!(matches!(
            parser(&doc).image(&image, &resources),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        let mut lossy = parser(&doc);
        lossy.allow_lossy = true;
        assert!(lossy.image(&image, &resources).unwrap().is_none());
        assert_eq!(lossy.issues[0].code, "pdf.image_encoding");
    }
}
