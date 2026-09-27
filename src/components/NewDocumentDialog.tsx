import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { BitDepth, ColorProfile, DocumentUnit, NewDocumentSettings } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { paperPresets, presetSettings, unitFactor, type PresetCategory } from '../document-presets';
import './new-document.css';

const labels = {
  en: { title: 'New document', recent: 'Recent', saved: 'Saved', photo: 'Photo', print: 'Print', art: 'Art & illustration', web: 'Web', mobile: 'Mobile', video: 'Film & video', details: 'Preset details', create: 'Create', cancel: 'Cancel', save: 'Save preset', remove: 'Remove preset', empty: 'No presets yet.', invalid: 'Use a name, dimensions of 1–8192 pixels and a resolution of 1–1200 ppi.', failed: 'Could not create document.', storage: 'Could not save presets on this device.', untitled: 'Untitled', size: 'Document size', Postcard: 'Postcard', Poster: 'Poster' },
  ja: { title: '新規ドキュメント', recent: '最近使用したもの', saved: '保存済み', photo: '写真', print: '印刷', art: 'アートとイラスト', web: 'Web', mobile: 'モバイル', video: 'フィルムとビデオ', details: 'プリセットの詳細', create: '作成', cancel: 'キャンセル', save: 'プリセットを保存', remove: 'プリセットを削除', empty: 'プリセットはまだありません。', invalid: '名称、1～8192ピクセルの寸法、1～1200 ppiの解像度を指定してください。', failed: 'ドキュメントを作成できませんでした。', storage: 'この端末にプリセットを保存できませんでした。', untitled: '名称未設定', size: 'ドキュメントサイズ', Postcard: 'ポストカード', Poster: 'ポスター' },
  'zh-CN': { title: '新建文档', recent: '最近使用', saved: '已保存', photo: '照片', print: '打印', art: '艺术与插图', web: 'Web', mobile: '移动设备', video: '电影与视频', details: '预设详细信息', create: '创建', cancel: '取消', save: '保存预设', remove: '删除预设', empty: '暂无预设。', invalid: '请指定名称、1–8192像素的尺寸及1–1200 ppi的分辨率。', failed: '无法创建文档。', storage: '无法在此设备上保存预设。', untitled: '未命名', size: '文档尺寸', Postcard: '明信片', Poster: '海报' },
};
type Tab = PresetCategory | 'recent' | 'saved';
type Stored = { id: string; settings: NewDocumentSettings };
const storageKey = (kind: string) => `lumapaint.document-presets.${kind}.v1`;
function readStored(kind: string): Stored[] {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(storageKey(kind)) ?? '[]');
    if (!Array.isArray(data)) return [];
    return data.filter((item): item is Stored => {
      const s = item?.settings; const d = s?.document;
      return typeof item?.id === 'string' && typeof d?.name === 'string' && d.name.length <= 120 && Number.isInteger(d.width) && d.width > 0 && d.width <= 8192 && Number.isInteger(d.height) && d.height > 0 && d.height <= 8192 && Number.isInteger(d.resolution) && d.resolution > 0 && d.resolution <= 1200 && ['pixels', 'inches', 'centimeters', 'millimeters'].includes(d.unit) && ['white', 'transparent'].includes(d.canvasColor) && typeof d.artboards === 'boolean' && Number.isFinite(d.pixelAspectRatio) && d.pixelAspectRatio >= 0.1 && d.pixelAspectRatio <= 10 && [8, 16, 32].includes(s.bitDepth) && (s.colorMode === 'rgb' ? ['srgb', 'displayP3', 'adobeRgb1998'].includes(s.colorProfile) : s.colorMode === 'cmyk' && s.colorProfile === 'japanColor2001Coated');
    }).slice(0, 30);
  } catch { return []; }
}
const shortUnits = { pixels: 'px', inches: 'in', centimeters: 'cm', millimeters: 'mm' };
function PaperIcon({ width, height, category }: { width: number; height: number; category: Tab }) {
  const w = 68 * Math.min(1, width / height); const h = 68 * Math.min(1, height / width);
  const x = (90 - w) / 2; const y = (90 - h) / 2;
  return <svg viewBox="0 0 90 90" aria-hidden="true"><rect x={x} y={y} width={w} height={h} rx={category === 'mobile' ? 5 : 1} />{category === 'photo' ? <path d={`M${x + 5} ${y + h - 5} l${w * .3} ${-h * .3} l${w * .25} ${h * .25} m${-w * .08} ${-h * .08} l${w * .15} ${-h * .2} l${w * .2} ${h * .3}`} /> : category === 'video' ? <path d="M40 35 L55 45 L40 55 Z" /> : category === 'web' ? <path d={`M${x} ${y + 8} h${w}`} /> : <path d={`M${x + 6} ${y + 9} h${Math.max(0, w - 12)}`} />}</svg>;
}

export function NewDocumentDialog({ locale, onCreate, onClose }: { locale: Locale; onCreate: (settings: NewDocumentSettings) => Promise<void>; onClose: () => void }) {
  const t = labels[locale]; const w = workspaceMessages[locale];
  const dialog = useRef<HTMLDialogElement>(null);
  const [tab, setTab] = useState<Tab>('print');
  const [selected, setSelected] = useState(paperPresets[0].id);
  const [settings, setSettings] = useState(() => presetSettings(paperPresets[0], `${t.untitled} 1`));
  const [width, setWidth] = useState('210'); const [height, setHeight] = useState('297');
  const [recent] = useState(() => readStored('recent'));
  const [saved, setSaved] = useState(() => readStored('saved'));
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const submitting = useRef(false);
  const d = settings.document; const factor = unitFactor(d.unit, d.resolution);
  const pixelWidth = Math.round(Number(width) * factor); const pixelHeight = Math.round(Number(height) * factor);
  const valid = d.name.trim().length > 0 && d.name.length <= 120 && Number(width) > 0 && Number(height) > 0 && pixelWidth >= 1 && pixelHeight >= 1 && pixelWidth <= 8192 && pixelHeight <= 8192 && Number.isInteger(d.resolution) && d.resolution >= 1 && d.resolution <= 1200;
  const current = (): NewDocumentSettings => ({ ...settings, document: { ...d, width: pixelWidth, height: pixelHeight } });
  useEffect(() => { const node = dialog.current!; node.showModal(); return () => node.close(); }, []);
  const setDoc = (patch: Partial<typeof d>) => setSettings(value => ({ ...value, document: { ...value.document, ...patch } }));
  function choose(id: string, value: NewDocumentSettings, dimensions?: [number, number]) {
    setSelected(id); setSettings(value); setError('');
    const f = unitFactor(value.document.unit, value.document.resolution);
    setWidth(String(dimensions?.[0] ?? Number((value.document.width / f).toFixed(4))));
    setHeight(String(dimensions?.[1] ?? Number((value.document.height / f).toFixed(4))));
  }
  function store(kind: string, values: Stored[]) {
    try { localStorage.setItem(storageKey(kind), JSON.stringify(values)); return true; }
    catch { setError(t.storage); return false; }
  }
  async function create() {
    if (!valid || submitting.current) return;
    submitting.current = true; setBusy(true); setError('');
    const value = current();
    try {
      await onCreate(value);
      const values = [{ id: crypto.randomUUID(), settings: value }, ...recent.filter(item => JSON.stringify(item.settings) !== JSON.stringify(value))].slice(0, 20);
      store('recent', values);
      onClose();
    } catch (cause) { setError(`${t.failed} ${String(cause)}`); }
    finally { submitting.current = false; setBusy(false); }
  }
  const stored = tab === 'recent' ? recent : saved;
  return createPortal(<dialog ref={dialog} className="new-document-dialog" aria-labelledby="new-document-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    <form onSubmit={event => { event.preventDefault(); void create(); }}>
      <header><h2 id="new-document-title">{t.title}</h2><button type="button" disabled={busy} aria-label={t.cancel} onClick={onClose}>×</button></header>
      <nav aria-label={t.title}>{(['recent', 'saved', 'photo', 'print', 'art', 'web', 'mobile', 'video'] as const).map(category => <button type="button" key={category} disabled={busy} aria-pressed={tab === category} onClick={() => setTab(category)}>{t[category]}</button>)}</nav>
      <div className="new-document-body">
        <section className="paper-preset-grid" aria-label={t[tab]}>
          {tab === 'recent' || tab === 'saved' ? stored.length ? stored.map(item => <button type="button" className="paper-preset" key={item.id} aria-pressed={selected === item.id} disabled={busy} onClick={() => choose(item.id, item.settings)}><PaperIcon width={item.settings.document.width} height={item.settings.document.height} category={tab} /><strong>{item.settings.document.name}</strong><small>{item.settings.document.width} × {item.settings.document.height} px @ {item.settings.document.resolution} ppi</small></button>) : <p className="preset-empty">{t.empty}</p> : paperPresets.filter(p => p.category === tab).map(p => <button type="button" className="paper-preset" key={p.id} aria-pressed={selected === p.id} disabled={busy} onClick={() => choose(p.id, presetSettings(p, d.name), [p.width, p.height])}><PaperIcon width={p.width} height={p.height} category={tab} /><strong>{p.name === 'Postcard' || p.name === 'Poster' ? t[p.name] : p.name}</strong><small>{p.width} × {p.height} {shortUnits[p.unit]} @ {p.resolution} ppi</small></button>)}
        </section>
        <fieldset className="preset-details" disabled={busy}>
          <legend>{t.details}</legend>
          <label>{w.documentName}<input maxLength={120} value={d.name} onChange={e => setDoc({ name: e.target.value })} /></label>
          <div className="preset-two-columns"><label>{w.width}<input type="number" min="0.0001" step="any" required value={width} onChange={e => setWidth(e.target.value)} /></label><label>{w.unit}<select aria-label={w.unit} value={d.unit} onChange={e => { const unit = e.target.value as DocumentUnit; const next = unitFactor(unit, d.resolution); setWidth(String(Number((Number(width) * factor / next).toFixed(4)))); setHeight(String(Number((Number(height) * factor / next).toFixed(4)))); setDoc({ unit }); }}>{(['pixels', 'millimeters', 'centimeters', 'inches'] as const).map(unit => <option key={unit} value={unit}>{w[unit]}</option>)}</select></label></div>
          <div className="preset-two-columns"><label>{w.height}<input type="number" min="0.0001" step="any" required value={height} onChange={e => setHeight(e.target.value)} /></label><div><span>{w.orientation}</span><div className="preset-orientation">{(['portrait', 'landscape'] as const).map(orientation => <button key={orientation} type="button" title={w[orientation]} aria-label={w[orientation]} aria-pressed={orientation === 'portrait' ? Number(height) >= Number(width) : Number(width) > Number(height)} onClick={() => { if ((orientation === 'portrait' && Number(width) > Number(height)) || (orientation === 'landscape' && Number(height) > Number(width))) { setWidth(height); setHeight(width); } }}><svg viewBox="0 0 24 24" aria-hidden="true"><rect x={orientation === 'portrait' ? 6 : 3} y={orientation === 'portrait' ? 3 : 6} width={orientation === 'portrait' ? 12 : 18} height={orientation === 'portrait' ? 18 : 12} /></svg></button>)}</div></div></div>
          <label className="preset-check"><input type="checkbox" checked={d.artboards} onChange={e => setDoc({ artboards: e.target.checked })} />{w.artboards}</label>
          <label>{w.resolution}<div className="preset-two-columns"><input type="number" min="1" max="1200" required value={d.resolution || ''} onChange={e => setDoc({ resolution: Number(e.target.value) })} /><span>{w.pixelsPerInch}</span></div></label>
          <div className="preset-two-columns"><label>{w.colorMode}<select aria-label={w.colorMode} value={settings.colorMode} onChange={e => setSettings(v => ({ ...v, colorMode: e.target.value as 'rgb' | 'cmyk', colorProfile: e.target.value === 'rgb' ? 'srgb' : 'japanColor2001Coated' }))}><option value="rgb">RGB</option><option value="cmyk">CMYK</option></select></label><label>{w.bitDepth}<select aria-label={w.bitDepth} value={settings.bitDepth} onChange={e => setSettings(v => ({ ...v, bitDepth: Number(e.target.value) as BitDepth }))}>{[8, 16, 32].map(n => <option key={n} value={n}>{n} bit</option>)}</select></label></div>
          <label>{w.canvasColor}<select aria-label={w.canvasColor} value={d.canvasColor} onChange={e => setDoc({ canvasColor: e.target.value as 'white' | 'transparent' })}><option value="white">{w.white}</option><option value="transparent">{w.transparent}</option></select></label>
          <details open><summary>{w.advancedOptions}</summary><label>{w.colorProfile}<select aria-label={w.colorProfile} value={settings.colorProfile} onChange={e => setSettings(v => ({ ...v, colorProfile: e.target.value as ColorProfile }))}>{(settings.colorMode === 'rgb' ? [['srgb', 'sRGB IEC61966-2.1'], ['displayP3', 'Display P3'], ['adobeRgb1998', 'Adobe RGB (1998)']] : [['japanColor2001Coated', 'Japan Color 2001 Coated']]).map(([value, text]) => <option key={value} value={value}>{text}</option>)}</select></label><label>{w.pixelAspectRatio}<select aria-label={w.pixelAspectRatio} value={d.pixelAspectRatio} onChange={e => setDoc({ pixelAspectRatio: Number(e.target.value) })}>{[1, .9091, 1.094, 1.2121, 1.3333, 1.4587, 2].map(n => <option key={n} value={n}>{n === 1 ? w.squarePixels : n}</option>)}</select></label></details>
          <p className="preset-pixel-size">{t.size}: {Number.isFinite(pixelWidth) ? pixelWidth : '—'} × {Number.isFinite(pixelHeight) ? pixelHeight : '—'} px</p>
          <button type="button" disabled={!valid} onClick={() => { const values = [{ id: crypto.randomUUID(), settings: current() }, ...saved].slice(0, 30); if (store('saved', values)) { setSaved(values); setTab('saved'); setSelected(values[0].id); } }}>{t.save}</button>
          {tab === 'saved' && saved.some(item => item.id === selected) && <button type="button" onClick={() => { const values = saved.filter(item => item.id !== selected); if (store('saved', values)) setSaved(values); }}>{t.remove}</button>}
        </fieldset>
      </div>
      <footer><span role="status">{error || (!valid ? t.invalid : '')}</span><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button type="submit" className="preset-create" disabled={busy || !valid}>{busy ? '…' : t.create}</button></footer>
    </form>
  </dialog>, document.body);
}
