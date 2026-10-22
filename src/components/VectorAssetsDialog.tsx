import {useEffect,useRef,useState} from 'react';
import {isTauri} from '@tauri-apps/api/core';
import {vectorSelectionAction,type DocumentSnapshot,type VectorAssetKind} from '../bridge';
import type {Locale} from '../i18n';
import {vectorAssetLabels} from '../vector-assets-i18n';
import {Icon} from './Icon';
import './tool-settings.css';
const kinds:VectorAssetKind[]=['graphicStyle','symbolInstance','linkedText'];
export function VectorAssetsDialog({locale,document:initial,onClose,onUpdate}:{locale:Locale;document:DocumentSnapshot;onClose:()=>void;onUpdate:(doc:DocumentSnapshot)=>void}){
 const t=vectorAssetLabels[locale],dialog=useRef<HTMLDialogElement>(null);
 const first=(initial.vectorAssets??[]).find(item=>item.kind==='graphicStyle'&&item.selected)??(initial.vectorAssets??[]).find(item=>item.kind==='graphicStyle');
 const [doc,setDoc]=useState(initial),[kind,setKind]=useState<VectorAssetKind>('graphicStyle'),[id,setId]=useState(first?.id??''),[name,setName]=useState(first?.name??''),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const entries=(doc.vectorAssets??[]).filter(item=>item.kind===kind),asset=entries.find(item=>item.id===id);
 const selected=doc.selectedVectorObjects.length;
 const selectedObjects=doc.layers.flatMap(l=>l.objects).filter(o=>doc.selectedVectorObjects.includes(o.id));
 const canCreate=kind==='linkedText'?selected>=2&&doc.textObjects.filter(t=>doc.selectedVectorObjects.includes(t.id)&&!t.text.pointText&&t.text.boxHeight!=null&&t.text.writingMode==='horizontal').length===selected:selected===1&&(kind!=='symbolInstance'||selectedObjects.every(o=>o.kind!=='text'&&!o.imageFrame&&!o.clippingMask));
 useEffect(()=>{if(!isTauri())dialog.current?.show();},[]);
 async function run(action:string){
  if(busy)return;setBusy(true);setError('');
  try{
   const next=await vectorSelectionAction({action,criterion:kind,name:action==='assetCreate'?name.trim():id,newName:name.trim()});
   setDoc(next);onUpdate(next);
   const available=(next.vectorAssets??[]).filter(item=>item.kind===kind);
   const current=action==='assetCreate'?available.find(item=>item.name===name.trim()):available.find(item=>item.id===id);
   setId(current?.id??'');if(current)setName(current.name);
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }
 return <dialog ref={dialog} open={isTauri()} className="tool-settings-dialog vector-selection-dialog vector-assets-dialog" aria-labelledby="vector-assets-title" onCancel={e=>{e.preventDefault();if(!busy)onClose();}}><form onSubmit={e=>{e.preventDefault();void run('assetCreate');}}>
  <h3 id="vector-assets-title">{t.title} <span role="img" title={t.hint} aria-label={t.hint}>ⓘ</span></h3>
  <fieldset disabled={busy}>
   <div className="vector-asset-tabs" role="tablist" aria-label={t.title}>{kinds.map((value,index)=><button key={value} type="button" role="tab" aria-selected={kind===value} onClick={()=>{const first=(doc.vectorAssets??[]).find(item=>item.kind===value&&item.selected)??(doc.vectorAssets??[]).find(item=>item.kind===value);setKind(value);setId(first?.id??'');setName(first?.name??'');setError('');}}>{t.types[index]}</button>)}</div>
   <select size={5} className="vector-asset-list" aria-label={t.types[kinds.indexOf(kind)]} value={id} onChange={e=>{setId(e.target.value);setName(entries.find(item=>item.id===e.target.value)?.name??'');}}>{!entries.length?<option value="" disabled>{t.empty}</option>:entries.map(item=><option key={item.id} value={item.id}>{item.name} ({item.count}){item.overset?' ⚠':''}</option>)}</select>
   {asset?.overset&&<p role="status" title={t.overset}>{t.overset}</p>}
   <label>{t.name}<input maxLength={100} value={name} onChange={e=>setName(e.target.value)}/></label>
   <div className="vector-asset-actions">
    <button disabled={!canCreate||!name.trim()} title={t.create} aria-label={t.create}><Icon name="plus"/></button>
    <button type="button" disabled={!asset} title={t.select} onClick={()=>void run('assetSelect')}><Icon name="vectorSelect"/></button>
    <button type="button" disabled={!asset||!selected} onClick={()=>void run('assetApply')}>{t.apply}</button>
    {kind==='symbolInstance'&&<button type="button" disabled={!asset} title={t.insert} onClick={()=>void run('assetInsert')}><Icon name="animationDuplicate"/>{t.insert}</button>}
    {kind!=='linkedText'&&<button type="button" disabled={!asset||selected!==1} onClick={()=>void run('assetRedefine')}>{t.redefine}</button>}
    {kind==='linkedText'&&<button type="button" disabled={!asset} onClick={()=>void run('assetReverse')}>{t.reverse}</button>}
    <button type="button" disabled={!asset||!name.trim()||name.trim()===asset.name} onClick={()=>void run('assetRename')}>{t.rename}</button>
    <button type="button" disabled={!asset||(!selected&&kind!=='linkedText')} onClick={()=>void run('assetDetach')}>{t.detach}</button>
    <button type="button" disabled={!asset} title={t.remove} aria-label={t.remove} onClick={()=>void run('assetDelete')}><Icon name="animationDelete"/></button>
   </div>
  </fieldset>
  {error&&<p role="alert">{error}</p>}
  <footer><button type="button" disabled={busy} onClick={onClose}>{t.close}</button></footer>
 </form></dialog>;
}
