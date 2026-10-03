//! Linked graphics and frame fitting. No filesystem, platform, or renderer dependency.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameFit {
    #[default]
    Contain,
    Cover,
    Stretch,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameImage {
    pub source_path: Option<String>,
    pub fingerprint: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub encoded_width: u32,
    pub encoded_height: u32,
    pub orientation_transform: [f32; 6],
    /// Embedded last-known preview keeps missing links visible and exports portable.
    pub data_uri: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageFrame {
    pub fitting: FrameFit,
    pub image: Option<FrameImage>,
    pub content_transform: Option<[f32; 6]>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFrameSummary {
    pub fitting: FrameFit,
    pub source_path: Option<String>,
    pub name: Option<String>,
    pub size: Option<[u32; 2]>,
    pub content_transform: Option<[f32; 6]>,
}
pub fn multiply(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
pub fn inverse(m: [f32; 6]) -> Result<[f32; 6], String> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det.abs() < 1e-10 {
        return Err("Invalid frame transform".into());
    }
    Ok([
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ])
}
impl ImageFrame {
    pub fn summary(&self) -> ImageFrameSummary {
        ImageFrameSummary {
            fitting: self.fitting,
            source_path: self.image.as_ref().and_then(|i| i.source_path.clone()),
            name: self.image.as_ref().map(|i| i.name.clone()),
            size: self.image.as_ref().map(|i| [i.width, i.height]),
            content_transform: self.content_transform,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.content_transform.is_some_and(|m| {
            m.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
                || (m[0] * m[3] - m[1] * m[2]).abs() < 1e-10
        }) {
            return Err("Invalid frame content transform".into());
        }
        if let Some(i) = &self.image {
            if [i.width, i.height, i.encoded_width, i.encoded_height]
                .iter()
                .any(|n| *n == 0 || *n > 100_000)
                || i.orientation_transform.iter().any(|n| !n.is_finite())
                || i.name.len() > 1024
                || i.fingerprint.len() > 128
                || i.source_path
                    .as_ref()
                    .is_some_and(|p| p.is_empty() || p.len() > 4096 || p.contains('\0'))
            {
                return Err("Invalid linked image metadata".into());
            }
            let data = i
                .data_uri
                .strip_prefix("data:image/png;base64,")
                .or_else(|| i.data_uri.strip_prefix("data:image/jpeg;base64,"))
                .ok_or("Unsupported image preview")?;
            if data.is_empty()
                || data.len() > 4 * 1024 * 1024
                || !data
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c))
            {
                return Err("Invalid linked image preview".into());
            }
        }
        Ok(())
    }
    pub fn fit(&mut self, bounds: [f32; 4], fitting: FrameFit) -> Result<(), String> {
        self.fitting = fitting;
        let Some(i) = &self.image else {
            return Ok(());
        };
        let [x, y, r, b] = bounds;
        let w = (r - x).max(0.001);
        let h = (b - y).max(0.001);
        let sx = w / i.width as f32;
        let sy = h / i.height as f32;
        let (sx, sy) = match fitting {
            FrameFit::Contain => (sx.min(sy), sx.min(sy)),
            FrameFit::Cover => (sx.max(sy), sx.max(sy)),
            FrameFit::Stretch => (sx, sy),
        };
        self.content_transform = Some([
            sx,
            0.,
            0.,
            sy,
            x + (w - i.width as f32 * sx) * 0.5,
            y + (h - i.height as f32 * sy) * 0.5,
        ]);
        self.validate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitting_preserves_aspect_and_centers() {
        let image = FrameImage {
            source_path: Some("/a.png".into()),
            fingerprint: "1".into(),
            name: "a".into(),
            width: 200,
            height: 100,
            encoded_width: 200,
            encoded_height: 100,
            orientation_transform: [1., 0., 0., 1., 0., 0.],
            data_uri: "data:image/png;base64,AA==".into(),
        };
        let mut f = ImageFrame {
            image: Some(image),
            ..Default::default()
        };
        f.fit([10., 20., 110., 120.], FrameFit::Contain).unwrap();
        assert_eq!(f.content_transform, Some([0.5, 0., 0., 0.5, 10., 45.]));
        f.fit([10., 20., 110., 120.], FrameFit::Cover).unwrap();
        assert_eq!(f.content_transform, Some([1., 0., 0., 1., -40., 20.]));
        f.fit([10., 20., 110., 120.], FrameFit::Stretch).unwrap();
        assert_eq!(f.content_transform, Some([0.5, 0., 0., 1., 10., 20.]));
        assert!(!serde_json::to_string(&f.summary())
            .unwrap()
            .contains("data:image"));
    }
}
