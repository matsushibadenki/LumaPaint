import './transform-panel.css';
import { useEffect, useRef, useState } from 'react';
import { editTransformPanel, type DocumentSnapshot, type TransformPanelEdit } from '../bridge';
import { readPreference, savePreference, type Locale } from '../i18n';

export const transformLabels = {
  en: { title: 'Transform', reference: 'Reference point', ratio: 'Constrain proportions', cornersLink: 'Link corner radii', rectangle: 'Rectangle Properties:', width: 'Width', height: 'Height', rotation: 'Rotation', shear: 'Shear', corner: ['Top left corner', 'Top right corner', 'Bottom right corner', 'Bottom left corner'], scaleCorners: 'Scale Corners', scaleStrokes: 'Scale Strokes & Effects', select: 'Select visible, unlocked vector objects.', units: 'Units', menu: 'Panel options', presets: 'presets', increase: 'Increase', decrease: 'Decrease', invalid: 'Enter a valid number.' },
  ja: { title: '変形', reference: '基準点', ratio: '縦横比を固定', cornersLink: '角半径を連動', rectangle: '長方形のプロパティ：', width: '幅', height: '高さ', rotation: '回転', shear: 'シアー', corner: ['左上の角', '右上の角', '右下の角', '左下の角'], scaleCorners: '角を拡大・縮小', scaleStrokes: '線幅と効果も拡大・縮小', select: '表示中のロックされていないベクターオブジェクトを選択してください。', units: '単位', menu: 'パネルオプション', presets: 'プリセット', increase: '増やす', decrease: '減らす', invalid: '有効な数値を入力してください。' },
  'zh-CN': { title: '变换', reference: '参考点', ratio: '锁定长宽比', cornersLink: '链接圆角半径', rectangle: '矩形属性：', width: '宽度', height: '高度', rotation: '旋转', shear: '倾斜', corner: ['左上角', '右上角', '右下角', '左下角'], scaleCorners: '缩放圆角', scaleStrokes: '缩放描边和效果', select: '请选择可见且未锁定的矢量对象。', units: '单位', menu: '面板选项', presets: '预设', increase: '增加', decrease: '减少', invalid: '请输入有效数值。' },
};

function LinkIcon({ linked }: { linked: boolean }) { return <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true"><path d="M9 15l6-6M8 13l-2 2a4 4 0 006 6l3-3M16 11l2-2a4 4 0 00-6-6l-3 3" />{!linked && <path d="M4 3l16 18" />}</svg>; }

type Unit = 'mm'|'cm'|'in'|'px'|'pt';
function Field({ label, symbol, value, unit, onCommit, min, max, invalid, presetsLabel, stepperLabels }: { label: string; symbol: string; value: number; unit: string; onCommit: (v: number) => void; min?: number; max?: number; invalid: string; presetsLabel?: string; stepperLabels?: [string, string] }) {
  const formatted = String(Number(value.toFixed(3)));
  const [draft, setDraft] = useState(formatted);
  const dirty = useRef(false);
  useEffect(() => { setDraft(formatted); dirty.current = false; }, [formatted]);
  const commit = (input: HTMLInputElement) => {
    if (!dirty.current) return;
    const number = Number(draft);
    if (!draft.trim() || !Number.isFinite(number) || (min !== undefined && number < min) || (max !== undefined && number > max)) { input.setCustomValidity(invalid); input.reportValidity(); setDraft(formatted); dirty.current = false; return; }
    input.setCustomValidity(''); dirty.current = false;
    if (Math.abs(number - value) > 0.000001) onCommit(number);
  };
  const step = (amount: number) => {
    const next = (draft.trim() && Number.isFinite(Number(draft)) ? Number(draft) : value) + amount;
    if ((min !== undefined && next < min) || (max !== undefined && next > max)) return;
    dirty.current = false; setDraft(String(next)); onCommit(next);
  };
  return <label className="transform-field" title={label}><span className="transform-symbol" aria-hidden="true">{symbol}</span><span className="transform-input">{stepperLabels && <span className="transform-stepper"><button type="button" aria-label={stepperLabels[0]} onMouseDown={e => e.preventDefault()} onClick={() => step(1)}>⌃</button><button type="button" aria-label={stepperLabels[1]} onMouseDown={e => e.preventDefault()} onClick={() => step(-1)}>⌄</button></span>}<input aria-label={label} inputMode="decimal" value={draft} onChange={event => { dirty.current = true; event.target.setCustomValidity(''); setDraft(event.target.value); }} onBlur={event => commit(event.currentTarget)} onKeyDown={event => { if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); const next = Number(draft) + (event.key === 'ArrowUp' ? 1 : -1); if (Number.isFinite(next) && (min === undefined || next >= min) && (max === undefined || next <= max)) { dirty.current = true; setDraft(String(next)); } } if (event.key === 'Enter') event.currentTarget.blur(); if (event.key === 'Escape') { dirty.current = false; setDraft(formatted); event.currentTarget.setCustomValidity(''); event.currentTarget.blur(); } }} /><span aria-hidden="true">{unit}</span>{presetsLabel && <select className="transform-angle-presets" aria-label={presetsLabel} value="" onChange={e => { const next = Number(e.target.value); dirty.current = false; setDraft(String(next)); onCommit(next); }}><option value="">⌄</option>{[-180,-90,-45,-30,-15,0,15,30,45,90,180].filter(v => (min === undefined || v >= min) && (max === undefined || v <= max)).map(v => <option key={v} value={v}>{v}°</option>)}</select>}</span></label>;
}

export function TransformPanel({ locale, document, enabled, onUpdate }: { locale: Locale; document: DocumentSnapshot; enabled: boolean; onUpdate: (snapshot: DocumentSnapshot) => void }) {
  const t = transformLabels[locale];
  const [reference, setReference] = useState<[number, number]>([0, 0]);
  const [proportional, setProportional] = useState(false);
  const [linkedCorners, setLinkedCorners] = useState(true);
  const [scaleCorners, setScaleCorners] = useState(() => readPreference('transform-scale-corners') !== 'false');
  const [scaleStrokes, setScaleStrokes] = useState(() => readPreference('transform-scale-strokes') !== 'false');
  const [unit, setUnit] = useState<Unit>(() => { const stored = readPreference('transform-unit'); return ['mm','cm','in','px','pt'].includes(stored ?? '') ? stored as Unit : 'mm'; });
  const [menu, setMenu] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const info = document.transformPanel;
  const editable = enabled && !!info;
  const dpi = Math.max(1, document.resolution);
  const factor = unit === 'mm' ? dpi / 25.4 : unit === 'cm' ? dpi / 2.54 : unit === 'in' ? dpi : unit === 'pt' ? dpi / 72 : 1;
  const box = info?.corners ?? [[0,0],[0,0],[0,0],[0,0]];
  const x = box[0][0] + (box[1][0]-box[0][0])*reference[0] + (box[3][0]-box[0][0])*reference[1];
  const y = box[0][1] + (box[1][1]-box[0][1])*reference[0] + (box[3][1]-box[0][1])*reference[1];
  const apply = async (field: TransformPanelEdit['field'], value: number | [number, number, number, number]) => {
    if (!editable || busy) return;
    setBusy(true); setError('');
    try { onUpdate(await editTransformPanel({ field, values: typeof value === 'number' ? [value,0,0,0] : value, reference, proportional, scaleCorners, scaleStrokes, revision: document.revision, ids: document.selectedVectorObjects })); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const field = (key: 'x'|'y'|'width'|'height'|'rotation'|'shear', value: number, label: string, symbol: string) => <Field key={`${document.selectedVectorObjects.join(',')}-${key}`} label={label} symbol={symbol} value={key === 'rotation' || key === 'shear' ? value : value/factor} unit={key === 'rotation' || key === 'shear' ? '°' : unit} min={key === 'width' || key === 'height' ? 0.001 : key === 'shear' ? -88.999 : undefined} max={key === 'shear' ? 88.999 : undefined} invalid={t.invalid} presetsLabel={key === 'rotation' || key === 'shear' ? `${label} ${t.presets}` : undefined} onCommit={v => void apply(key, key === 'rotation' || key === 'shear' ? v : v*factor)} />;
  return <div className="transform-panel">
    <div className="transform-heading"><strong>{t.title}</strong><button className="transform-menu-button" aria-label={t.menu} aria-expanded={menu} onClick={() => setMenu(!menu)}>☰</button></div>
    {menu && <label className="transform-unit-menu">{t.units}<select aria-label={t.units} value={unit} onChange={e => { const next = e.target.value as Unit; setUnit(next); savePreference('transform-unit', next); setMenu(false); }}>{['mm','cm','in','px','pt'].map(u => <option key={u} value={u}>{u}</option>)}</select></label>}
    <fieldset disabled={!editable || busy} className="transform-controls">
      <div className="transform-top">
        <div className="transform-reference" role="group" aria-label={t.reference}>{[0,.5,1].flatMap((v,row) => [0,.5,1].map((u,col) => <button key={`${row}-${col}`} type="button" aria-label={`${t.reference} ${row+1}, ${col+1}`} aria-pressed={reference[0] === u && reference[1] === v} onClick={() => setReference([u,v])} />))}</div>
        <div className="transform-coordinate-grid">{field('x',x,'X','X :')}{field('width',info?.width ?? 0,t.width,'W :')}{field('y',y,'Y','Y :')}{field('height',info?.height ?? 0,t.height,'H :')}{field('rotation',info?.rotation ?? 0,t.rotation,'∠ :')}{field('shear',info?.shear ?? 0,t.shear,'▱ :')}</div>
        <button className={`transform-link${proportional ? ' active' : ''}`} title={t.ratio} aria-label={t.ratio} aria-pressed={proportional} onClick={() => setProportional(!proportional)}><LinkIcon linked={proportional} /></button>
      </div>
      <div className="transform-rectangle">
        <div className="transform-section-title">{t.rectangle}</div>
        <fieldset disabled={!info?.rectangle} className="transform-rectangle-controls">
          <div className="transform-rectangle-size">{field('width',info?.width ?? 0,t.width,'↔')}<button className={`transform-link${proportional ? ' active' : ''}`} title={t.ratio} aria-label={t.ratio} aria-pressed={proportional} onClick={() => setProportional(!proportional)}><LinkIcon linked={proportional} /></button>{field('height',info?.height ?? 0,t.height,'↕')}</div>
          <div className="transform-rectangle-rotation">{field('rotation',info?.rotation ?? 0,t.rotation,'⟲')}</div>
          <div className="transform-corner-grid">{[0,1,3,2].map(index => <Field key={`${document.selectedVectorObjects.join(',')}-corner-${index}`} label={t.corner[index]} symbol={['┌','┐','┘','└'][index]} value={(info?.radii[index] ?? 0)/factor} unit={unit} stepperLabels={[`${t.corner[index]} ${t.increase}`, `${t.corner[index]} ${t.decrease}`]} min={0} max={4096/factor} invalid={t.invalid} onCommit={v => { const radii = [...(info?.radii ?? [0,0,0,0])] as [number,number,number,number]; if (linkedCorners) radii.fill(v*factor); else radii[index] = v*factor; void apply('corners',radii); }} />)}<button className={`transform-link transform-corner-link${linkedCorners ? ' active' : ''}`} aria-label={t.cornersLink} title={t.cornersLink} aria-pressed={linkedCorners} onClick={() => setLinkedCorners(!linkedCorners)}><LinkIcon linked={linkedCorners} /></button></div>
        </fieldset>
      </div>
      <div className="transform-options"><label><input type="checkbox" checked={scaleCorners} onChange={e => { setScaleCorners(e.target.checked); savePreference('transform-scale-corners',String(e.target.checked)); }} />{t.scaleCorners}</label><label><input type="checkbox" checked={scaleStrokes} onChange={e => { setScaleStrokes(e.target.checked); savePreference('transform-scale-strokes',String(e.target.checked)); }} />{t.scaleStrokes}</label></div>
    </fieldset>
    {!editable && <p className="transform-hint">{t.select}</p>}
    {error && <p className="panel-error" role="alert">{error}</p>}
  </div>;
}
