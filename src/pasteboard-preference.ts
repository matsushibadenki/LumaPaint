import { useEffect, useState } from 'react';
import { readPreference, savePreference } from './i18n';
import type { Brush } from './bridge';
const key = 'pasteboardColor';
const eventName = 'lumapaint-pasteboard-color';
export function readPasteboardColor(): Brush['color'] | null {
  const value = readPreference(key);
  return value && /^#[0-9a-f]{6}$/i.test(value) ? [1, 3, 5].map(index => parseInt(value.slice(index, index + 2), 16)) as Brush['color'] : null;
}
export function usePasteboardColor() {
  const [color, setColor] = useState(readPasteboardColor);
  useEffect(() => {
    const update = () => setColor(readPasteboardColor());
    window.addEventListener('storage', update);
    window.addEventListener(eventName, update);
    return () => { window.removeEventListener('storage', update); window.removeEventListener(eventName, update); };
  }, []);
  const change = (next: Brush['color'] | null) => {
    savePreference(key, next ? '#' + next.map(value => value.toString(16).padStart(2, '0')).join('') : 'theme');
    setColor(next);
    window.dispatchEvent(new Event(eventName));
  };
  return [color, change] as const;
}
