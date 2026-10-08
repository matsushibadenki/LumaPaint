import { useEffect, useState } from 'react';
import { createScreentoneLayer, defaultLayerEffects, setLayerEffects, type DocumentSnapshot, type LayerSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import { defaultScreentone, screentoneLabels, toneDesigns, toneKinds, type Screentone, type ToneKind } from '../screentone';
import './screentone-panel.css';
export function ScreentonePanel({locale,layer,resolution,enabled,canCreate,onUpdate}:{locale:Locale;layer?:LayerSnapshot;resolution:number;enabled:boolean;canCreate:boolean;onUpdate:(snapshot:DocumentSnapshot)=>void}) {
  const t=screentoneLabels[locale];
  const signature=JSON.stringify(layer?.effects?.screentone ?? defaultScreentone(resolution));
  const [draft,setDraft]=useState<Screentone>(()=>JSON.parse(signature));
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState('');
  useEffect(()=>{setDraft(JSON.parse(signature));setError('');},[signature,layer?.id]);
  const editable=enabled && !busy;
  const patch=(value:Partial<Screentone>)=>setDraft(old=>({...old,...value}));
  async function commit(action:'apply'|'create'|'remove') {
    if (!editable || (action!=='create' && (!layer?.effects || layer.locked))) return;
    setBusy(true);setError('');
    try {
      const tone={...draft,dpi:resolution};
      if(action==='create') onUpdate(await createScreentoneLayer(tone));
      else if(layer) onUpdate(await setLayerEffects(layer.id,{...(layer.effects??defaultLayerEffects()),...(action==='remove'?{}:{enabled:true}),screentone:action==='remove'?null:tone}));
    } catch(cause) {setError(String(cause));} finally {setBusy(false);}
  }
  const number=(field:'frequency'|'density'|'size'|'angle'|'gradientEnd'|'extent'|'seed',label:string,min:number,max:number,step=1)=><ToneNumber key={field} value={draft[field]} label={label} min={min} max={max} step={step} onCommit={value=>patch({[field]:value})}/>;
  return <div className="screentone-panel" aria-busy={busy}>
    <header><h3>{t.title}</h3><p>{layer?.name}</p></header>
    <fieldset disabled={!editable}>
      <label className="screentone-row screentone-category"><span>{t.kind}</span><select value={draft.kind} aria-label={t.kind} onChange={e=>{const kind=e.currentTarget.value as ToneKind;patch({kind,variant:0,...(kind==='copy'?{density:100}:{}),...(kind==='color'?{color:[48,108,200] as [number,number,number]}:{color:[0,0,0] as [number,number,number]})});}}>{toneKinds.map((kind,i)=><option key={kind} value={kind}>{t.families[i]}</option>)}</select></label>
      <div className="screentone-designs" role="group" aria-label={t.design}>{toneDesigns(locale,draft.kind).map((name,i)=><button key={name} type="button" aria-pressed={draft.variant===i} onClick={()=>patch({variant:i})}>{name}</button>)}</div>
      {number('frequency',t.frequency,1,150)}{number('density',t.density,0,100)}{number('size',t.size,.1,4,.1)}{number('angle',t.angle,-360,360)}
      <details><summary>{t.design}</summary>{[t.x,t.y].map((label,i)=><ToneNumber key={label} label={label} value={draft.offset[i]} min={-100000} max={100000} step={1} onCommit={value=>{const offset:[number,number]=[...draft.offset];offset[i]=value;patch({offset});}}/>)}{number('seed',t.seed,0,65535)}</details>
      {draft.kind==='gradient' && <>{number('gradientEnd',t.end,0,100)}{number('extent',t.extent,1,100000)}</>}
      {draft.kind!=='white' && <label className="screentone-row"><span>{t.color}</span><input type="color" aria-label={t.color} value={'#'+draft.color.map(c=>c.toString(16).padStart(2,'0')).join('')} onChange={e=>patch({color:e.currentTarget.value.slice(1).match(/../g)!.map(c=>parseInt(c,16)) as [number,number,number]})}/></label>}
      {(['paper','luminance','inverted'] as const).map(field=><label className="screentone-check" key={field}><input type="checkbox" checked={field==='luminance'&&draft.kind==='copy'?true:draft[field]} disabled={field==='luminance'&&draft.kind==='copy'} onChange={e=>patch({[field]:e.currentTarget.checked})}/>{t[field]}</label>)}
    </fieldset>
    <p className="screentone-note">{t.resolution}: {resolution} dpi · {t.existing}</p>
    {(draft.kind==='white'||draft.kind==='copy'||draft.kind==='transfer')&&<p className="screentone-note">{t[draft.kind]}</p>}
    <div className="screentone-actions"><button disabled={!editable||!layer?.effects||layer.locked} onClick={()=>void commit('apply')}>{t.apply}</button><button disabled={!editable||!canCreate} onClick={()=>void commit('create')}>{t.create}</button><button disabled={!editable||!layer?.effects?.screentone||layer.locked} onClick={()=>void commit('remove')}>{t.remove}</button></div>
    <p className="screentone-note">{t.hint}</p>{error&&<p role="alert">{error}</p>}
  </div>;
}

function ToneNumber({value,label,min,max,step,onCommit}:{value:number;label:string;min:number;max:number;step:number;onCommit:(value:number)=>void}) {
  const [text,setText]=useState(String(value));
  useEffect(()=>setText(String(value)),[value]);
  const valid=text.trim()!==''&&Number.isFinite(Number(text))&&Number(text)>=min&&Number(text)<=max;
  const commit=()=>{if(valid)onCommit(Number(text));else setText(String(value));};
  return <label className="screentone-row"><span>{label}</span><input type="number" aria-label={label} aria-invalid={!valid} min={min} max={max} step={step} value={text} onChange={e=>setText(e.currentTarget.value)} onBlur={commit} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){setText(String(value));e.preventDefault();}}}/></label>;
}
