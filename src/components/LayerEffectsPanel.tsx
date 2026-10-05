import { useEffect, useRef, useState } from 'react';
import { defaultLayerEffects, type LayerEffects, type LayerSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import { GradingWheel } from './GradingWheel';
import { CompactSlider } from './CompactSlider';
import { CurveEditor } from './CurveEditor';
export const effectLabels = {
  ja: { title: 'レイヤーエフェクト', light: 'ライト', color: 'カラー', curve: 'カーブ', preview: 'エフェクトを適用', reset: 'リセット', unavailable: 'レイヤーを選択してください。', channel: ['RGB', 'レッド', 'グリーン', 'ブルー'], values: ['露光量', 'コントラスト', 'ハイライト', 'シャドウ', '白レベル', '黒レベル', '色温度', '色かぶり補正', '自然な彩度', '彩度'], points: '中間点を追加', hint: ['数値は確定後に反映します。', 'スライダーは離すと反映します。'] },
  en: { title: 'Layer Effects', light: 'Light', color: 'Color', curve: 'Curves', preview: 'Apply effects', reset: 'Reset', unavailable: 'Select a layer.', channel: ['RGB', 'Red', 'Green', 'Blue'], values: ['Exposure', 'Contrast', 'Highlights', 'Shadows', 'Whites', 'Blacks', 'Temperature', 'Tint', 'Vibrance', 'Saturation'], points: 'Add midpoints', hint: ['Confirm a value to update the preview.', 'Release a slider to apply its value.'] },
  'zh-CN': { title: '图层效果', light: '光线', color: '颜色', curve: '曲线', preview: '应用效果', reset: '重置', unavailable: '请选择图层。', channel: ['RGB', '红色', '绿色', '蓝色'], values: ['曝光', '对比度', '高光', '阴影', '白色色阶', '黑色色阶', '色温', '色调', '自然饱和度', '饱和度'], points: '添加中间点', hint: ['确认数值后更新预览。', '释放滑块后应用数值。'] },
};
function EffectNumber({ value, label, limit, min = -limit, disabled, onCommit }: { value: number; label: string; limit: number; min?: number; disabled: boolean; onCommit: (value: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => { setText(String(value)); }, [value]);
  const valid = text.trim() !== '' && Number.isFinite(Number(text)) && Number(text) >= min && Number(text) <= limit;
  return <input className="effect-number" type="text" inputMode="decimal" aria-label={label} aria-invalid={!valid} disabled={disabled} value={text}
    onChange={e => setText(e.currentTarget.value)} onBlur={() => { if (valid) onCommit(Number(text)); else setText(String(value)); }}
    onKeyDown={e => { if(e.key === 'Enter') e.currentTarget.blur(); if(e.key === 'Escape') { setText(String(value)); e.preventDefault(); } }} />;
}
export function LayerEffectsPanel({ locale, layer, enabled, onCommit }: { locale: Locale; layer?: LayerSnapshot; enabled: boolean; onCommit: (id: string, effects: LayerEffects) => void }) {
  const t = effectLabels[locale];
  const effects = layer?.effects ?? defaultLayerEffects();
  const signature = JSON.stringify(effects);
  const [draft, setDraft] = useState(effects);
  const [channel, setChannel] = useState(0);
  const [mixerMode, setMixerMode] = useState(0);
  const extra = locale === 'ja' ? {mixer:'カラーミキサー', grading:'カラーグレーディング', modes:['色相','彩度','輝度'], colors:['レッド','オレンジ','イエロー','グリーン','アクア','ブルー','パープル','マゼンタ'], zones:['シャドウ','中間調','ハイライト']} : locale === 'zh-CN' ? {mixer:'颜色混合器',grading:'颜色分级',modes:['色相','饱和度','明度'],colors:['红色','橙色','黄色','绿色','青色','蓝色','紫色','洋红色'],zones:['阴影','中间调','高光']} : {mixer:'Color Mixer',grading:'Color Grading',modes:['Hue','Saturation','Luminance'],colors:['Red','Orange','Yellow','Green','Aqua','Blue','Purple','Magenta'],zones:['Shadows','Midtones','Highlights']};
  const committed = useRef(signature);
  useEffect(() => { if (enabled) { setDraft(JSON.parse(signature) as LayerEffects); committed.current = signature; } }, [signature, layer?.id, enabled]);
  if (!layer?.effects) return <p>{t.unavailable}</p>;
  const disabled = !enabled || layer.locked;
  const commit = (next: LayerEffects) => {
    if (disabled || next.values.some((v,i) => !Number.isFinite(v) || Math.abs(v) > (i === 0 ? 5 : 100))) return;
    const key = JSON.stringify(next); if (key === committed.current) return;
    committed.current = key; onCommit(layer.id, next);
  };
  const change = (index: number, value: number) => { const next = { ...draft, values: draft.values.map((v,i) => i === index ? value : v) }; setDraft(next); return next; };
  const rows = (from: number, to: number) => t.values.slice(from,to).map((label,i) => { const index = from+i; const limit = index === 0 ? 5 : 100; return <div className="effect-control" key={label}><span>{label}</span><EffectNumber label={label} limit={limit} disabled={disabled || !draft.enabled} value={draft.values[index]} onCommit={value => commit(change(index,value))} /><CompactSlider aria-label={label} min={-limit} max={limit} step={index === 0 ? .1 : 1} disabled={disabled || !draft.enabled} value={draft.values[index]} onChange={e => change(index,Number(e.currentTarget.value))} onPointerUp={e => commit(change(index, Number(e.currentTarget.value)))} onKeyUp={() => commit(draft)} onBlur={() => commit(draft)} /></div>; });
  const adjust = (field: 'mixer' | 'grading', row: number, column: number, value: number) => {
    const next = {...draft, [field]: draft[field].map((v,i)=>i===row?v.map((n,j)=>j===column?value:n):v)};
    setDraft(next); return next;
  };
  const colorRow = (field: 'mixer' | 'grading', row: number, column: number, label: string, min: number, max: number) => <div className="effect-control" key={label}><span>{label.split(' · ').at(-1)}</span><EffectNumber label={label} min={min} limit={max} disabled={disabled || !draft.enabled} value={draft[field][row][column]} onCommit={v=>commit(adjust(field,row,column,v))}/><CompactSlider aria-label={label} min={min} max={max} disabled={disabled || !draft.enabled} value={draft[field][row][column]} onChange={e=>adjust(field,row,column,Number(e.currentTarget.value))} onPointerUp={e=>commit(adjust(field,row,column,Number(e.currentTarget.value)))} onKeyUp={()=>commit(draft)} onBlur={()=>commit(draft)}/></div>;
  const adjustWheel = (row: number, hue: number, saturation: number) => {
    const next = {...draft, grading: draft.grading.map((v,i)=>i===row?[hue,saturation,v[2]]:v)};
    setDraft(next); return next;
  };
  const gradingLabels = locale==='ja'?['ブレンド','バランス']:locale==='zh-CN'?['混合','平衡']:['Blending','Balance'];
  const gradingControl = (field: 'gradingBlend' | 'gradingBalance', label: string, min: number) => {
    const update=(v:number)=>{const next={...draft,[field]:v};setDraft(next);return next;};
    return <div className="effect-control" key={field}><span>{label.split(' · ').at(-1)}</span><EffectNumber label={label} min={min} limit={100} value={draft[field]} disabled={disabled || !draft.enabled} onCommit={v=>commit(update(v))}/><CompactSlider aria-label={label} min={min} max={100} disabled={disabled || !draft.enabled} value={draft[field]} onChange={e=>update(Number(e.currentTarget.value))} onPointerUp={e=>commit(update(Number(e.currentTarget.value)))} onKeyUp={()=>commit(draft)} onBlur={()=>commit(draft)}/></div>;
  };
  return <div className="layer-effects-panel"><header className="effect-header"><h3>{t.title}</h3><p className="effect-layer-name" title={layer.name}>{layer.name}</p></header><div className="effect-toolbar"><label className="effect-enable"><input type="checkbox" disabled={disabled} checked={draft.enabled} onChange={e => { const next={...draft,enabled:e.currentTarget.checked};setDraft(next);commit(next); }} />{t.preview}</label><button disabled={disabled} onClick={() => { const next=defaultLayerEffects();setDraft(next);commit(next); }}>{t.reset}</button></div><p className="effect-hint">{t.hint.map(line=><span key={line}>{line}</span>)}</p>
    <details open><summary>{t.light}</summary>{rows(0,6)}</details><details open><summary>{t.color}</summary>{rows(6,10)}</details>
    <details open><summary>{t.curve}</summary><select aria-label={t.curve} disabled={disabled} value={channel} onChange={e=>setChannel(Number(e.currentTarget.value))}>{t.channel.map((name,i)=><option key={name} value={i}>{name}</option>)}</select>
      <CurveEditor key={`${layer.id}-${channel}`} locale={locale} channel={t.channel[channel]} points={draft.curves[channel]} smooth={draft.curveSmooth[channel]} disabled={disabled || !draft.enabled}
        onChange={(points,smooth)=>{setDraft({...draft,curves:draft.curves.map((c,i)=>i===channel?points:c),curveSmooth:draft.curveSmooth.map((s,i)=>i===channel?smooth:s)});}}
        onCommit={(points,smooth)=>{const next={...draft,curves:draft.curves.map((c,i)=>i===channel?points:c),curveSmooth:draft.curveSmooth.map((s,i)=>i===channel?smooth:s)};setDraft(next);commit(next);}}/>
    </details>
    <details open><summary>{extra.mixer}</summary><select aria-label={extra.mixer} value={mixerMode} onChange={e=>setMixerMode(Number(e.currentTarget.value))}>{extra.modes.map((label,i)=><option key={label} value={i}>{label}</option>)}</select>{extra.colors.map((label,i)=>colorRow('mixer',i,mixerMode,label,-100,100))}</details>
    <details open><summary>{extra.grading}</summary>{extra.zones.map((label,i)=><fieldset key={label} disabled={disabled || !draft.enabled}><legend>{label}</legend><GradingWheel label={`${label} · ${extra.modes[0]} / ${extra.modes[1]}`} hue={draft.grading[i][0]} saturation={draft.grading[i][1]} disabled={disabled || !draft.enabled} onChange={(h,s)=>adjustWheel(i,h,s)} onCommit={(h,s)=>commit(adjustWheel(i,h,s))}/>{extra.modes.map((mode,j)=>colorRow('grading',i,j,`${label} · ${mode}`,j===2?-100:0,j===0?360:100))}</fieldset>)}{gradingControl('gradingBlend',gradingLabels[0],0)}{gradingControl('gradingBalance',gradingLabels[1],-100)}</details></div>;
}
