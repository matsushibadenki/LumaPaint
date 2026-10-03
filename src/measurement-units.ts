import { useSyncExternalStore } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DocumentUnit } from './bridge';
import type { Locale } from './i18n';
export const measurementUnits: DocumentUnit[] = ['pixels','millimeters','centimeters','inches','points'];
export const unitSymbols: Record<DocumentUnit,string> = { pixels:'px', millimeters:'mm', centimeters:'cm', inches:'in', points:'pt' };
const names = {ja:['ピクセル','ミリメートル','センチメートル','インチ','ポイント'],en:['Pixels','Millimeters','Centimeters','Inches','Points'],'zh-CN':['像素','毫米','厘米','英寸','点']};
export function unitName(unit: DocumentUnit, locale: Locale) { return names[locale][measurementUnits.indexOf(unit)]; }
export { pixelsPerMeasurement } from './measurement-math';
let current: DocumentUnit = 'pixels';
let started = false;
let revision = 0;
const subscribers = new Set<()=>void>();
function update(unit: DocumentUnit) { if (!measurementUnits.includes(unit)) return; revision++; current=unit; subscribers.forEach(fn=>fn()); }
async function initialize() {
  if (!isTauri()) { const saved=localStorage.getItem('lumapaint.measurement-unit') as DocumentUnit; if(measurementUnits.includes(saved))update(saved); return; }
  const generation=revision;
  await listen<DocumentUnit>('measurement-unit-changed', e=>update(e.payload));
  const unit=await invoke<DocumentUnit>('measurement_unit');
  if(generation===revision)update(unit);
}
function subscribe(fn:()=>void) { subscribers.add(fn); if(!started){started=true;void initialize().catch(console.error);} return ()=>{subscribers.delete(fn);}; }
export function useMeasurementUnit() { return useSyncExternalStore(subscribe,()=>current); }
export async function setMeasurementUnit(unit: DocumentUnit) {
  if(isTauri())await invoke('set_measurement_unit',{unit});
  else localStorage.setItem('lumapaint.measurement-unit',unit);
  update(unit);
}
