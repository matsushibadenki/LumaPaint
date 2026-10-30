import {useEffect,useRef,useState} from 'react';
import {publicationTextContext,publicationTextAction,type PublicationTextContext,type PublicationTextRequest,type PageTextFrame,type DocumentSnapshot} from '../bridge';
import type {Locale} from '../i18n';
import {publicationTextLabels} from '../publication-text-i18n';
import {Icon} from './Icon';
const key=(target:PageTextFrame)=>JSON.stringify([target.pageId,target.objectId]);
export function PublicationTextView({locale,context,busy,error,onAction,onRefresh}:{locale:Locale;context:PublicationTextContext|null;busy:boolean;error:string;onAction:(request:PublicationTextRequest)=>void;onRefresh:()=>void}){
 const t=publicationTextLabels[locale];
 const [threadId,setThreadId]=useState(''),[name,setName]=useState(''),[targets,setTargets]=useState<PageTextFrame[]>([]);
 const frames=context?.frames??[],threads=context?.threads??[];
 const previousThreads=useRef(new Set<string>());
 useEffect(()=>{const chosen=context?.threads.find(thread=>thread.id===threadId)??context?.threads.find(thread=>!previousThreads.current.has(thread.id));setThreadId(chosen?.id??'');setTargets(chosen?.frames??context?.frames.filter(f=>f.selected&&f.available).map(f=>f.target)??[]);previousThreads.current=new Set(context?.threads.map(thread=>thread.id)??[]);},[context]);
 const selected=targets.map(target=>frames.find(f=>key(f.target)===key(target)));
 const valid=!!context&&!threadId&&targets.length>=2&&targets.length<=64&&!!name.trim()&&name.trim().length<=100&&selected.every(f=>f?.available&&f.writingMode===selected[0]?.writingMode&&f.resolution===selected[0]?.resolution);
 const ordered=[...targets.map(target=>frames.find(f=>key(f.target)===key(target))).filter(f=>f!=null),...frames.filter(f=>!targets.some(target=>key(f.target)===key(target)))];
 function move(index:number,delta:number){setTargets(previous=>{const next=[...previous];[next[index],next[index+delta]]=[next[index+delta],next[index]];return next;});}
 function create(){if(context&&valid&&!busy)onAction({revision:context.revision,action:'create',name:name.trim(),frames:targets});}
 return <section className="publication-text-panel" aria-label={t.title}>
  <div className="publication-text-heading"><label>{t.story}<select value={threadId} disabled={busy||!context} onChange={e=>{const id=e.target.value;setThreadId(id);setTargets(threads.find(thread=>thread.id===id)?.frames??[]);}}><option value="">{t.newStory}</option>{threads.map(thread=><option key={thread.id} value={thread.id}>{thread.name} ({thread.frames.length}){thread.overset?' ⚠':''}</option>)}</select></label><button type="button" disabled={busy} title={t.refresh} aria-label={t.refresh} onClick={onRefresh}><Icon name="undo"/></button>{threads.find(thread=>thread.id===threadId)?.overset&&<span role="img" title={t.overset} aria-label={t.overset}>⚠</span>}<span role="img" title={t.hint} aria-label={t.hint}>ⓘ</span></div>
  {!context&&busy&&<p role="status">{t.loading}</p>}
  <div className="publication-frame-list" aria-label={t.title}>
   {!frames.length&&context&&<p>{t.empty}</p>}
   {ordered.map(frame=>{const id=key(frame.target),index=targets.findIndex(target=>key(target)===id);return <div key={id} className="publication-frame-row">
    <label title={!frame.available?t.unavailable:frame.name}><input type="checkbox" checked={index>=0} disabled={busy||!!threadId||!frame.available||(index<0&&targets.length>=64)} aria-label={`${t.page} ${frame.pageNumber} · ${frame.name}`} onChange={e=>setTargets(previous=>e.target.checked?[...previous,frame.target]:previous.filter(target=>key(target)!==id))}/><span className="publication-frame-order">{index>=0?index+1:'·'}</span><Icon name={frame.writingMode==='vertical'?'textFrameVertical':'textFrame'}/><span className="publication-frame-name">{frame.pageNumber} · {frame.name}</span><small>{frame.characters.toLocaleString(locale)}</small></label>
    {index>=0&&!threadId&&<span className="publication-frame-move"><button type="button" disabled={busy||index===0} aria-label={`${t.up}: ${frame.pageNumber} · ${frame.name}`} title={t.up} onClick={()=>move(index,-1)}>↑</button><button type="button" disabled={busy||index===targets.length-1} aria-label={`${t.down}: ${frame.pageNumber} · ${frame.name}`} title={t.down} onClick={()=>move(index,1)}>↓</button></span>}
   </div>;})}
  </div>
  {!threadId&&<label>{t.name}<input maxLength={100} value={name} disabled={busy||!context} onChange={e=>setName(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();create();}}}/></label>}
  <div className="publication-text-actions"><button type="button" disabled={busy||!valid} title={t.create} aria-label={t.create} onClick={create}><Icon name="links"/></button><button type="button" disabled={busy||!threadId||!context} title={t.detach} aria-label={t.detach} onClick={()=>context&&onAction({revision:context.revision,action:'detach',id:threadId})}><Icon name="minus"/></button></div>
  {error&&<p role="alert">{error}</p>}
 </section>;
}
export function PublicationTextPanel({locale,onUpdate}:{locale:Locale;onUpdate:(document:DocumentSnapshot)=>void}){
 const [context,setContext]=useState<PublicationTextContext|null>(null),[busy,setBusy]=useState(true),[error,setError]=useState('');
 useEffect(()=>{let active=true;publicationTextContext().then(value=>{if(active)setContext(value);}).catch(e=>{if(active)setError(String(e));}).finally(()=>{if(active)setBusy(false);});return()=>{active=false;};},[]);
 async function refresh(){if(busy)return;setBusy(true);setError('');try{setContext(await publicationTextContext());}catch(e){setError(String(e));}finally{setBusy(false);}}
 async function run(request:PublicationTextRequest){if(busy||!context)return;setBusy(true);setError('');try{const result=await publicationTextAction(context.documentId,request);setContext(result.context);onUpdate(result.document);}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <PublicationTextView locale={locale} context={context} busy={busy} error={error} onAction={request=>void run(request)} onRefresh={()=>void refresh()}/>;
}
