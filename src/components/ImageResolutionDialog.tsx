import {useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {Locale} from '../i18n';
import type {DocumentSnapshot} from '../bridge';
const labels={
 ja:{title:'画像解像度',resolution:'解像度 (ppi)',pixels:'ピクセル寸法',resample:'リサンプリング',hint:'リサンプリングを有効にすると、印刷サイズを保つようピクセル寸法を変更します。',apply:'適用',processing:'処理中…',cancel:'キャンセル'},
 en:{title:'Image Resolution',resolution:'Resolution (ppi)',pixels:'Pixel dimensions',resample:'Resample',hint:'Resampling changes pixel dimensions to preserve print size.',apply:'Apply',processing:'Processing…',cancel:'Cancel'},
 'zh-CN':{title:'图像分辨率',resolution:'分辨率 (ppi)',pixels:'像素尺寸',resample:'重新采样',hint:'启用重新采样后，像素尺寸将随分辨率变化以保持打印尺寸。',apply:'应用',processing:'正在处理…',cancel:'取消'},
};
export function ImageResolutionDialog({locale,document,documentId,onClose,onBusyChange}:{locale:Locale;document:DocumentSnapshot;documentId:number;onClose:()=>void;onBusyChange?:(busy:boolean)=>Promise<unknown>}){
 const t=labels[locale];const [value,setValue]=useState(String(document.resolution)),[resample,setResample]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');const n=Number(value),valid=Number.isInteger(n)&&n>=1&&n<=1200;
 const pending=useRef(false);
 const width=resample&&valid?Math.round(document.width*n/document.resolution):document.width,height=resample&&valid?Math.round(document.height*n/document.resolution):document.height;
 async function apply(){if(!valid||pending.current)return;pending.current=true;setBusy(true);setError('');let completed=false,locked=false;try{if(onBusyChange){await onBusyChange(true);locked=true;}await invoke('set_image_resolution_menu',{documentId,revision:document.revision,resolution:n,resample});completed=true;}catch(e){setError(String(e));}finally{if(locked){try{await onBusyChange?.(false);}catch(e){setError(String(e));completed=false;}}pending.current=false;setBusy(false);}if(completed)onClose();}
 return <dialog open className="transform-dialog image-resolution-dialog" onKeyDown={e=>{if((e.key==='Escape'||(e.metaKey&&e.key==='.' ))&&!pending.current&&!e.nativeEvent.isComposing){e.preventDefault();onClose();}}}><form onSubmit={e=>{e.preventDefault();void apply();}}><h3>{t.title} <span title={t.hint} aria-label={t.hint}>ⓘ</span></h3><p>{t.pixels}: {width} × {height}</p><label>{t.resolution}<input autoFocus aria-label={t.resolution} type="number" min={1} max={1200} step={1} value={value} onChange={e=>setValue(e.target.value)} disabled={busy}/></label><label className="tool-preview"><input type="checkbox" checked={resample} onChange={e=>setResample(e.target.checked)} disabled={busy}/>{t.resample}</label>{error&&<p role="alert">{error}</p>}<div className="dialog-actions"><button type="button" onClick={onClose} disabled={busy}>{t.cancel}</button><button className="resolution-default" type="submit" disabled={!valid||busy}>{busy?t.processing:t.apply}</button></div></form></dialog>;
}
