import { useEffect, useRef, useState, type PointerEvent } from 'react';
import type { Brush } from '../bridge';
import type { Locale } from '../i18n';
import { HexInput, toHex } from './BrushControls';
import { workspaceMessages } from '../workspace-i18n';

export const colorPanelLabels = {
  ja: { color: 'カラー', hue: '色相', saturation: '彩度', value: '明度' },
  en: { color: 'Color', hue: 'Hue', saturation: 'Saturation', value: 'Brightness' },
  'zh-CN': { color: '颜色', hue: '色相', saturation: '饱和度', value: '明度' },
};
function hsv(rgb: Brush['color']) {
  const [r, g, b] = rgb.map(v => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  const h = d === 0 ? 0 : max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return { h: h * 60, s: max === 0 ? 0 : d / max, v: max };
}
function rgb(h: number, s: number, v: number): Brush['color'] {
  const c = v * s, x = c * (1 - Math.abs((h / 60) % 2 - 1)), m = v - c;
  const channels = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return channels.map(n => Math.round((n + m) * 255)) as Brush['color'];
}
export type ColorTarget = 'foreground' | 'background';

export function ColorPairControl({ locale, foreground, background, compact = false, activeColor, onSelectColor, onSwap }: {
  locale: Locale; foreground: Brush['color']; background: Brush['color']; compact?: boolean;
  activeColor: ColorTarget; onSelectColor: (target: ColorTarget) => void; onSwap: () => void;
}) {
  const t = workspaceMessages[locale];
  return <div className={`color-pair${compact ? ' compact' : ''}`} role="group" aria-label={`${t.foreground} · ${t.background}`}>
    <button type="button" className="background-color" style={{ background: toHex(background) }} aria-label={t.background} aria-pressed={activeColor === 'background'} title={`${t.background}: ${toHex(background)}`} onClick={() => onSelectColor('background')} />
    <button type="button" className="foreground-color" style={{ background: toHex(foreground) }} aria-label={t.foreground} aria-pressed={activeColor === 'foreground'} title={`${t.foreground}: ${toHex(foreground)}`} onClick={() => onSelectColor('foreground')} />
    <button type="button" className="swap-colors" onClick={onSwap} title={`${t.swapColors} (X)`} aria-label={t.swapColors}>↔</button>
  </div>;
}

export function ColorPanel({ locale, color: foregroundColor, backgroundColor, activeColor, onSelectColor, onChange: onForegroundChange, onBackgroundChange, onSwap }: { locale: Locale; color: Brush['color']; backgroundColor: Brush['color']; activeColor: ColorTarget; onSelectColor: (target: ColorTarget) => void; onChange: (color: Brush['color']) => void; onBackgroundChange: (color: Brush['color']) => void; onSwap: () => void }) {
  const t = colorPanelLabels[locale], w = workspaceMessages[locale];
  const [rememberedHue, setRememberedHue] = useState(0);
  const color = activeColor === 'background' ? backgroundColor : foregroundColor;
  const onChange = activeColor === 'background' ? onBackgroundChange : onForegroundChange;
  const current = hsv(color);
  const h = current.s === 0 ? rememberedHue : current.h;
  const canvas = useRef<HTMLCanvasElement>(null);
  const mode = useRef<'hue' | 'triangle' | null>(null);
  const pure = toHex(rgb(h, 1, 1));
  useEffect(() => {
    const ctx = canvas.current?.getContext('2d');
    if (!ctx) return;
    const size = 480, data = ctx.createImageData(size, size);
    const hueColor = rgb(h, 1, 1);
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const px = (x + .5) / size * 100, py = (y + .5) / size * 100;
      const c = (px - 29) / 62;
      const white = (1 - c - (py - 50) / 36) / 2;
      const black = 1 - c - white;
      if (Math.min(c, white, black) < 0) continue;
      const offset = (y * size + x) * 4;
      for (let i = 0; i < 3; i++) data.data[offset + i] = Math.round(hueColor[i] * c + 255 * white);
      data.data[offset + 3] = 255;
    }
    ctx.putImageData(data, 0, 0);
  }, [h]);
  function changeHue(next: number) { setRememberedHue(next); onChange(rgb(next, current.s, current.v)); }
  function pick(event: PointerEvent<HTMLDivElement>, start = false) {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = (event.clientX - rect.left) / rect.width * 100;
    const y = (event.clientY - rect.top) / rect.height * 100;
    if (start) {
      const radius = Math.hypot(x - 50, y - 50);
      mode.current = radius >= 42 ? 'hue' : x >= 29 && x <= 91 && Math.abs(y - 50) <= 36 * (1 - (x - 29) / 62) ? 'triangle' : null;
    }
    if (mode.current === 'hue') changeHue((Math.atan2(50 - y, x - 50) * 180 / Math.PI + 360) % 360);
    if (mode.current === 'triangle') {
      const c = Math.max(0, Math.min(1, (x - 29) / 62));
      const white = Math.max(0, Math.min(1 - c, (1 - c - (y - 50) / 36) / 2));
      const v = c + white;
      onChange(rgb(h, v === 0 ? 0 : c / v, v));
    }
  }
  const c = current.s * current.v, white = current.v - c;
  const channels = [
    { key: 'hue', short: 'H', value: h, max: 359, background: 'linear-gradient(to right, red, yellow, lime, cyan, blue, magenta, red)' },
    { key: 'saturation', short: 'S', value: current.s * 100, max: 100, background: 'linear-gradient(to right, #999, ' + pure + ')' },
    { key: 'value', short: 'B', value: current.v * 100, max: 100, background: 'linear-gradient(to right, #000, ' + pure + ')' },
  ] as const;
  function setChannel(key: 'hue' | 'saturation' | 'value', n: number) {
    if (!Number.isFinite(n)) return;
    n = Math.max(0, Math.min(key === 'hue' ? 359 : 100, n));
    if (key === 'hue') changeHue(n);
    else onChange(rgb(h, key === 'saturation' ? n / 100 : current.s, key === 'value' ? n / 100 : current.v));
  }
  return <div className="color-panel-controls">
    <div className="hsb-header">
      <ColorPairControl locale={locale} foreground={foregroundColor} background={backgroundColor} activeColor={activeColor} onSelectColor={onSelectColor} onSwap={onSwap} />
      <div className="hsb-sliders">{channels.map(item => <div className="color-slider" key={item.key}>
        <span title={t[item.key]}>{item.short}</span><input aria-label={t[item.key]} type="range" min="0" max={item.max} value={Math.round(item.value)} style={{ background: item.background }} onChange={event => setChannel(item.key, Number(event.target.value))} />
        <input className="hsb-number" aria-label={t[item.key] + ' (' + item.short + ')'} type="number" min="0" max={item.max} value={Math.round(item.value)} onChange={event => setChannel(item.key, Number(event.target.value))} /><span>{item.key === 'hue' ? '°' : '%'}</span>
      </div>)}</div>
    </div>
    <div className="color-wheel" role="group" aria-label={t.color}
      onPointerDown={event => { if (event.button !== 0) return; event.currentTarget.setPointerCapture(event.pointerId); pick(event, true); }}
      onPointerMove={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) pick(event); }}
      onPointerUp={event => { mode.current = null; if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); }}
      onPointerCancel={() => { mode.current = null; }}>
      <div className="color-wheel-center" />
      <canvas ref={canvas} width="480" height="480" className="color-triangle" aria-hidden="true" />
      <span className="color-wheel-marker" style={{ left: (50 + 46 * Math.cos(h * Math.PI / 180)) + '%', top: (50 - 46 * Math.sin(h * Math.PI / 180)) + '%' }} />
      <span className="color-wheel-marker" style={{ left: (29 + 62 * c) + '%', top: (50 + 36 * (1 - c - 2 * white)) + '%' }} />
    </div>
    <HexInput key={activeColor} color={color} onChange={onChange} label={`${w[activeColor]} · ${w.hex}`} invalid={w.invalidColor} />
  </div>;
}
