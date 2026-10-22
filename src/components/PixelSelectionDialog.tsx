import {useEffect,useRef,useState} from 'react';
import {isTauri} from '@tauri-apps/api/core';
import {pixelSelectionAction,type PixelSelectionRequest,type DocumentSnapshot} from '../bridge';
import type {Locale} from '../i18n';
import {pixelSelectionLabels} from '../pixel-selection-i18n';
import {ColorPickerPopover} from './ColorPickerPopover';
import './tool-settings.css';
import {useSelectionPreview} from './useSelectionPreview';
export function PixelSelectionDialog({locale,document:initial,action,onClose,onUpdate,onBusyChange}:{locale:Locale;document:DocumentSnapshot;action:string;onClose:()=>void;onUpdate:(doc:DocumentSnapshot)=>void;onBusyChange?:(busy:boolean)=>Promise<unknown>}){
 const t=pixelSelectionLabels[locale],ref=useRef<HTMLDialogElement>(null);const[doc,setDoc]=useState(initial),[name,setName]=useState(''),[saved,setSaved]=useState(initial.savedPixelSelections?.[0]??''),[operation,setOperation]=useState<'replace'|'add'|'subtract'|'intersect'>('replace');
 const mode=action.startsWith('modify:')?'modify':action;
 const [draft,setDraft]=useState<PixelSelectionRequest>({action:mode,kind:action.split(':')[1]??'sampled',amount:mode==='modify'?5:mode==='focusArea'?35:32,smooth:0,feather:0,contrast:0,shift:0,color:[0,0,0],inverse:false,allLayers:true,outputMask:false});
 const [transform,setTransform]=useState({x:0,y:0,sx:100,sy:100,angle:0});const[busy,setBusy]=useState(false),[error,setError]=useState('');
 const [previewEnabled,setPreviewEnabled]=useState(true);
 const preview=useSelectionPreview(mode==='refine',mode==='refine'&&previewEnabled&&!busy?draft:null,t.previewStale);
 async function close(){
  if(busy)return;
  setBusy(true);setError('');
  try{if(mode==='refine')await preview.clear();onClose();}
  catch(cause){setError(String(cause));setBusy(false);}
 }
 useEffect(()=>{if(!isTauri())ref.current?.show();},[]);
 const title=mode==='modify'?t.modify[['border','smooth','expand','contract','feather'].indexOf(draft.kind??'expand')]:t.items[({save:19,load:18,colorRange:8,focusArea:9,refine:12,grow:14,similar:15,transform:16}as Record<string,number>)[mode]??13];
 const change=(patch:Partial<PixelSelectionRequest>)=>setDraft(old=>({...old,...patch}));
 async function run(command?:'update'|'rename'|'delete'){
  setBusy(true);setError('');let closeAfter=false;
  try{
   await onBusyChange?.(true);
   if(mode==='refine')await preview.clear();
   let request:PixelSelectionRequest={...draft};
   if(mode==='save')request={action:command??'save',name:command?saved:name.trim(),newName:name.trim()};
   if(mode==='load')request={action:command??'load',name:saved,newName:name.trim(),mode:operation};
   if(mode==='transform'){
    const bounds=doc.selection?.regions.map(r=>r.bounds)??[];const left=Math.min(...bounds.map(b=>b[0])),top=Math.min(...bounds.map(b=>b[1])),right=Math.max(...bounds.map(b=>b[0]+b[2])),bottom=Math.max(...bounds.map(b=>b[1]+b[3]));
    const cx=Number.isFinite(left)?(left+right)/2:doc.width/2,cy=Number.isFinite(top)?(top+bottom)/2:doc.height/2,angle=transform.angle*Math.PI/180,a=Math.cos(angle)*transform.sx/100,b=Math.sin(angle)*transform.sx/100,c=-Math.sin(angle)*transform.sy/100,d=Math.cos(angle)*transform.sy/100;
    request={action:'transform',matrix:[a,b,c,d,cx-a*cx-c*cy+transform.x,cy-b*cx-d*cy+transform.y]};
   }
   const next=await pixelSelectionAction(request);setDoc(next);onUpdate(next);
   if(command){const chosen=command==='rename'?name.trim():(next.savedPixelSelections??[]).includes(saved)?saved:next.savedPixelSelections?.[0]??'';setSaved(chosen);}else closeAfter=true;
  }catch(cause){setError(String(cause));}finally{setBusy(false);try{await onBusyChange?.(false);}catch(cause){setError(String(cause));}}
  if(closeAfter)onClose();
 }
 function numeric(key:'amount'|'smooth'|'feather'|'contrast'|'shift',label:string,min:number,max:number){return <label>{label}<input aria-label={label} type="number" min={min} max={max} step={1} value={draft[key]??0} onChange={e=>{if(e.target.value!=='')change({[key]:Number(e.target.value)});}}/></label>;}
 return <dialog ref={ref} open={isTauri()} className="tool-settings-dialog pixel-selection-dialog" aria-labelledby="pixel-selection-title" onCancel={e=>{e.preventDefault();if(!busy)void close();}}><form onSubmit={e=>{e.preventDefault();void run();}}><h3 id="pixel-selection-title">{title}{!['save','load','transform'].includes(mode)&&<span className="timeline-info" tabIndex={0} title={t.analysis} aria-label={t.analysis}>ⓘ</span>}</h3><fieldset disabled={busy}>
 {['save','load'].includes(mode)&&<>
 {(mode==='load'||!!doc.savedPixelSelections?.length)&&<label>{t.saved}<select aria-label={t.saved} value={saved} onChange={e=>setSaved(e.target.value)}>{!doc.savedPixelSelections?.length&&<option value="">{t.empty}</option>}{doc.savedPixelSelections?.map(name=><option key={name}>{name}</option>)}</select></label>}
 <label>{t.name}<input aria-label={t.name} maxLength={100} value={name} onChange={e=>setName(e.target.value)} required={mode==='save'}/></label>
 {mode==='load'&&<label>{t.operation}<select aria-label={t.operation} value={operation} onChange={e=>setOperation(e.target.value as typeof operation)}>{(['replace','add','subtract','intersect'] as const).map((value,i)=><option key={value} value={value}>{t.operations[i]}</option>)}</select></label>}
 {!!doc.savedPixelSelections?.length&&<div className="saved-selection-actions"><button type="button" disabled={!saved||!doc.selection} onClick={()=>void run('update')}>{t.overwrite}</button><button type="button" disabled={!saved||!name.trim()} onClick={()=>void run('rename')}>{t.rename}</button><button type="button" disabled={!saved} onClick={()=>void run('delete')}>{t.remove}</button></div>}
 </>}
 {mode==='colorRange'&&<><label>{t.range}<select value={draft.kind} onChange={e=>change({kind:e.target.value})}>{['sampled','reds','greens','blues','highlights','midtones','shadows'].map((kind,i)=><option key={kind} value={kind}>{t.kinds[i]}</option>)}</select></label>{draft.kind==='sampled'&&<label>{t.color}<ColorPickerPopover locale={locale} label={t.color} color={draft.color??[0,0,0]} disabled={busy} onChange={color=>change({color})}/></label>}</>}
 {['colorRange','grow','similar','focusArea','modify'].includes(mode)&&numeric('amount',mode==='focusArea'?t.focus:mode==='modify'?t.radius:t.fuzziness,0,mode==='focusArea'?100:mode==='colorRange'?200:255)}
 {mode==='refine'&&<>{numeric('smooth',t.smooth,0,100)}{numeric('feather',t.feather,0,255)}{numeric('contrast',t.contrast,0,100)}{numeric('shift',t.shift,-100,100)}</>}
 {mode==='focusArea'&&numeric('feather',t.feather,0,255)}
 {['colorRange','focusArea','grow','similar'].includes(mode)&&<label><input type="checkbox" checked={draft.allLayers} onChange={e=>change({allLayers:e.target.checked})}/>{t.allLayers}</label>}
 {['colorRange','focusArea','refine'].includes(mode)&&<label><input type="checkbox" checked={draft.inverse} onChange={e=>change({inverse:e.target.checked})}/>{t.inverse}</label>}
 {['focusArea','refine'].includes(mode)&&<label>{t.output}<select aria-label={t.output} value={draft.outputMask?'mask':'selection'} onChange={e=>change({outputMask:e.target.value==='mask'})}><option value="selection">{t.selection}</option><option value="mask">{t.mask}</option></select></label>}
 {mode==='transform'&&(Object.keys(transform)as (keyof typeof transform)[]).map(key=><label key={key}>{t[key]}<input aria-label={t[key]} type="number" min={key==='sx'||key==='sy'?1:-32768} max={key==='sx'||key==='sy'?10000:32768} value={transform[key]} onChange={e=>{if(e.target.value!=='')setTransform(old=>({...old,[key]:Number(e.target.value)}));}}/></label>)}
 {mode==='refine'&&<label className="tool-preview"><input aria-label={t.preview} type="checkbox" checked={previewEnabled} onChange={e=>setPreviewEnabled(e.target.checked)}/>{t.preview}<span className="timeline-info" tabIndex={0} title={t.previewHint} aria-label={t.previewHint}>ⓘ</span><span role="status" aria-live="polite">{preview.pending?t.working:''}</span></label>}
 </fieldset>{(error||preview.error)&&<p role="alert">{error||preview.error}</p>}<footer><button type="button" disabled={busy} onClick={()=>void close()}>{t.close}</button><button disabled={busy||mode==='save'&&!name.trim()||mode==='load'&&!saved}>{busy?t.working:t.apply}</button></footer></form></dialog>;
}
