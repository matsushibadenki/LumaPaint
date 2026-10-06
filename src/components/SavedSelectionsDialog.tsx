import {useEffect,useRef,useState} from 'react';
import {isTauri} from '@tauri-apps/api/core';
import {vectorSelectionAction,type DocumentSnapshot} from '../bridge';
import type {Locale} from '../i18n';
import {vectorSelectionLabels} from '../vector-selection-i18n';
import './tool-settings.css';
export function SavedSelectionsDialog({locale,document:initial,mode,onClose,onUpdate}:{locale:Locale;document:DocumentSnapshot;mode:'save'|'edit';onClose:()=>void;onUpdate:(doc:DocumentSnapshot)=>void}){
 const t=vectorSelectionLabels[locale],ref=useRef<HTMLDialogElement>(null);const [doc,setDoc]=useState(initial),[selected,setSelected]=useState(initial.savedVectorSelections[0]?.name??''),[name,setName]=useState(mode==='save'?'':initial.savedVectorSelections[0]?.name??''),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{if(!isTauri())ref.current?.show();},[]);
 const run=async(action:string)=>{setBusy(true);setError('');try{const value=await vectorSelectionAction({action,name:action==='save'?name.trim():selected,newName:name.trim()});setDoc(value);onUpdate(value);if(action==='save')onClose();else{const current=action==='rename'?name.trim():value.savedVectorSelections[0]?.name??'';setSelected(current);setName(current);}}catch(e){setError(String(e));}finally{setBusy(false);}};
 return <dialog ref={ref} open={isTauri()} className="tool-settings-dialog" onCancel={e=>{e.preventDefault();if(!busy)onClose();}} aria-labelledby="saved-selection-title"><form onSubmit={e=>{e.preventDefault();void run(mode==='save'?'save':'rename');}}><h3 id="saved-selection-title">{mode==='save'?t.save:t.edit}</h3><fieldset disabled={busy}>
 {mode==='edit'&&(doc.savedVectorSelections.length?<label>{t.title}<select value={selected} onChange={e=>{setSelected(e.target.value);setName(e.target.value);}}>{doc.savedVectorSelections.map(item=><option key={item.name}>{item.name}</option>)}</select></label>:<p>{t.empty}</p>)}
 <label>{t.name}<input autoFocus required maxLength={100} value={name} onChange={e=>setName(e.target.value)}/></label>
 {mode==='edit'&&<button type="button" disabled={!selected} onClick={()=>void run('delete')}>{t.remove}</button>}
 </fieldset>{error&&<p role="alert">{error}</p>}<footer><button type="button" disabled={busy} onClick={onClose}>{t.close}</button><button disabled={busy||!name.trim()||mode==='edit'&&!selected}>{mode==='save'?t.apply:t.rename}</button></footer></form></dialog>
}
