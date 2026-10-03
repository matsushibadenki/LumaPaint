import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import { useRef, useState } from 'react';
import type { Brush, BrushEnvelope } from '../bridge';
import type { Locale } from '../i18n';
import { CompactSlider } from './CompactSlider';

const defaults: BrushEnvelope = { enabled: false, attack: 20, decay: 100, sustain: .6, hold: 200, release: 200, dryness: .5 };
const messages = {
  ja: { title: '減衰エンベロープ', attack: '立ち上がり', decay: '減衰', sustain: '持続濃度', hold: '持続距離', release: '消え際', dryness: 'かすれ量', reset: 'リセット', hint: '横軸：描画距離（px）／縦軸：濃度。点をドラッグして調整できます。終端以降は描画されず、次のストロークで再開します。ペイント用ブラシ・インクペンに適用。', length: '全長' },
  en: { title: 'Decay envelope', attack: 'Attack', decay: 'Decay', sustain: 'Sustain level', hold: 'Hold distance', release: 'Release', dryness: 'Dryness', reset: 'Reset', hint: 'X: distance (px) / Y: density. Drag the points to adjust. Drawing stops after the endpoint and restarts on the next stroke. Applies to paint brushes and ink pens.', length: 'Total' },
  'zh-CN': { title: '衰减包络', attack: '起音', decay: '衰减', sustain: '持续浓度', hold: '持续距离', release: '释音', dryness: '干涩程度', reset: '重置', hint: '横轴：绘制距离（px）／纵轴：浓度。拖动节点调整。终点后停止绘制，下一个笔画重新开始。适用于绘画画笔与墨水笔。', length: '总长' },
};
export function BrushEnvelopePanel({ locale, brush, enabled, onChange, resolution=72 }: { locale: Locale; resolution?:number; brush: Brush; enabled: boolean; onChange: (brush: Brush) => void }) {
  const unit=useMeasurementUnit(),factor=pixelsPerMeasurement(unit,resolution),symbol=unitSymbols[unit];
  const t = messages[locale]; const envelope = brush.envelope ?? defaults;
  const [viewScale, setViewScale] = useState<number | null>(null);
  const total = envelope.attack + envelope.decay + envelope.hold + envelope.release;
  const scale = viewScale ?? Math.max(100, total * 1.2);
  const x = (v: number) => 14 + v / scale * 250;
  const y = (v: number) => 112 - v * 92;
  const breaks = [envelope.attack, envelope.attack + envelope.decay, envelope.attack + envelope.decay + envelope.hold, total];
  const levels = [1, envelope.sustain, envelope.sustain, 0];
  const drag = useRef<{ index: number; envelope: BrushEnvelope; scale: number } | null>(null);
  // Numeric edits may reframe the plot; pointer release must keep its coordinate system.
  const update = (patch: Partial<BrushEnvelope>) => {
    if (patch.attack !== undefined || patch.decay !== undefined || patch.hold !== undefined || patch.release !== undefined) setViewScale(null);
    onChange({ ...brush, envelope: { ...envelope, ...patch } });
  };
  return <section className="brush-envelope">
    <label className="brush-envelope-enable"><input type="checkbox" disabled={!enabled} checked={envelope.enabled} onChange={e => update({ enabled: e.target.checked })} />{t.title}</label>
    <svg viewBox="0 0 280 140" aria-label={t.title} className={!enabled || !envelope.enabled ? 'inactive' : ''}
      onPointerMove={event => {
        if (!drag.current) return;
        const rect = event.currentTarget.getBoundingClientRect();
        const dx = ((event.clientX - rect.left) / rect.width * 280 - 14) / 250 * drag.current.scale;
        const level = Math.max(0, Math.min(1, (112 - (event.clientY - rect.top) / rect.height * 140) / 92));
        const { index, envelope: start } = drag.current;
        const durations = [start.attack, start.decay, start.hold, start.release];
        const remaining = durations.reduce((sum, value, i) => i === index ? sum : sum + value, 0);
        const limit = Math.max(0, drag.current.scale - remaining);
        const clamp = (v: number) => Math.round(Math.max(0, Math.min(10000, limit, v)));
        const next = { ...start };
        if (index === 0) next.attack = clamp(dx);
        if (index === 1) { next.decay = clamp(dx - start.attack); next.sustain = Math.round(level * 100) / 100; }
        if (index === 2) { next.hold = clamp(dx - start.attack - start.decay); next.sustain = Math.round(level * 100) / 100; }
        if (index === 3) next.release = clamp(dx - start.attack - start.decay - start.hold);
        onChange({ ...brush, envelope: next });
      }} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }} onLostPointerCapture={() => { drag.current = null; }}>
      {[0, .5, 1].map(v => <path key={v} className="envelope-grid" d={`M14 ${y(v)} H270`} />)}
      {breaks.map((v, i) => <path key={i} className="envelope-grid" d={`M${x(v)} 12 V112`} />)}
      <path className="envelope-fill" d={`M14 112 L14 ${y(envelope.attack === 0 ? 1 : 0)} ${breaks.map((v, i) => `L${x(v)} ${y(levels[i])}`).join(' ')} L270 112 Z`} />
      <path className="envelope-line" d={`M14 ${y(envelope.attack === 0 ? 1 : 0)} ${breaks.map((v, i) => `L${x(v)} ${y(levels[i])}`).join(' ')} L270 112`} />
      {breaks.map((v, i) => <circle key={i} cx={x(v)} cy={y(levels[i])} r="6" className="envelope-handle" onPointerDown={event => { if (!enabled || !envelope.enabled) return; event.preventDefault(); setViewScale(scale); drag.current = { index: i, envelope: { ...envelope }, scale }; event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId); }} />)}
      <text x="14" y="132">0</text><text x="268" y="132" textAnchor="end">{t.length}: {Number((total/factor).toFixed(3))} {symbol}</text>
    </svg>
    <p className="muted small">{t.hint.replace('px',symbol)}</p>
    {(['attack', 'decay', 'hold', 'release', 'sustain', 'dryness'] as const).map(key => {
      const percent = key === 'sustain' || key === 'dryness';
      const value = percent ? Math.round(envelope[key] * 100) : Number((envelope[key]/factor).toFixed(4));
      return <label className="envelope-control" key={key}><span>{t[key]}</span><CompactSlider aria-label={t[key]} disabled={!enabled || !envelope.enabled} min={0} max={percent ? 100 : 10000/factor} step={percent?1:1/factor} value={value} onChange={event => update({ [key]: Number(event.target.value) / (percent ? 100 : 1/factor) })} /><input aria-label={`${t[key]} ${percent ? '%' : symbol}`} disabled={!enabled || !envelope.enabled} type="number" step="any" min="0" max={percent ? 100 : 10000/factor} value={value} onChange={event => { const n = Number(event.target.value); if (Number.isFinite(n)) update({ [key]: Math.max(0, Math.min(percent ? 100 : 10000/factor, n)) / (percent ? 100 : 1/factor) }); }} /><small>{percent ? '%' : symbol}</small></label>;
    })}
    <button type="button" disabled={!enabled} onClick={() => update({ ...defaults, enabled: envelope.enabled })}>{t.reset}</button>
  </section>;
}
