// Small UI color coordinates only; document pixels remain in Rust/GPU memory.
// CMYK uses a device-independent arithmetic approximation, not an ICC transform.
export type RgbColor = [number, number, number];
export type CmykColor = [number, number, number, number];

export function rgbToCmyk([r, g, b]: RgbColor): CmykColor {
  const max = Math.max(r, g, b) / 255;
  if (max === 0) return [0, 0, 0, 100];
  return [(1 - r / 255 / max) * 100, (1 - g / 255 / max) * 100, (1 - b / 255 / max) * 100, (1 - max) * 100];
}

export function cmykToRgb([c, m, y, k]: CmykColor): RgbColor {
  return [c, m, y].map(ink => Math.round(255 * (1 - ink / 100) * (1 - k / 100))) as RgbColor;
}
