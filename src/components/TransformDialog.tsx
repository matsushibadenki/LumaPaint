import { useEffect, useRef, useState } from 'react';
import type { Locale } from '../i18n';
import type { TransformAction } from '../bridge';
export const transformLabels = {
  ja: { title:'変形', move:'移動', rotate:'回転', reflect:'リフレクト', scale:'拡大・縮小', shear:'シアー', individual:'個別に変形', reset:'バウンディングボックスのリセット', x:'水平方向 (px)', y:'垂直方向 (px)', width:'横倍率 (%)', height:'縦倍率 (%)', angle:'角度 (°)', axis:'反射軸 (°)：0=水平、90=垂直', apply:'適用', cancel:'キャンセル' },
  en: { title:'Transform', move:'Move', rotate:'Rotate', reflect:'Reflect', scale:'Scale', shear:'Shear', individual:'Transform Each', reset:'Reset Bounding Box', x:'Horizontal (px)', y:'Vertical (px)', width:'Horizontal (%)', height:'Vertical (%)', angle:'Angle (°)', axis:'Reflection axis (°): 0=horizontal, 90=vertical', apply:'Apply', cancel:'Cancel' },
  'zh-CN': { title:'变换', move:'移动', rotate:'旋转', reflect:'对称', scale:'缩放', shear:'倾斜', individual:'分别变换', reset:'重置定界框', x:'水平 (px)', y:'垂直 (px)', width:'水平 (%)', height:'垂直 (%)', angle:'角度 (°)', axis:'对称轴 (°)：0=水平，90=垂直', apply:'应用', cancel:'取消' },
};
export function TransformDialog({action,locale,onApply,onClose}: {action:TransformAction;locale:Locale;onApply:(values:number[])=>Promise<void>;onClose:()=>void}) {
  const t=transformLabels[locale]; const ref=useRef<HTMLDialogElement>(null);
  const [values,setValues]=useState(action==='scale'||action==='individual'?[100,100,0,0]:[0,0,0,0]);
  const [busy,setBusy]=useState(false);const [error,setError]=useState('');
  useEffect(()=>{ref.current?.showModal();},[]);
  const labels=action==='move'?[t.x,t.y]:action==='scale'?[t.width,t.height]:action==='individual'?[t.width,t.height,t.angle]:action==='reset'?[]:[action==='reflect'?t.axis:t.angle];
  return <dialog ref={ref} className="transform-dialog" onCancel={event=>{event.preventDefault();if(!busy)onClose();}}><form onSubmit={async event=>{event.preventDefault();setBusy(true);setError('');try{const v=[...values];if(action==='scale'||action==='individual'){v[0]/=100;v[1]/=100;}await onApply(v);onClose();}catch(e){setError(String(e));}finally{setBusy(false);}}}>
    <h3>{t[action]}</h3>{labels.map((label,i)=><label key={label} style={{display:'grid',gap:6,marginBottom:12}}>{label}<input type="number" required step="any" value={Number.isNaN(values[i])?'':values[i]} onChange={e=>setValues(v=>v.map((n,j)=>i===j?e.target.valueAsNumber:n))} disabled={busy}/></label>)}
    {error&&<p role="alert">{error}</p>}<div style={{display:'flex',gap:8,justifyContent:'flex-end'}}><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button disabled={busy||values.some(v=>!Number.isFinite(v))}>{t.apply}</button></div>
  </form></dialog>;
}
