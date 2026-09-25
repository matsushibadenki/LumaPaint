import { useState } from 'react';
import type { Brush } from '../bridge';
import { readPreference, type Locale } from '../i18n';
import { fromHex, toHex } from './BrushControls';

const labels = {
  ja: { title: 'スウォッチ', add: '現在の色を登録', remove: '選択色を削除', empty: '登録したい色を選び、＋を押してください。', full: '登録できる色は128色までです。', failed: '保存できませんでした。', target: '適用先' },
  en: { title: 'Swatches', add: 'Save current color', remove: 'Delete selected swatch', empty: 'Choose a color, then press + to save it.', full: 'Up to 128 colors can be saved.', failed: 'Could not save swatches.', target: 'Apply to' },
  'zh-CN': { title: '色板', add: '保存当前颜色', remove: '删除所选色样', empty: '选择颜色后，按＋保存。', full: '最多可保存128种颜色。', failed: '无法保存色板。', target: '应用于' },
};

function loadSwatches(): string[] {
  try {
    const stored: unknown = JSON.parse(readPreference('color-swatches-v1') ?? '[]');
    if (!Array.isArray(stored)) return [];
    return [...new Set(stored.filter((value): value is string => typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value)).map(value => value.toLowerCase()))].slice(0, 128);
  } catch { return []; }
}

export function ColorSwatches({ locale, color, targetLabel, onChange }: {
  locale: Locale; color: Brush['color']; targetLabel: string; onChange: (color: Brush['color']) => void;
}) {
  const t = labels[locale];
  const [swatches, setSwatches] = useState(loadSwatches);
  const [error, setError] = useState(false);
  const hex = toHex(color).toLowerCase();
  const registered = swatches.includes(hex);
  function save(next: string[]) {
    try {
      localStorage.setItem('lumapaint.color-swatches-v1', JSON.stringify(next));
      setSwatches(next); setError(false);
    } catch { setError(true); }
  }
  return <section className="saved-swatches" aria-label={t.title}>
    <div className="saved-swatches-heading">
      <strong>{t.title}</strong>
      <span>{swatches.length}/128</span>
      <button type="button" title={t.add} aria-label={t.add} disabled={registered || swatches.length >= 128} onClick={() => save([...swatches, hex])}>＋</button>
      <button type="button" title={t.remove} aria-label={t.remove} disabled={!registered} onClick={() => save(swatches.filter(value => value !== hex))}>−</button>
    </div>
    <p className="muted small">{t.target}: {targetLabel}</p>
    {swatches.length === 0 ? <p className="muted small">{t.empty}</p> : <div className="saved-swatches-grid" role="group" aria-label={t.title}>
      {swatches.map(value => <button type="button" key={value} style={{ backgroundColor: value }} title={value.toUpperCase()}
        aria-label={`${t.title} ${value.toUpperCase()}`} aria-pressed={hex === value} onClick={() => onChange(fromHex(value))} />)}
    </div>}
    {swatches.length >= 128 && <p className="muted small">{t.full}</p>}
    {error && <p role="alert">{t.failed}</p>}
  </section>;
}
