import { useEffect, useRef, useState, type PointerEvent } from 'react';
import type { Brush } from '../bridge';
import type { Locale } from '../i18n';
import { HexInput, toHex } from './BrushControls';
import { cmykToRgb, rgbToCmyk, type CmykColor } from '../color-models';
import { workspaceMessages } from '../workspace-i18n';
import { ColorSwatches } from './ColorSwatches';

export const colorPanelLabels = {
  ja: { color: 'カラー', hue: '色相', saturation: '彩度', value: '明度', model: 'カラー方式', red: '赤', green: '緑', blue: '青', cyan: 'シアン', magenta: 'マゼンタ', yellow: 'イエロー', black: 'ブラック', cmykNote: 'CMYKはRGB換算値です（ICC変換なし）。' },
  en: { color: 'Color', hue: 'Hue', saturation: 'Saturation', value: 'Brightness', model: 'Color model', red: 'Red', green: 'Green', blue: 'Blue', cyan: 'Cyan', magenta: 'Magenta', yellow: 'Yellow', black: 'Black', cmykNote: 'CMYK uses RGB conversion (no ICC transform).' },
  'zh-CN': { color: '颜色', hue: '色相', saturation: '饱和度', value: '明度', model: '颜色模型', red: '红', green: '绿', blue: '蓝', cyan: '青色', magenta: '品红', yellow: '黄色', black: '黑色', cmykNote: 'CMYK为RGB换算值（无ICC转换）。' },
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

type ColorModel = 'hsb' | 'rgb' | 'cmyk';
type ChannelKey = 'hue' | 'saturation' | 'value' | 'red' | 'green' | 'blue' | 'cyan' | 'magenta' | 'yellow' | 'black';
type ColorChannel = { key: ChannelKey; short: string; value: number; max: number; unit: string; background: string };

function ChannelNumber({ value, max, label, onChange }: { value: number; max: number; label: string; onChange: (value: number) => void }) {
  const rounded = Math.round(value);
  const [draft, setDraft] = useState(String(rounded));
  useEffect(() => setDraft(String(rounded)), [rounded]);
  return <input className="hsb-number" aria-label={label} type="number" min="0" max={max} step="1" value={draft}
    onChange={event => {
      setDraft(event.target.value);
      const next = event.target.valueAsNumber;
      if (Number.isFinite(next) && next >= 0 && next <= max) onChange(next);
    }}
    onBlur={() => {
      const next = draft.trim() === '' ? rounded : Number(draft);
      const clamped = Number.isFinite(next) ? Math.max(0, Math.min(max, Math.round(next))) : rounded;
      setDraft(String(clamped)); onChange(clamped);
    }}
    onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} />;
}

export function ColorPanel({ locale, color: foregroundColor, backgroundColor, activeColor, onSelectColor, onChange: onForegroundChange, onBackgroundChange, onSwap }: { locale: Locale; color: Brush['color']; backgroundColor: Brush['color']; activeColor: ColorTarget; onSelectColor: (target: ColorTarget) => void; onChange: (color: Brush['color']) => void; onBackgroundChange: (color: Brush['color']) => void; onSwap: () => void }) {
  const t = colorPanelLabels[locale], w = workspaceMessages[locale];
  const [colorModel, setColorModel] = useState<ColorModel>('hsb');
  const [cmykEdits, setCmykEdits] = useState<Partial<Record<ColorTarget, { hex: string; values: CmykColor }>>>({});
  const [rememberedHue, setRememberedHue] = useState(0);
  const color = activeColor === 'background' ? backgroundColor : foregroundColor;
  const onChange = activeColor === 'background' ? onBackgroundChange : onForegroundChange;
  const savedCmyk = cmykEdits[activeColor];
  // Preserve the chosen black/ink separation: RGB alone cannot reconstruct it.
  const cmyk = savedCmyk?.hex === toHex(color) ? savedCmyk.values : rgbToCmyk(color);
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
  const hsbChannels: ColorChannel[] = [
    { key: 'hue', short: 'H', value: h, max: 359, unit: '°', background: 'linear-gradient(to right, red, yellow, lime, cyan, blue, magenta, red)' },
    { key: 'saturation', short: 'S', value: current.s * 100, max: 100, unit: '%', background: 'linear-gradient(to right, #999, ' + pure + ')' },
    { key: 'value', short: 'B', value: current.v * 100, max: 100, unit: '%', background: 'linear-gradient(to right, #000, ' + pure + ')' },
  ];
  const rgbKeys = ['red', 'green', 'blue'] as const;
  const cmykKeys = ['cyan', 'magenta', 'yellow', 'black'] as const;
  const gradient = (start: Brush['color'], end: Brush['color']) => `linear-gradient(to right, ${toHex(start)}, ${toHex(end)})`;
  const channels: ColorChannel[] = colorModel === 'hsb' ? hsbChannels : colorModel === 'rgb'
    ? rgbKeys.map((key, index) => {
      const start: Brush['color'] = [...color], end: Brush['color'] = [...color];
      start[index] = 0; end[index] = 255;
      return { key, short: ['R', 'G', 'B'][index], value: color[index], max: 255, unit: '', background: gradient(start, end) };
    })
    : cmykKeys.map((key, index) => {
      const start: CmykColor = [...cmyk], end: CmykColor = [...cmyk];
      start[index] = 0; end[index] = 100;
      return { key, short: ['C', 'M', 'Y', 'K'][index], value: cmyk[index], max: 100, unit: '%', background: gradient(cmykToRgb(start), cmykToRgb(end)) };
    });
  function setChannel(key: ChannelKey, n: number) {
    if (!Number.isFinite(n)) return;
    const rgbIndex = rgbKeys.findIndex(channel => channel === key);
    if (rgbIndex >= 0) {
      const next: Brush['color'] = [...color];
      next[rgbIndex] = Math.round(Math.max(0, Math.min(255, n)));
      onChange(next); return;
    }
    const cmykIndex = cmykKeys.findIndex(channel => channel === key);
    if (cmykIndex >= 0) {
      const values: CmykColor = [...cmyk];
      values[cmykIndex] = Math.max(0, Math.min(100, n));
      const next = cmykToRgb(values);
      setCmykEdits(previous => ({ ...previous, [activeColor]: { hex: toHex(next), values } }));
      onChange(next); return;
    }
    n = Math.max(0, Math.min(key === 'hue' ? 359 : 100, n));
    if (key === 'hue') changeHue(n);
    else onChange(rgb(h, key === 'saturation' ? n / 100 : current.s, key === 'value' ? n / 100 : current.v));
  }
  return <div className="color-panel-controls">
    <label className="color-model-select"><span>{t.model}</span><select value={colorModel} onChange={event => setColorModel(event.target.value as ColorModel)}>
      <option value="hsb">HSB</option><option value="rgb">RGB</option><option value="cmyk">CMYK</option>
    </select></label>
    <div className="hsb-header">
      <ColorPairControl locale={locale} foreground={foregroundColor} background={backgroundColor} activeColor={activeColor} onSelectColor={onSelectColor} onSwap={onSwap} />
      <div className="hsb-sliders">{channels.map(item => <div className="color-slider" key={activeColor + item.key}>
        <span title={t[item.key]}>{item.short}</span><input aria-label={t[item.key]} type="range" min="0" max={item.max} value={Math.round(item.value)} style={{ background: item.background }} onChange={event => setChannel(item.key, Number(event.target.value))} />
        <ChannelNumber label={t[item.key] + ' (' + item.short + ')'} max={item.max} value={item.value} onChange={next => setChannel(item.key, next)} /><span>{item.unit}</span>
      </div>)}</div>
    </div>
    {colorModel === 'cmyk' && <p className="color-model-note">{t.cmykNote}</p>}
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
    <ColorSwatches locale={locale} color={color} targetLabel={w[activeColor]} onChange={onChange} />
  </div>;
}
