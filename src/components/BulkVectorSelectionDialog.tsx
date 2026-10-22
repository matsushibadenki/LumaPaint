import {useEffect,useRef,useState} from 'react';
import {isTauri} from '@tauri-apps/api/core';
import {vectorSelectionAction,type DocumentSnapshot,type VectorSelectionRequest} from '../bridge';
import type {Locale} from '../i18n';
import {sameCriteria,vectorSelectionLabels} from '../vector-selection-i18n';
import './tool-settings.css';
export function BulkVectorSelectionDialog({locale,document:doc,onClose,onUpdate,attributeOnly=false}:{locale:Locale;attributeOnly?:boolean;document:DocumentSnapshot;onClose:()=>void;onUpdate:(doc:DocumentSnapshot)=>void}){
 const t=vectorSelectionLabels[locale],ref=useRef<HTMLDialogElement>(null);
 const [criteria,setCriteria]=useState<string[]>([attributeOnly?'fill':'appearance']);
 const [matchAny,setMatchAny]=useState(false),[scope,setScope]=useState<'page'|'artboard'|'sameLayers'>('page');
 const [busy,setBusy]=useState(false),[error,setError]=useState('');
 const hasText=doc.textObjects.some(text=>doc.selectedVectorObjects.includes(text.id));
 useEffect(()=>{if(!isTauri())ref.current?.show();},[]);
 async function apply(){
  if(busy||!criteria.length)return;
  setBusy(true);setError('');
  const request:VectorSelectionRequest=attributeOnly?{action:'same',criterion:criteria[0]}:{action:'bulk',bulk:{criteria,matchAny,scope}};
  try{const next=await vectorSelectionAction(request);onUpdate(next);onClose();}
  catch(e){setError(String(e));}finally{setBusy(false);}
 }
 return <dialog ref={ref} open={isTauri()} className="tool-settings-dialog vector-selection-dialog" aria-labelledby="bulk-selection-title" onCancel={e=>{e.preventDefault();if(!busy)onClose();}}>
  <form onSubmit={e=>{e.preventDefault();void apply();}}>
   <h3 id="bulk-selection-title">{attributeOnly?t.unavailable[3]:t.bulk} <span role="img" aria-label={t.bulkHint} title={t.bulkHint}>ⓘ</span></h3>
   <fieldset disabled={busy}>
    {attributeOnly?<label>{t.conditions}<select value={criteria[0]} onChange={e=>setCriteria([e.target.value])}>{[4,6,7,5,2].map(i=><option key={i} value={sameCriteria[i]}>{t.sameNames[i]}</option>)}<option value="effects">{t.effects}</option></select></label>:<>
    <label>{t.scope}<select value={scope} onChange={e=>setScope(e.target.value as typeof scope)}>{(['page','artboard','sameLayers'] as const).map((value,i)=><option key={value} value={value}>{t.scopes[i]}</option>)}</select></label>
    <label>{t.matching}<select value={String(matchAny)} onChange={e=>setMatchAny(e.target.value==='true')}><option value="false">{t.allConditions}</option><option value="true">{t.anyCondition}</option></select></label>
    <fieldset className="bulk-selection-conditions"><legend>{t.conditions}</legend>{sameCriteria.map((criterion,i)=><label key={criterion}><input type="checkbox" checked={criteria.includes(criterion)} disabled={i>=9&&!hasText} onChange={e=>setCriteria(previous=>e.target.checked?[...previous,criterion]:previous.filter(key=>key!==criterion))}/><span>{t.sameNames[i]}</span></label>)}</fieldset></>}
   </fieldset>
   {error&&<p role="alert">{error}</p>}
   <footer><button type="button" disabled={busy} onClick={onClose}>{t.close}</button><button disabled={busy||!criteria.length||!doc.selectedVectorObjects.length}>{t.select}</button></footer>
  </form>
 </dialog>;
}
