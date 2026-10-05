import type { DocumentUnit } from './bridge';
export function pixelsPerMeasurement(unit: DocumentUnit, resolution: number) {
  const dpi = Number.isFinite(resolution) && resolution > 0 ? resolution : 72;
  return unit==='pixels'?1:unit==='inches'?dpi:unit==='centimeters'?dpi/2.54:unit==='points'?dpi/72:dpi/25.4;
}
