import { useSyncExternalStore } from 'react';

export type ColorWheelShape = 'square' | 'triangle';
const key = 'lumapaint.color-wheel-shape';
const changeEvent = 'lumapaint-color-wheel-shape';
function snapshot(): ColorWheelShape {
  if (storageFailed) return fallback;
  try { return localStorage.getItem(key) === 'triangle' ? 'triangle' : 'square'; }
  catch { return fallback; }
}
let fallback: ColorWheelShape = 'square';
let storageFailed = false;
function subscribe(listener: () => void) {
  const storage = (event: StorageEvent) => { if (event.key === key || event.key === null) listener(); };
  window.addEventListener(changeEvent, listener);
  window.addEventListener('storage', storage);
  return () => { window.removeEventListener(changeEvent, listener); window.removeEventListener('storage', storage); };
}
export function setColorWheelShape(shape: ColorWheelShape) {
  fallback = shape;
  try { localStorage.setItem(key, shape); storageFailed = false; }
  catch { storageFailed = true; }
  window.dispatchEvent(new Event(changeEvent));
}
export function useColorWheelShape() {
  return useSyncExternalStore(subscribe, snapshot, () => 'square' as const);
}
