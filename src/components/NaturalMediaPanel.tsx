import {useEffect,useState} from 'react';
import {setPaperSimulation,type Brush,type DocumentSnapshot,type MaterialSettings,type PaperSettings} from '../bridge';
import type {Locale} from '../i18n';
import {defaultMaterial,defaultPaper,mediaLabels} from '../natural-media';
import {CompactSlider} from './CompactSlider';
import {PercentInput} from './BrushControls';
import {ColorPickerPopover} from './ColorPickerPopover';
const media=['graphite','charcoal','pastel','watercolor','gouache','oil'] as const;
const papers=['smooth','coldPress','rough','canvas','washi'] as const;
export function MaterialControls({brush,locale,enabled,onChange}:{brush:Brush;locale:Locale;enabled:boolean;onChange:(b:Brush)=>void}){
 const t=mediaLabels[locale],m=brush.material??defaultMaterial;
 const change=(patch:Partial<MaterialSettings>)=>onChange({...brush,material:{...m,...patch}});
 return <fieldset className="natural-media" disabled={!enabled}><legend>{t.material} <span title={t.hint} aria-label={t.hint}>ⓘ</span></legend>
 <label className="property-row"><span>{t.enable}</span><input type="checkbox" checked={m.enabled} onChange={e=>change({enabled:e.target.checked})}/></label>
 <label className="property-row"><span>{t.medium}</span><select aria-label={t.medium} disabled={!m.enabled} value={m.medium} onChange={e=>change({medium:e.target.value as MaterialSettings['medium']})}>{media.map((v,i)=><option key={v} value={v}>{t.media[i]}</option>)}</select></label>
 {(['loading','wetness','granulation','edge'] as const).map(key=><div className="diameter-row" key={key}><span>{t[key]}</span><CompactSlider aria-label={t[key]} min="0" max="100" value={Math.round(m[key]*100)} disabled={!m.enabled||(key==='edge'&&m.medium!=='watercolor')||(key==='wetness'&&!['watercolor','gouache','oil'].includes(m.medium))} onChange={e=>change({[key]:Number(e.target.value)/100})}/><PercentInput label={t[key]} value={m[key]} disabled={!m.enabled||(key==='edge'&&m.medium!=='watercolor')||(key==='wetness'&&!['watercolor','gouache','oil'].includes(m.medium))} onChange={v=>change({[key]:v})}/></div>)}
 </fieldset>
}
export function PaperControls({document,locale,enabled,onUpdate}:{document:DocumentSnapshot;locale:Locale;enabled:boolean;onUpdate:(d:DocumentSnapshot)=>void}){
 const t=mediaLabels[locale],paper=document.paper??defaultPaper;const[draft,setDraft]=useState(paper),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>setDraft(paper),[paper]);
 const apply=async()=>{setBusy(true);setError('');try{onUpdate(await setPaperSimulation(draft))}catch(e){setError(String(e))}finally{setBusy(false)}};
 return <fieldset className="natural-media" disabled={!enabled||busy}><legend>{t.paper} <span title={t.paperHint} aria-label={t.paperHint}>ⓘ</span></legend>
 <label className="property-row"><span>{t.enable}</span><input type="checkbox" checked={draft.enabled} onChange={e=>setDraft({...draft,enabled:e.target.checked})}/></label>
 <label className="property-row"><span>{t.kind}</span><select aria-label={t.kind} value={draft.kind} onChange={e=>setDraft({...draft,kind:e.target.value as PaperSettings['kind']})}>{papers.map((v,i)=><option key={v} value={v}>{t.kinds[i]}</option>)}</select></label>
 <div className="property-row"><span>{t.color}</span><ColorPickerPopover locale={locale} color={draft.color} label={t.color} onChange={color=>setDraft({...draft,color})}/></div>
 <label className="property-row"><span>{t.scale}</span><input aria-label={t.scale} type="number" min="0.25" max="8" step="0.25" value={draft.scale} onChange={e=>{const n=Number(e.target.value);if(Number.isFinite(n))setDraft({...draft,scale:Math.max(0.25,Math.min(8,n))})}}/></label>
 <div className="diameter-row"><span>{t.depth}</span><CompactSlider aria-label={t.depth} min="0" max="100" value={Math.round(draft.depth*100)} onChange={e=>setDraft({...draft,depth:Number(e.target.value)/100})}/><PercentInput label={t.depth} value={draft.depth} onChange={depth=>setDraft({...draft,depth})}/></div>
 <button type="button" onClick={()=>void apply()} disabled={JSON.stringify(draft)===JSON.stringify(paper)}>{t.apply}</button>{error&&<p role="alert">{error}</p>}
 </fieldset>
}
