//! Printing bounds in document coordinates; no renderer, GPU or UI dependency.
use crate::export::{ExportError, ExportSnapshot};
use lumapaint_core::document::Document;

pub(super) struct PrintLayout {
    pub bounds: [f32; 4],
    pub bleed: f32,
}
impl PrintLayout {
    pub fn capture(snapshot: &ExportSnapshot) -> Result<Option<Self>, ExportError> {
        let document = Document::from_document_state(snapshot.state().clone())
            .map_err(ExportError::InvalidDocument)?;
        let w = snapshot.state().width as f32;
        let h = snapshot.state().height as f32;
        let mm = snapshot.state().resolution.unwrap_or(72) as f32 / 25.4;
        let mut bounds = [0., 0., w, h];
        let mut outside = false;
        for layer in document.visible_svg_layers() {
            for o in layer.vector_objects.iter().filter(|o| o.visible) {
                if ![o.fill, o.stroke]
                    .into_iter()
                    .flatten()
                    .any(|p| p.registration)
                {
                    continue;
                }
                let [a, b, c, d, e, f] = o.transform;
                let half = if o.stroke.is_some_and(|p| p.registration) {
                    o.stroke_width / 2. * o.stroke_style.miter_limit.max(1.)
                } else {
                    0.
                };
                let dx = half * (a.abs() + c.abs());
                let dy = half * (b.abs() + d.abs());
                let contours = lumapaint_core::bezier::cubic_contours(&o.path.data)
                    .map_err(ExportError::InvalidDocument)?;
                // The transformed control hull contains every cubic, including extrema.
                for (points, _) in contours {
                    for p in points {
                        let x = a * p[0] + c * p[1] + e;
                        let y = b * p[0] + d * p[1] + f;
                        if [x - dx, y - dy, x + dx, y + dy]
                            .iter()
                            .any(|v| !v.is_finite())
                        {
                            return Err(ExportError::InvalidDocument(
                                "Non-finite PDF print geometry".into(),
                            ));
                        }
                        outside |= x - dx < 0. || y - dy < 0. || x + dx > w || y + dy > h;
                        bounds[0] = bounds[0].min(x - dx);
                        bounds[1] = bounds[1].min(y - dy);
                        bounds[2] = bounds[2].max(x + dx);
                        bounds[3] = bounds[3].max(y + dy);
                    }
                }
            }
        }
        if !outside {
            return Ok(None);
        }
        let bleed = 3. * mm;
        bounds = [
            bounds[0].min(-bleed) - mm,
            bounds[1].min(-bleed) - mm,
            bounds[2].max(w + bleed) + mm,
            bounds[3].max(h + bleed) + mm,
        ];
        if bounds.iter().any(|v| !v.is_finite())
            || (bounds[2] - bounds[0]) * 72. / snapshot.state().resolution.unwrap_or(72) as f32
                > 14_400.
            || (bounds[3] - bounds[1]) * 72. / snapshot.state().resolution.unwrap_or(72) as f32
                > 14_400.
        {
            return Err(ExportError::InvalidDocument(
                "PDF print bounds exceed 200 inches".into(),
            ));
        }
        Ok(Some(Self { bounds, bleed }))
    }
    pub fn svg(&self, xml: &str, snapshot: &ExportSnapshot) -> Result<String, ExportError> {
        let parsed = roxmltree::Document::parse(xml)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        let body: String = parsed
            .root_element()
            .children()
            .filter(|n| n.is_element())
            .map(|n| &xml[n.range()])
            .collect();
        let [x, y, r, b] = self.bounds;
        let width = r - x;
        let height = b - y;
        let bleed = self.bleed;
        let bw = snapshot.state().width as f32 + 2. * bleed;
        let bh = snapshot.state().height as f32 + 2. * bleed;
        // Clip artwork to the bleed. Registration marks are overlaid separately in /All.
        Ok(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" viewBox=\"{x} {y} {width} {height}\"><defs><clipPath id=\"lp-pdf-print-bleed\"><rect x=\"{}\" y=\"{}\" width=\"{bw}\" height=\"{bh}\"/></clipPath></defs><g clip-path=\"url(#lp-pdf-print-bleed)\">{body}</g></svg>", -bleed, -bleed))
    }
    pub fn boxes(
        &self,
        pdf: &mut lopdf::Document,
        snapshot: &ExportSnapshot,
    ) -> Result<(), ExportError> {
        let scale = 72. / snapshot.state().resolution.unwrap_or(72) as f32;
        let [x, y, r, b] = self.bounds;
        let w = snapshot.state().width as f32;
        let h = snapshot.state().height as f32;
        let real = |v: [f32; 4]| {
            v.into_iter()
                .map(|v| lopdf::Object::Real(v * scale))
                .collect::<Vec<_>>()
        };
        for id in pdf.get_pages().values() {
            let page = pdf
                .get_dictionary_mut(*id)
                .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
            let media = real([0., 0., r - x, b - y]);
            page.set("MediaBox", media.clone());
            page.set("CropBox", media);
            page.set("TrimBox", real([-x, b - h, w - x, b]));
            page.set(
                "BleedBox",
                real([
                    -x - self.bleed,
                    b - h - self.bleed,
                    w - x + self.bleed,
                    b + self.bleed,
                ]),
            );
        }
        Ok(())
    }
}
