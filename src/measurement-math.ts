import type { DocumentUnit } from './bridge';
export function pixelsPerMeasurement(unit: DocumentUnit, resolution: number) {
  const dpi = Math.max(1,resolution);
  return unit==='pixels'?1:unit==='inches'?dpi:unit==='centimeters'?dpi/2.54:unit==='points'?dpi/72:dpi/25.4;
}
