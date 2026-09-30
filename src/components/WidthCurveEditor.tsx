import { useEffect, useRef, useState } from 'react';
import type { WidthStop } from '../bridge';
import type { Locale } from '../i18n';
const labels = {
  en: { graph: 'Edit width curve', hint: 'Double-click to add a point. Drag to move. Width is a multiplier of the stroke weight.', position: 'Position', width: 'Width', slope: 'Slope', remove: 'Remove point', add: 'Add point', point: 'Edit point' },
  ja: { graph: '幅カーブの編集', hint: 'ダブルクリックで点を追加、ドラッグで移動。幅は線幅に対する倍率です。', position: '位置', width: '幅', slope: '傾き', remove: '点を削除', add: '点を追加', point: '編集する点' },
  'zh-CN': { graph: '编辑宽度曲线', hint: '双击添加点，拖动移动。宽度为描边粗细的倍数。', position: '位置', width: '宽度', slope: '斜率', remove: '删除点', add: '添加点', point: '编辑点' },
};
function CurveNumber({ label, value, min, max, step, disabled = false, onCommit }: { label: string; value: number; min: number; max: number; step: number; disabled?: boolean; onCommit: (value: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(Number(value.toFixed(3)))), [value]);
  const commit = () => { const n = Number(text); if (text.trim() && Number.isFinite(n) && n !== value) onCommit(n); else setText(String(Number(value.toFixed(3)))); };
  return <label className="stroke-setting"><span>{label}</span><input type="number" min={min} max={max} step={step} disabled={disabled} value={text} onChange={e => setText(e.target.value)} onBlur={commit} onKeyDown={e => { if (e.key === 'Enter') e.currentTarget.blur(); if (e.key === 'Escape') setText(String(value)); }} /></label>;
}
export function WidthCurveEditor({ curve, locale, onCommit, disabled }: { curve: WidthStop[]; locale: Locale; disabled: boolean; onCommit: (curve: WidthStop[]) => void }) {
  const t = labels[locale];
  const [draft, setDraft] = useState(curve);
  const draftRef = useRef(curve);
  const [selected, setSelected] = useState(0);
  const dragging = useRef(false);
  const graph = useRef<SVGSVGElement>(null);
  useEffect(() => { if (!dragging.current) { setDraft(curve); draftRef.current = curve; setSelected(i => Math.min(i, curve.length - 1)); } }, [curve]);
  const set = (next: WidthStop[]) => { draftRef.current = next; setDraft(next); };
  const update = (key: keyof WidthStop, value: number, commit = true) => {
    if (!Number.isFinite(value)) return;
    const next = draftRef.current.map(p => ({ ...p }));
    const point = next[selected];
    if (key === 'position') {
      if (selected === 0 || selected === next.length - 1) return;
      value = Math.min(next[selected + 1].position - .001, Math.max(next[selected - 1].position + .001, value));
    } else value = key === 'width' ? Math.max(0, Math.min(4, value)) : Math.max(-20, Math.min(20, value));
    point[key] = value;
    set(next); if (commit) onCommit(next);
  };
  const coordinate = (clientX: number, clientY: number) => {
    const rect = graph.current!.getBoundingClientRect();
    return { position: Math.max(0, Math.min(1, (clientX - rect.left) / rect.width)), width: Math.max(0, Math.min(4, 4 * (1 - (clientY - rect.top) / rect.height))) };
  };
  const add = (position = .5, width = 1) => {
    if (draft.length >= 32) return;
    const gaps = draft.slice(1).map((p, i) => ({ i, span: p.position - draft[i].position }));
    if (draft.some(p => Math.abs(p.position - position) < .001)) {
      const gap = gaps.reduce((a,b) => b.span > a.span ? b : a);
      if (gap.span < .002) return;
      position = (draft[gap.i].position + draft[gap.i + 1].position) / 2;
    }
    position = Math.max(.001, Math.min(.999, position));
    const next = [...draft, { position, width, slope: 0 }].sort((a,b) => a.position - b.position);
    setSelected(next.findIndex(p => p.position === position)); set(next); onCommit(next);
  };
  const samples = Array.from({ length: 257 }, (_, i) => {
    const x = i / 256;
    const pairIndex = draft.findIndex((p, j) => j > 0 && p.position >= x);
    const b = draft[Math.max(1, pairIndex)]; const a = draft[Math.max(1, pairIndex) - 1];
    const span = b.position - a.position, u = (x - a.position) / span;
    const width = (2*u**3-3*u*u+1)*a.width + (u**3-2*u*u+u)*span*a.slope + (-2*u**3+3*u*u)*b.width + (u**3-u*u)*span*b.slope;
    return `${i === 0 ? 'M' : 'L'}${x * 240} ${100 - Math.min(4, Math.max(0, width)) * 25}`;
  }).join('');
  const point = draft[selected] ?? draft[0];
  return <div className="width-curve-editor">
    <svg ref={graph} viewBox="0 0 240 100" preserveAspectRatio="none" role="img" aria-label={t.graph} onDoubleClick={e => { if (disabled || (e.target as Element).tagName === 'circle') return; const p = coordinate(e.clientX,e.clientY); add(p.position,p.width); }}>
      {[25,50,75].map(y => <path key={y} d={`M0 ${y}H240`} className="width-curve-grid" />)}
      <path d={samples} className="width-curve-line" />
      {draft.map((p,i) => <circle key={i} cx={p.position*240} cy={100-p.width*25} r={i === selected ? 5 : 4} className={i === selected ? 'selected' : ''} onPointerDown={e => { if (disabled) return; e.preventDefault(); setSelected(i); dragging.current = true; e.currentTarget.setPointerCapture(e.pointerId); }} onPointerMove={e => { if (!dragging.current || !e.currentTarget.hasPointerCapture(e.pointerId)) return; const p = coordinate(e.clientX,e.clientY); update('position',p.position,false); update('width',p.width,false); }} onPointerUp={e => { if (!dragging.current) return; dragging.current = false; e.currentTarget.releasePointerCapture(e.pointerId); onCommit(draftRef.current); }} onPointerCancel={() => { dragging.current = false; set(curve); }} />)}
    </svg>
    <p className="stroke-hint">{t.hint}</p>
    <label className="stroke-setting"><span>{t.point}</span><select value={selected} onChange={e => setSelected(Number(e.target.value))}>{draft.map((_,i) => <option key={i} value={i}>{i+1}</option>)}</select></label>
    <CurveNumber label={`${t.position} (%)`} min={0} max={100} step={1} disabled={selected === 0 || selected === draft.length - 1} value={point.position * 100} onCommit={n => update('position',n/100)} />
    <CurveNumber label={`${t.width} (×)`} min={0} max={4} step={.1} value={point.width} onCommit={n => update('width',n)} />
    <CurveNumber label={t.slope} min={-20} max={20} step={.25} value={point.slope} onCommit={n => update('slope',n)} />
    <div className="stroke-dash-heading"><button type="button" disabled={draft.length >= 32} onClick={() => add()}>{t.add}</button><button type="button" disabled={selected === 0 || selected === draft.length - 1} onClick={() => { const next = draft.filter((_,i) => i !== selected); setSelected(0); set(next); onCommit(next); }}>{t.remove}</button></div>
  </div>;
}
