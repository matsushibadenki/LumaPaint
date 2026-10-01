import { useEffect, useRef, useState } from 'react';
import { directControlInfo, editDirectControls, type DirectControlInfo, type DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';
const labels = {
  ja: { title:'節点・ライブコーナー', position:'絶対座標', move:'選択点を移動', corner:'ライブコーナー', x:'X (px)', y:'Y (px)', radius:'半径 (px)', apply:'適用', cancel:'キャンセル', empty:'ダイレクト選択で節点またはハンドルを選択してください。', hint:'直線の角に対応。半径は隣接辺の長さに合わせて制限されます。', preview:'形状プレビュー', loading:'選択点を取得中…' },
  en: { title:'Controls & Live Corners', position:'Absolute coordinates', move:'Move selected controls', corner:'Live corners', x:'X (px)', y:'Y (px)', radius:'Radius (px)', apply:'Apply', cancel:'Cancel', empty:'Select anchors or handles with Direct Selection.', hint:'For straight corners. Radii are limited by adjacent edge lengths.', preview:'Shape preview', loading:'Loading selected controls…' },
  'zh-CN': { title:'节点与实时圆角', position:'绝对坐标', move:'移动所选控制点', corner:'实时圆角', x:'X (px)', y:'Y (px)', radius:'半径 (px)', apply:'应用', cancel:'取消', empty:'请使用直接选择工具选择锚点或手柄。', hint:'适用于直线角点。半径受相邻边长限制。', preview:'形状预览', loading:'正在获取所选控制点…' },
};
export function DirectControlDialog({locale,doc,onApply,onClose,onBusyChange}: {locale:Locale;doc:DocumentSnapshot;onApply:(doc:DocumentSnapshot)=>void;onClose:()=>void;onBusyChange?:(busy:boolean)=>Promise<void>}) {
  const t=labels[locale];const ref=useRef<HTMLDialogElement>(null);
  const [points,setPoints]=useState<DirectControlInfo[]|null>(null);
  const [mode,setMode]=useState<'position'|'move'|'corner'>('position');
  const [values,setValues]=useState<number[]>([0,0]);const [svg,setSvg]=useState('');
  const [error,setError]=useState('');const [busy,setBusy]=useState(false);const [previewing,setPreviewing]=useState(false);
  const generation=useRef(0);
  useEffect(()=> {ref.current?.showModal();let active=true;void directControlInfo().then(p=>{if(!active)return;setPoints(p);setMode(p.length===1?'position':'move');setValues(p.length===1?[p[0].x,p[0].y]:[0,0]);}).catch(e=>{if(active)setError(String(e));});return()=>{active=false;generation.current++;};},[]);
  useEffect(()=> {
    const serial=++generation.current;
    if(!points?.length || values.some(v=>!Number.isFinite(v))) {setSvg('');setPreviewing(false);return;}
    setPreviewing(true);setError('');
    const timer=window.setTimeout(()=> {void editDirectControls(mode,values,true,points,doc.revision).then(result=>{if(generation.current===serial)setSvg(result.preview);}).catch(e=>{if(generation.current===serial){setError(String(e));setSvg('');}}).finally(()=>{if(generation.current===serial)setPreviewing(false);});},120);
    return()=>window.clearTimeout(timer);
  },[mode,values,points,doc.revision]);
  const changeMode=(next:typeof mode)=>{setMode(next);setValues(next==='corner'?[points?.[0]?.radius??0]:next==='position'&&points?.length===1?[points[0].x,points[0].y]:[0,0]);};
  return <dialog ref={ref} className="transform-dialog direct-control-dialog" onCancel={e=>{e.preventDefault();if(!busy)onClose();}}><form onSubmit={async e=>{e.preventDefault();if(!points?.length)return;setBusy(true);setError('');try{await onBusyChange?.(true);const result=await editDirectControls(mode,values,false,points,doc.revision);onApply(result.snapshot);await onBusyChange?.(false);onClose();}catch(cause){setError(String(cause));}finally{await onBusyChange?.(false).catch(()=>{});setBusy(false);}}}>
    <h3>{t.title}</h3>{points===null?<p>{t.loading}</p>:points.length===0?<p>{t.empty}</p>:<>
      <select aria-label={t.title} value={mode} disabled={busy} onChange={e=>changeMode(e.target.value as typeof mode)}><option value="position" disabled={points.length!==1}>{t.position}</option><option value="move">{t.move}</option><option value="corner" disabled={points.some(p=>!p.anchor)}>{t.corner}</option></select>
      {(mode==='corner'?[t.radius]:[t.x,t.y]).map((label,i)=><label className="direct-number" key={label}>{label}<input type="number" required step="any" min={mode==='corner'?0:undefined} max={mode==='corner'?4096:undefined} value={Number.isNaN(values[i])?'':values[i]} onChange={e=>setValues(v=>v.map((n,j)=>i===j?e.target.valueAsNumber:n))} disabled={busy}/></label>)}
      {mode==='corner'&&<><input type="range" aria-label={t.radius} min="0" max="256" step="0.5" value={Math.min(256,values[0]||0)} disabled={busy} onChange={e=>setValues([e.target.valueAsNumber])}/><p className="direct-corner-hint">{t.hint}</p></>}
      {svg&&<img className="direct-shape-preview" alt={t.preview} src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`}/>}</>}
    {error&&<p role="alert">{error}</p>}<div className="direct-actions"><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button disabled={busy||previewing||!points?.length||values.some(v=>!Number.isFinite(v))||!!error}>{t.apply}</button></div>
  </form></dialog>;
}
export const directControlLabels=labels;
