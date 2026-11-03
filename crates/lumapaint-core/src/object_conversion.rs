use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversionKind {
    Expand,
    Appearance,
    Rasterize,
    Flatten,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectConversion {
    pub kind: ConversionKind,
    pub object: bool,
    pub fill: bool,
    pub stroke: bool,
    pub colors: u32,
    pub resolution: u32,
    pub transparent: bool,
    pub grayscale: bool,
    pub preserve_vectors: bool,
}
impl ObjectConversion {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1200).contains(&self.resolution)
            || !(2..=256).contains(&self.colors)
            || (self.kind == ConversionKind::Expand && !self.object && !self.fill && !self.stroke)
        {
            return Err("Invalid conversion settings / 変換設定が不正です / 转换设置无效".into());
        }
        Ok(())
    }
    pub fn combine(&self) -> bool {
        matches!(
            self.kind,
            ConversionKind::Rasterize | ConversionKind::Flatten
        )
    }
}
