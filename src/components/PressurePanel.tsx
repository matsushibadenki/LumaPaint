import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { isTauri, convertFileSrc } from '@tauri-apps/api/core';
import { defaultPressure, pressurePreset, pressurePreview, type Brush, type PressureResponse, type PressurePreview, type PressureSample, type PressureSettings, type PressureBrushKind, type PaperSettings } from '../bridge';
import type { Locale } from '../i18n';
import { pressureBrushNames, pressureLabels } from '../pressure-i18n';
import { PanelBlock } from './PanelBlock';
import { defaultMaterial } from '../natural-media';

const curves = { linear:[0,.25,.5,.75,1],soft:[0,.5,.75,.9,1],firm:[0,.1,.3,.6,1],smooth:[0,.15,.5,.85,1] } satisfies Record<string,PressureResponse['points']>;
function Curve({locale,kind,value,samples,enabled,onChange}:{locale:Locale;kind:'size'|'opacity';value:PressureResponse;samples?:[number,number][];enabled:boolean;onChange:(value:PressureResponse)=>void}) {
  const t=pressureLabels[locale],drag=useRef<{index:number;start:PressureResponse}|null>(null);
  const active=Object.entries(curves).find(([,points])=>points.every((p,i)=>p===value.points[i]))?.[0]??'custom';
  function change(index:number,next:number){const points=[...value.points] as PressureResponse['points'];points[index]=Math.max(points[index-1],Math.min(points[index+1],next));onChange({...value,points});}
  function move(event:PointerEvent<SVGSVGElement>){if(!drag.current)return;const rect=event.currentTarget.getBoundingClientRect();change(drag.current.index,Math.round(Math.max(0,Math.min(1,(86-(event.clientY-rect.top)/rect.height*100)/80))*100)/100);}
  const path=(samples??value.points.map((v,i)=>[i/4,v] as [number,number])).map(([x,y])=>`${12+x*176},${86-y*80}`).join(' ');
  return <PanelBlock id={`brush.pressure.${kind}`} title={t[kind]} defaultOpen={kind==='size'}>
    <div className="pressure-response-heading"><label><input type="checkbox" checked={value.enabled} disabled={!enabled} onChange={event=>onChange({...value,enabled:event.target.checked})}/>{t.enabled}</label><select aria-label={`${t[kind]} · ${t.curve}`} disabled={!enabled} value={active} onChange={event=>onChange({...value,points:[...curves[event.target.value as keyof typeof curves]]})}><option value="custom" disabled>{t.custom}</option>{Object.keys(curves).map(key=><option key={key} value={key}>{t[key as keyof typeof curves]}</option>)}</select></div>
    <svg className="pressure-response-curve" viewBox="0 0 200 100" role="group" aria-label={`${t[kind]} · ${t.curve}`} onPointerMove={move} onPointerUp={event=>{drag.current=null;if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);}} onPointerCancel={()=>{if(drag.current){onChange(drag.current.start);drag.current=null;}}} onLostPointerCapture={()=>{drag.current=null;}}>
      {[0,.25,.5,.75,1].map(v=><path key={v} className="pressure-response-grid" d={`M12 ${86-v*80}H188M${12+v*176} 6V86`}/>)}
      <polyline points={path} className="pressure-response-line"/>
      {[1,2,3].map(index=><circle key={index} cx={12+index*44} cy={86-value.points[index]*80} r="4" role="slider" tabIndex={enabled?0:-1} aria-label={`${t[kind]} · ${t.input} ${index*25}%`} aria-valuemin={Math.round(value.points[index-1]*100)} aria-valuemax={Math.round(value.points[index+1]*100)} aria-valuenow={Math.round(value.points[index]*100)} aria-disabled={!enabled}
        onPointerDown={event=>{if(!enabled||event.button!==0)return;event.preventDefault();event.currentTarget.focus();drag.current={index,start:structuredClone(value)};event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId);}}
        onKeyDown={event=>{if(!enabled)return;const step=event.shiftKey?.1:.01;const next=event.key==='ArrowUp'?value.points[index]+step:event.key==='ArrowDown'?value.points[index]-step:event.key==='Home'?value.points[index-1]:event.key==='End'?value.points[index+1]:null;if(next!==null){event.preventDefault();event.stopPropagation();change(index,Math.round(next*100)/100);}}}/>)}
      <text x="12" y="98">0</text><text x="188" y="98" textAnchor="end">100%</text>
    </svg>
    <label className="pressure-minimum"><span>{t.minimum}</span><input type="number" aria-label={`${t[kind]} · ${t.minimum}`} min="0" max="100" step="1" disabled={!enabled} value={Math.round(value.minimum*100)} onChange={event=>{const n=event.target.valueAsNumber;if(Number.isFinite(n))onChange({...value,minimum:Math.max(0,Math.min(100,n))/100});}}/><span>%</span></label>
  </PanelBlock>;
}
export function PressurePanel({locale,brush,enabled,onChange,paper}:{paper?:PaperSettings;locale:Locale;brush:Brush;enabled:boolean;onChange:(brush:Brush)=>void}) {
  const t=pressureLabels[locale],settings=brush.pressure??defaultPressure;
  const [preview,setPreview]=useState<PressurePreview|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false),[saved,setSaved]=useState(false),[mouse,setMouse]=useState(.5);
  const current=useRef(brush);current.current=brush;
  const owner=useRef(crypto.randomUUID());
  const live=useRef(true),queue=useRef<Promise<unknown>>(Promise.resolve()),pending=useRef<PressureSample[]>([]),frame=useRef(0),drawing=useRef(false),count=useRef(0),selection=useRef(0);
  const signature=JSON.stringify({brush,paper});
  const paperRef=useRef(paper);paperRef.current=paper;
  function request(action:string,samples:PressureSample[]=[]){
    const base=current.current;
    const snapshot=paperRef.current?{...base,material:{...defaultMaterial,...base.material,paper:paperRef.current}}:base;
    const result=queue.current.then(()=>pressurePreview(action,snapshot,samples,owner.current));
    queue.current=result.then(value=>{if(live.current&&current.current===base){setPreview({...value,url:value.url.startsWith('pressure-preview:')?convertFileSrc(value.url.split('/').at(-1)!,'pressure-preview'):value.url});setError('');}},cause=>{if(live.current)setError(String(cause));});
    return result;
  }
  useEffect(()=>{live.current=true;if(isTauri())void request('open').catch(()=>{});return()=>{live.current=false;++selection.current;clearTimeout(frame.current);pending.current=[];if(drawing.current){drawing.current=false;void request('cancel').catch(()=>{});}};},[]);
  useEffect(()=>{if(!isTauri())return;const timer=setTimeout(()=>{void request('refresh').catch(()=>{});},80);return()=>clearTimeout(timer);},[signature]);
  async function preset(action:'get'|'save'|'reset',id:PressureBrushKind=settings.brushId){const version=++selection.current;const snapshot=current.current;setBusy(true);setSaved(false);setError('');try{const value=await pressurePreset(id,action,action==='save'?settings:undefined);if(live.current&&version===selection.current&&current.current===snapshot){onChange({...current.current,pressure:value});setSaved(action==='save');}}catch(cause){if(live.current)setError(String(cause));}finally{if(live.current&&version===selection.current)setBusy(false);}}
  function change(patch:Partial<PressureSettings>){setSaved(false);onChange({...brush,pressure:{...settings,...patch}});}
  function sample(event:PointerEvent<HTMLDivElement>):PressureSample {const rect=event.currentTarget.getBoundingClientRect();return {x:Math.max(0,Math.min(256,(event.clientX-rect.left)/rect.width*256)),y:Math.max(0,Math.min(128,(event.clientY-rect.top)/rect.height*128)),pressure:event.pointerType==='pen'?Math.max(0,Math.min(1,event.pressure)):mouse};}
  function flush(){clearTimeout(frame.current);frame.current=0;const samples=pending.current.splice(0,32);if(samples.length)void request('append',samples).catch(()=>{});}
  return <PanelBlock id="brush.pressure" title={<>{t.title}<span className="pressure-info" title={`${t.hint} ${t.scope}`} aria-label={t.hint}>ⓘ</span></>}>
    <div className="pressure-preset"><label><span>{t.brush}</span><select aria-label={t.brush} value={settings.brushId} disabled={!enabled||busy} onChange={event=>void preset('get',event.target.value as PressureBrushKind)}>{Object.entries(pressureBrushNames[locale]).map(([id,label])=><option key={id} value={id}>{label}</option>)}</select></label><div><button type="button" disabled={!enabled||busy} title={saved?t.saved:t.save} aria-label={saved?t.saved:t.save} onClick={()=>void preset('save')}>{saved?'✓':'＋'}</button><button type="button" disabled={!enabled||busy} title={t.reset} aria-label={t.reset} onClick={()=>void preset('reset')}>↺</button></div></div>
    <Curve locale={locale} kind="size" value={settings.size} samples={preview?.sizeCurve} enabled={enabled&&!busy} onChange={size=>change({size})}/>
    <Curve locale={locale} kind="opacity" value={settings.opacity} samples={preview?.opacityCurve} enabled={enabled&&!busy} onChange={opacity=>change({opacity})}/>
    <PanelBlock id="brush.pressure.pad" title={<>{t.pad}<span className="pressure-info" title={t.padHint} aria-label={t.padHint}>ⓘ</span></>}>
      <div className="pressure-pad-actions"><button type="button" disabled={!isTauri()||busy} aria-label={t.demo} title={t.demo} onClick={()=>void request('demo').catch(()=>{})}>∿</button><button type="button" disabled={!isTauri()||busy} aria-label={t.clear} title={t.clear} onClick={()=>void request('clear').catch(()=>{})}>⌫</button><output aria-live="off">{preview?`${Math.round(preview.pressure*100)}% · ${Number(preview.size.toFixed(1))}px · ${Math.round(preview.opacity*100)}%`:''}</output></div>
      <div className="pressure-pad" role="img" aria-label={t.pad} aria-disabled={!isTauri()||!enabled} onPointerDown={event=>{if(drawing.current||event.button!==0||!enabled||!isTauri())return;event.preventDefault();drawing.current=true;count.current=1;pending.current=[];event.currentTarget.setPointerCapture(event.pointerId);void request('begin',[sample(event)]).catch(()=>{});}}
        onPointerMove={event=>{if(!drawing.current||!event.currentTarget.hasPointerCapture(event.pointerId)||count.current>=250)return;pending.current.push(sample(event));++count.current;if(pending.current.length>=32)flush();else if(!frame.current)frame.current=window.setTimeout(flush,40);}}
        onPointerUp={event=>{if(!drawing.current||!event.currentTarget.hasPointerCapture(event.pointerId))return;flush();drawing.current=false;void request('end',[sample(event)]).catch(()=>{});if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);}}
        onPointerCancel={()=>{if(!drawing.current)return;clearTimeout(frame.current);frame.current=0;pending.current=[];drawing.current=false;void request('cancel').catch(()=>{});}}
        onLostPointerCapture={()=>{if(!drawing.current)return;clearTimeout(frame.current);frame.current=0;pending.current=[];drawing.current=false;void request('cancel').catch(()=>{});}}>
        {preview&&<img src={preview.url} alt="" draggable={false}/>}
      </div>
      <label className="pressure-mouse"><span>{t.mouse}</span><input type="range" aria-label={t.mouse} min="0" max="100" value={Math.round(mouse*100)} onChange={event=>setMouse(Number(event.target.value)/100)}/><output>{Math.round(mouse*100)}%</output></label>
      {!isTauri()&&<p className="type-hint">{t.unavailable}</p>}
    </PanelBlock>
    {error&&<p className="panel-error" role="alert">{error}</p>}
  </PanelBlock>;
}
