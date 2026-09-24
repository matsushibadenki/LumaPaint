import { useEffect, useState } from 'react';
import type { DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import './stroke-panel.css';

export const strokeLabels = {
  en: { title: 'Stroke', width: 'Weight', preset: 'Stroke presets', preview: 'Stroke preview', select: 'Select a vector path to edit its stroke.', mixed: 'Mixed', selected: 'Selected paths', profile: 'Profile', uniform: 'Uniform', error: 'Enter a valid width (0–4096 px).' },
  ja: { title: '線', width: '線幅', preset: '線幅プリセット', preview: '線のプレビュー', select: 'ベクターパスを選択して線幅を変更します。', mixed: '混在', selected: '選択中のパス', profile: 'プロファイル', uniform: '均等', error: '有効な線幅を入力してください（0〜4096 px）。' },
  'zh-CN': { title: '描边', width: '粗细', preset: '描边预设', preview: '描边预览', select: '选择矢量路径以修改描边。', mixed: '混合', selected: '所选路径', profile: '配置文件', uniform: '均匀', error: '请输入有效的宽度（0–4096 px）。' },
};

export function StrokePanel({ locale, document, enabled, onChange }: { locale: Locale; document: DocumentSnapshot; enabled: boolean; onChange: (width: number) => Promise<void> }) {
  const t = strokeLabels[locale];
  const [unit, setUnit] = useState<'pt' | 'px'>('pt');
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const selected = document.layers.flatMap(layer => layer.objects.filter(object => document.selectedVectorObjects.includes(object.id)).map(object => ({ layer, object })));
  const editable = enabled && selected.length > 0 && selected.every(({ layer, object }) => layer.kind === 'vector' && !layer.locked && layer.visible && object.visible && object.kind !== 'text');
  const widths = selected.map(({ object }) => object.strokeWidth);
  const mixed = widths.some(width => width !== widths[0]);
  const pixelsPerUnit = unit === 'pt' ? document.resolution / 72 : 1;
  const value = widths[0] ?? 0;
  const display = mixed || !selected.length ? '' : String(Number((value / pixelsPerUnit).toFixed(4)));
  const selectionKey = document.selectedVectorObjects.join(',');
  useEffect(() => { setDraft(display); setError(''); }, [display, selectionKey, unit]);
  const commit = async (text = draft) => {
    if (!editable || busy || text.trim() === '') return;
    const width = Number(text) * pixelsPerUnit;
    if (!Number.isFinite(width) || width < 0 || width > 4096) { setError(t.error); return; }
    if (!mixed && Math.abs(width - value) < 0.0001) return;
    setBusy(true); setError('');
    try { await onChange(width); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };
  return <div className="stroke-panel">
    <h2>{t.title}</h2>
    <fieldset disabled={!editable || busy}>
      <div className="stroke-width-row">
        <label htmlFor="vector-stroke-width">{t.width}：</label>
        <input id="vector-stroke-width" type="number" min="0" max={4096 / pixelsPerUnit} step="0.25" value={draft} placeholder={mixed ? t.mixed : '—'} onChange={event => setDraft(event.target.value)} onBlur={() => void commit()} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); void commit(); } if (event.key === 'Escape') { setDraft(display); setError(''); } }} />
        <select aria-label={t.width} value={unit} onChange={event => setUnit(event.target.value as 'pt' | 'px')}><option>pt</option><option>px</option></select>
        <select className="stroke-presets" aria-label={t.preset} value="" onChange={event => { const next = event.target.value; setDraft(next); void commit(next); }}><option value="">▾</option>{[0, 0.25, 0.5, 0.75, 1, 2, 3, 4, 6, 8, 12, 16, 24, 48].map(width => <option key={width} value={width}>{width} {unit}</option>)}</select>
      </div>
    </fieldset>
    <div className="stroke-preview" aria-label={t.preview}>
      <svg viewBox="0 0 240 72" role="img" aria-label={mixed ? t.mixed : `${draft || 0} ${unit}`}><path d="M18 48 C70 48 60 24 120 24 S174 48 222 24" fill="none" stroke="currentColor" strokeWidth={mixed ? 1 : Math.min(36, value)} /></svg>
    </div>
    <div className="stroke-profile"><span>{t.profile}：</span><svg viewBox="0 0 84 18" aria-hidden="true"><path d="M4 9H80" stroke="currentColor" strokeWidth="4" /></svg><span>{t.uniform}</span></div>
    <p className="stroke-note">{editable ? `${t.selected}：${selected.length}` : t.select}</p>
    {error && <p role="alert" className="stroke-error">{error}</p>}
  </div>;
}
