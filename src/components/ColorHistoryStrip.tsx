import { useLayoutEffect, useRef, useState } from 'react';
import { colorHistoryLabels, useColorUsage, type UsedColor } from '../color-history';
import type { Locale } from '../i18n';
import { toHex } from './BrushControls';

export function ColorHistoryStrip({ locale, onSelect }: { locale: Locale; onSelect: (color: UsedColor) => void }) {
  const { colors, resize, commit } = useColorUsage();
  const root = useRef<HTMLDivElement>(null);
  const resizeRef = useRef(resize); resizeRef.current = resize;
  const [capacity, setCapacity] = useState(0);
  const [size, setSize] = useState(0);
  const t = colorHistoryLabels[locale];
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    let previous = -1;
    const measure = () => {
      const { width, height } = element.getBoundingClientRect();
      const side = height * .9;
      const next = side > 0 ? Math.min(512, Math.floor(width / side)) : 0;
      setSize(side); setCapacity(next);
      if (next !== previous && height > 0) { previous = next; resizeRef.current(next); }
    };
    const observer = new ResizeObserver(measure); observer.observe(element); measure();
    return () => observer.disconnect();
  }, []);
  return <div ref={root} className="color-history-strip" role="group" aria-label={t.history}>
    {colors.slice(0, capacity).map(item => {
      const hex = toHex(item.color).toUpperCase();
      const label = `${t.apply} · ${hex} · ${Math.round(item.alpha * 100)}% · ${item.colorProfile}`;
      return <button type="button" key={`${hex}:${item.alpha}:${item.colorMode}:${item.colorProfile}`} title={label} aria-label={label}
        style={{ width: size, height: size }} onClick={() => { onSelect(item); commit(item.color, item.alpha); }}><span style={{ background: `rgba(${item.color.join(',')},${item.alpha})` }} /></button>;
    })}
  </div>;
}
