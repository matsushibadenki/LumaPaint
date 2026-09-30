import { useEffect, useState } from 'react';
import { defaultStrokeStyle, strokePreview, type DocumentSnapshot, type StrokeStyle } from '../bridge';
import type { Locale } from '../i18n';
import './stroke-panel.css';

export const strokeLabels = {
  en: { title: 'Stroke', width: 'Weight', preset: 'Stroke presets', preview: 'Stroke preview', select: 'Select visible, unlocked vector paths.', mixed: 'Mixed', selected: 'Selected paths', profile: 'Width profile', error: 'Enter valid stroke settings.', cap: 'Cap', caps: ['Butt', 'Round', 'Square'], join: 'Join', joins: ['Miter', 'Round', 'Bevel'], miter: 'Miter limit', alignment: 'Align stroke', alignments: ['Center', 'Inside', 'Outside'], dash: 'Dash pattern', dashHint: 'Dash / gap lengths, separated by spaces (max. 12). Empty = solid.', offset: 'Dash offset', start: 'Start arrow', end: 'End arrow', arrows: ['None', 'Triangle', 'Open', 'Circle'], scale: 'Arrow scale', profiles: ['Uniform', 'Taper both ends', 'Taper start', 'Taper end', 'Bulge'], note: 'Inside / outside applies to closed paths. Arrows apply to open paths.', solid: 'Solid', dashed: 'Dashed' },
  ja: { title: '線', width: '線幅', preset: '線幅プリセット', preview: '線のプレビュー', select: 'ロックされていない表示中のパスを選択してください。', mixed: '混在', selected: '選択中のパス', profile: '可変幅プロファイル', error: '有効な線の設定値を入力してください。', cap: '線端', caps: ['バット', '丸型', '突出'], join: '角', joins: ['マイター', 'ラウンド', 'ベベル'], miter: 'マイター制限', alignment: '線位置', alignments: ['中央', '内側', '外側'], dash: '破線パターン', dashHint: '線・間隔を空白で区切って入力（最大12個）。空欄は実線。', offset: '破線の開始位置', start: '始点の矢印', end: '終点の矢印', arrows: ['なし', '三角', '開いた矢印', '丸'], scale: '矢印の倍率', profiles: ['均等', '両端を細く', '始点を細く', '終点を細く', '中央を太く'], note: '内側・外側は閉じたパス、矢印は開いたパスに適用されます。', solid: '実線', dashed: '破線' },
  'zh-CN': { title: '描边', width: '粗细', preset: '描边预设', preview: '描边预览', select: '请选择未锁定的可见路径。', mixed: '混合', selected: '所选路径', profile: '可变宽度', error: '请输入有效的描边设置。', cap: '端点', caps: ['平头', '圆头', '方头'], join: '拐角', joins: ['尖角', '圆角', '斜角'], miter: '尖角限制', alignment: '描边位置', alignments: ['居中', '内部', '外部'], dash: '虚线图案', dashHint: '用空格分隔线段和间隔长度（最多12个）。留空为实线。', offset: '虚线偏移', start: '起点箭头', end: '终点箭头', arrows: ['无', '三角', '开放箭头', '圆形'], scale: '箭头比例', profiles: ['均匀', '两端渐细', '起点渐细', '终点渐细', '中间加粗'], note: '内部和外部适用于闭合路径，箭头适用于开放路径。', solid: '实线', dashed: '虚线' },
};
const caps = ['butt', 'round', 'square'] as const;
const joins = ['miter', 'round', 'bevel'] as const;
const alignments = ['center', 'inside', 'outside'] as const;
const arrows = ['none', 'triangle', 'open', 'circle'] as const;
const profiles = ['uniform', 'taperBoth', 'taperStart', 'taperEnd', 'bulge'] as const;

function NumericSetting({ label, value, mixed, min, max, step = .25, onCommit }: { label: string; value: number; mixed: string | null; min: number; max: number; step?: number; onCommit: (value: number) => void }) {
  const [draft, setDraft] = useState('');
  useEffect(() => setDraft(mixed ? '' : String(Number(value.toFixed(4)))), [value, mixed]);
  const commit = () => { if (draft.trim()) onCommit(Number(draft)); };
  return <label className="stroke-setting"><span>{label}</span><input type="number" min={min} max={max} step={step} value={draft} placeholder={mixed ?? ''} onChange={e => setDraft(e.target.value)} onBlur={commit} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); e.currentTarget.blur(); } if (e.key === 'Escape') setDraft(mixed ? '' : String(value)); }} /></label>;
}

export function StrokePanel({ locale, document, enabled, onChange, onStyle }: { locale: Locale; document: DocumentSnapshot; enabled: boolean; onChange: (width: number) => Promise<void>; onStyle: (patch: Partial<StrokeStyle>) => Promise<void> }) {
  const t = strokeLabels[locale];
  const [unit, setUnit] = useState<'pt' | 'px'>('pt');
  const [draft, setDraft] = useState('');
  const [dashDraft, setDashDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [preview, setPreview] = useState('');
  const ids = new Set(document.selectedVectorObjects);
  const selected = document.layers.flatMap(layer => layer.objects.filter(object => ids.has(object.id)).map(object => ({ layer, object })));
  const editable = enabled && selected.length > 0 && selected.every(({ layer, object }) => layer.kind === 'vector' && !layer.locked && layer.visible && object.visible && object.kind !== 'text');
  const styles = selected.map(({ object }) => object.strokeStyle ?? defaultStrokeStyle);
  const style = styles[0] ?? defaultStrokeStyle;
  const isMixed = (key: keyof StrokeStyle) => styles.some(next => JSON.stringify(next[key]) !== JSON.stringify(style[key]));
  const widths = selected.map(({ object }) => object.strokeWidth);
  const mixed = widths.some(width => width !== widths[0]);
  const pixelsPerUnit = unit === 'pt' ? document.resolution / 72 : 1;
  const value = widths[0] ?? 0;
  const display = mixed || !selected.length ? '' : String(Number((value / pixelsPerUnit).toFixed(4)));
  const selectionKey = document.selectedVectorObjects.join(',');
  const dashDisplay = isMixed('dashArray') ? '' : style.dashArray.map(n => Number((n / pixelsPerUnit).toFixed(4))).join(' ');
  useEffect(() => { setDraft(display); setError(''); }, [display, selectionKey, unit]);
  useEffect(() => setDashDraft(dashDisplay), [dashDisplay, selectionKey, unit]);
  const previewKey = JSON.stringify(style);
  useEffect(() => {
    let current = true;
    void strokePreview(value, JSON.parse(previewKey) as StrokeStyle).then(svg => { if (current) setPreview(svg); }).catch(() => { if (current) setPreview(''); });
    return () => { current = false; };
  }, [previewKey, value]);
  const apply = async (action: () => Promise<void>) => {
    if (!editable || busy) return;
    setBusy(true); setError('');
    try { await action(); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };
  const change = (patch: Partial<StrokeStyle>) => {
    const invalid = Object.entries(patch).some(([key, v]) => typeof v === 'number' && (!Number.isFinite(v) ||
      (key === 'miterLimit' && (v < 1 || v > 100)) || (key === 'arrowScale' && (v < .1 || v > 10)) || (key === 'dashOffset' && Math.abs(v) > 100000)));
    if (invalid) { setError(t.error); return; }
    void apply(() => onStyle(patch));
  };
  const commit = (text = draft) => {
    if (!text.trim()) return;
    const width = Number(text) * pixelsPerUnit;
    if (!Number.isFinite(width) || width < 0 || width > 4096) { setError(t.error); return; }
    if (!mixed && Math.abs(width - value) < .0001) return;
    void apply(() => onChange(width));
  };
  const commitDash = () => {
    if (!dashDraft.trim() && isMixed('dashArray')) return;
    const array = dashDraft.trim() ? dashDraft.trim().split(/[\s,]+/).map(n => Number(n) * pixelsPerUnit) : [];
    if (array.length > 12 || array.some(n => !Number.isFinite(n) || n < .1 || n > 100000)) { setError(t.error); return; }
    if (!isMixed('dashArray') && JSON.stringify(array) === JSON.stringify(style.dashArray)) return;
    change({ dashArray: array });
  };
  const choice = <K extends keyof StrokeStyle>(key: K, label: string, values: readonly StrokeStyle[K][], labels: string[]) =>
    <label className="stroke-setting"><span>{label}</span><select value={isMixed(key) ? '' : String(style[key])} onChange={e => change({ [key]: e.target.value } as Partial<StrokeStyle>)}>
      {isMixed(key) && <option value="" disabled>{t.mixed}</option>}{values.map((v, i) => <option key={String(v)} value={String(v)}>{labels[i]}</option>)}
    </select></label>;
  return <div className="stroke-panel">
    <h2>{t.title}</h2>
    <fieldset disabled={!editable || busy}>
      <div className="stroke-width-row">
        <label htmlFor="vector-stroke-width">{t.width}</label>
        <input id="vector-stroke-width" type="number" min="0" max={4096 / pixelsPerUnit} step="0.25" value={draft} placeholder={mixed ? t.mixed : '—'} onChange={e => setDraft(e.target.value)} onBlur={() => commit()} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); e.currentTarget.blur(); } if (e.key === 'Escape') { setDraft(display); setError(''); } }} />
        <select aria-label={`${t.width} pt / px`} value={unit} onChange={e => setUnit(e.target.value as 'pt' | 'px')}><option>pt</option><option>px</option></select>
        <select className="stroke-presets" aria-label={t.preset} value="" onChange={e => { const next = e.target.value; setDraft(next); commit(next); }}><option value="">▾</option>{[0, .25, .5, .75, 1, 2, 3, 4, 6, 8, 12, 16, 24, 48].map(width => <option key={width} value={width}>{width} {unit}</option>)}</select>
      </div>
      <div className="stroke-settings">
        {choice('cap', t.cap, caps, t.caps)}
        {choice('join', t.join, joins, t.joins)}
        <NumericSetting label={t.miter} value={style.miterLimit} mixed={isMixed('miterLimit') ? t.mixed : null} min={1} max={100} step={1} onCommit={n => change({ miterLimit: n })} />
        {choice('alignment', t.alignment, alignments, t.alignments)}
      </div>
      <div className="stroke-dash-heading"><span>{t.dash} ({unit})</span><button type="button" onClick={() => change({ dashArray: [] })}>{t.solid}</button><button type="button" onClick={() => change({ dashArray: [8 * pixelsPerUnit, 4 * pixelsPerUnit] })}>{t.dashed}</button></div>
      <input className="stroke-dash-input" aria-label={`${t.dash} (${unit})`} aria-describedby="stroke-dash-hint" value={dashDraft} placeholder={isMixed('dashArray') ? t.mixed : '8 4'} onChange={e => setDashDraft(e.target.value)} onBlur={commitDash} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); e.currentTarget.blur(); } if (e.key === 'Escape') setDashDraft(dashDisplay); }} />
      <p id="stroke-dash-hint" className="stroke-hint">{t.dashHint}</p>
      <div className="stroke-settings">
        <NumericSetting label={`${t.offset} (${unit})`} value={style.dashOffset / pixelsPerUnit} mixed={isMixed('dashOffset') ? t.mixed : null} min={-100000 / pixelsPerUnit} max={100000 / pixelsPerUnit} onCommit={n => change({ dashOffset: n * pixelsPerUnit })} />
        {choice('startArrow', t.start, arrows, t.arrows)}
        {choice('endArrow', t.end, arrows, t.arrows)}
        <NumericSetting label={`${t.scale} (×)`} value={style.arrowScale} mixed={isMixed('arrowScale') ? t.mixed : null} min={.1} max={10} step={.1} onCommit={n => change({ arrowScale: n })} />
        {choice('profile', t.profile, profiles, t.profiles)}
      </div>
    </fieldset>
    <div className="stroke-preview" role="img" aria-label={t.preview} dangerouslySetInnerHTML={{ __html: preview }} />
    {selected.length > 1 && (mixed || styles.some(s => JSON.stringify(s) !== previewKey)) && <p className="stroke-hint">{t.mixed}</p>}
    <p className="stroke-note">{t.note}</p>
    <p className="stroke-note">{editable ? `${t.selected}：${selected.length}` : t.select}</p>
    {error && <p role="alert" className="stroke-error">{error}</p>}
  </div>;
}
