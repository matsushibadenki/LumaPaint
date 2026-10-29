import {useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {Locale} from '../i18n';
import type {DocumentSnapshot} from '../bridge';
const labels={
 ja:{title:'カンバスサイズ',width:'幅 (px)',height:'高さ (px)',anchor:'基準位置',apply:'適用',cancel:'キャンセル',point:['左上','上','右上','左','中央','右','左下','下','右下']},
 en:{title:'Canvas Size',width:'Width (px)',height:'Height (px)',anchor:'Anchor',apply:'Apply',cancel:'Cancel',point:['Top left','Top','Top right','Left','Center','Right','Bottom left','Bottom','Bottom right']},
 'zh-CN':{title:'画布大小',width:'宽度 (px)',height:'高度 (px)',anchor:'定位',apply:'应用',cancel:'取消',point:['左上','上','右上','左','居中','右','左下','下','右下']},
};
export function CanvasSizeDialog({locale,document,documentId,onClose}:{locale:Locale;document:DocumentSnapshot;documentId:number;onClose:()=>void}){
 const t=labels[locale];const [width,setWidth]=useState(String(document.width)),[height,setHeight]=useState(String(document.height)),[anchor,setAnchor]=useState(4),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const w=Number(width),h=Number(height),valid=Number.isInteger(w)&&w>=1&&w<=8192&&Number.isInteger(h)&&h>=1&&h<=8192;
 async function apply(){if(!valid||busy)return;setBusy(true);setError('');const x=Math.floor((document.width-w)*(anchor%3)/2),y=Math.floor((document.height-h)*Math.floor(anchor/3)/2);try{await invoke('resize_canvas_menu',{documentId,revision:document.revision,x,y,width:w,height:h});onClose();}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <dialog open className="transform-dialog"><form onSubmit={e=>{e.preventDefault();void apply();}}><h3>{t.title}</h3><label>{t.width}<input type="number" min={1} max={8192} step={1} value={width} onChange={e=>setWidth(e.target.value)} disabled={busy}/></label><label>{t.height}<input type="number" min={1} max={8192} step={1} value={height} onChange={e=>setHeight(e.target.value)} disabled={busy}/></label><fieldset><legend>{t.anchor}</legend><div style={{display:'grid',gridTemplateColumns:'repeat(3,32px)',gap:2}}>{t.point.map((name,i)=><button key={name} type="button" aria-label={name} title={name} aria-pressed={anchor===i} onClick={()=>setAnchor(i)} disabled={busy}>{anchor===i?'●':'·'}</button>)}</div></fieldset>{error&&<p role="alert">{error}</p>}<div className="dialog-actions"><button type="button" onClick={onClose} disabled={busy}>{t.cancel}</button><button type="submit" disabled={!valid||busy}>{t.apply}</button></div></form></dialog>;
}
