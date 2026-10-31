import { PanelBlock, panelBlockLabels } from './PanelBlock';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Locale } from '../i18n';
import { motionBehavior } from '../motion-behavior';
import { motionLabels } from '../motion-i18n';
import { curveDefaults, newMotion, type Curve, type MotionCatalog, type MotionClip, type MotionPreset, type MotionState, type VariableFont, defaultAxes } from '../motion-types';
import { ColorPickerPopover } from './ColorPickerPopover';
import { parseSignedNumber } from './SignedNumberInput';
import './motion-panel.css';

const curveRanges:Record<string,[number,number,number]>={exponent:[1,8,.1],overshoot:[0,8,.01],amplitude:[1,5,.01],period:[.05,2,.01],restitution:[.1,.95,.01],stiffness:[1,2000,1],damping:[.1,200,.1],mass:[.1,20,.1],count:[1,120,1],strength:[0,2,.01],points:[2,120,1],plateau:[.05,.95,.01],ratio:[1.01,100,.01],cycles:[1,30,.1],decay:[.1,30,.1]};
type Transport = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
let catalogPromise:Promise<MotionCatalog>|undefined;
let fontsPromise:Promise<VariableFont[]>|undefined;
const defaultTransport:Transport=(command,args)=>invoke(command,args);
function MotionNumber({label,value,change,min,max,step,unit,disabled,reportInvalid}:{label:string;value:number;change:(value:number)=>void;min:number;max:number;step:number;unit?:string;disabled:boolean;reportInvalid:(id:symbol,invalid:boolean)=>void}) {
  const id=useRef(Symbol(label));
  const [text,setText]=useState<string|null>(null);
  const parsed=text===null?value:parseSignedNumber(text);
  const valid=parsed!==null&&parsed>=min&&parsed<=max;
  useEffect(()=>()=>reportInvalid(id.current,false),[reportInvalid]);
  const update=(next:string)=>{setText(next);const number=parseSignedNumber(next);const ok=number!==null&&number>=min&&number<=max;reportInvalid(id.current,!ok);if(ok)change(number);};
  const reset=()=>{setText(null);reportInvalid(id.current,false);};
  return <label className="motion-number"><span>{label}</span><span><input aria-label={label} type="text" inputMode="decimal" role="spinbutton" aria-valuemin={min} aria-valuemax={max} aria-valuenow={valid?parsed:undefined} aria-invalid={!valid} value={text??String(value)} disabled={disabled} onChange={event=>update(event.currentTarget.value)} onBlur={reset} onKeyDown={event=>{if(event.key==='ArrowUp'||event.key==='ArrowDown'){event.preventDefault();const next=Math.max(min,Math.min(max,(valid?parsed:value)+(event.key==='ArrowUp'?step:-step)));update(String(Number(next.toPrecision(12))));}else if(event.key==='Enter')event.currentTarget.blur();else if(event.key==='Escape'){event.preventDefault();reset();event.currentTarget.blur();}}}/>{unit&&<small>{unit}</small>}</span></label>;
}
export function MotionPanel({locale,transport=defaultTransport}:{locale:Locale;transport?:Transport}) {
  const t=motionLabels[locale];
  const [fonts,setFonts]=useState<VariableFont[]>([]);
  const [catalog,setCatalog]=useState<MotionCatalog|null>(null);
  const [state,setState]=useState<MotionState>({frame:0,playing:false,preview:false,data:null});
  const [layerId,setLayerId]=useState(''),[category,setCategory]=useState(''),[query,setQuery]=useState('');
  const [tab,setTab]=useState<'catalog'|'clips'>('catalog'),[limit,setLimit]=useState(48);
  const [draft,setDraft]=useState<MotionClip|null>(null),[selectedId,setSelectedId]=useState('');
  const [busy,setBusy]=useState(false),[error,setError]=useState(''),[path,setPath]=useState('');
  const invalidNumbers=useRef(new Set<symbol>());
  const [hasInvalidNumber,setHasInvalidNumber]=useState(false);
  const reportInvalid=useCallback((id:symbol,invalid:boolean)=>{if(invalid)invalidNumbers.current.add(id);else invalidNumbers.current.delete(id);setHasInvalidNumber(invalidNumbers.current.size>0);},[]);
  const alive=useRef(false),serial=useRef<Promise<unknown>>(Promise.resolve()),frame=useRef(0),pendingSeek=useRef<number|null>(null),seeking=useRef(false);
  const currentDocument=useRef<number|undefined>(undefined);
  currentDocument.current=state.data?.documentId;
  const request=useCallback((command:Record<string,unknown>,quiet=false):Promise<boolean>=>{
    const run=async()=>{
      if(!alive.current)return false;
      if(!quiet){setBusy(true);setError('');}
      try{
        const next=await transport<MotionState>('timeline',{request:command});
        if(alive.current)setState(old=>next.data===null&&(next.dataUnchanged??command.action==='tick')&&old.frame===next.frame&&old.playing===next.playing&&old.preview===next.preview?old:({...next,data:(next.dataUnchanged??command.action==='tick')?(next.data??old.data):next.data}));
        return true;
      }catch(cause){if(alive.current){setError(String(cause));if(command.action==='get'||command.action==='refresh')setState({frame:0,playing:false,preview:false,data:null});}return false;}
      finally{if(alive.current&&!quiet)setBusy(false);}
    };
    const next=serial.current.then(run,run);serial.current=next;return next;
  },[transport]);
  useEffect(()=>{
    alive.current=true;
    const load=transport===defaultTransport?(catalogPromise??=transport<MotionCatalog>('motion_catalog')):transport<MotionCatalog>('motion_catalog');
    void load.then(value=>{if(alive.current)setCatalog(value);}).catch(cause=>{catalogPromise=undefined;if(alive.current)setError(String(cause));});
    void request({action:'get'});
    let canceled=false,timeout:ReturnType<typeof setTimeout>|undefined;
    const unsubscribe=transport===defaultTransport?getCurrentWebviewWindow().listen('documents-changed',()=>{clearTimeout(timeout);timeout=setTimeout(()=>{if(!canceled)void request({action:'refresh'},true);},40);}).catch(()=>()=>{}):Promise.resolve(()=>{});
    const timelineUnsubscribe=transport===defaultTransport?getCurrentWebviewWindow().listen('timeline-changed',()=>{if(!canceled)void request({action:'refresh'},true);}).catch(()=>()=>{}):Promise.resolve(()=>{});
    return()=>{const documentId=currentDocument.current;alive.current=false;canceled=true;clearTimeout(timeout);cancelAnimationFrame(frame.current);void unsubscribe.then(stop=>stop());void timelineUnsubscribe.then(stop=>stop());if(documentId!==undefined)void serial.current.then(()=>transport('timeline',{request:{action:'stopMotion',documentId}})).catch(()=>{});};
  },[request,transport]);
  useEffect(()=>{
    if(!state.playing)return;
    let canceled=false;
    const tick=async()=>{await request({action:'tick'},true);if(!canceled)frame.current=requestAnimationFrame(()=>void tick());};
    frame.current=requestAnimationFrame(()=>void tick());return()=>{canceled=true;cancelAnimationFrame(frame.current);};
  },[state.playing,request]);
  const documentId=state.data?.documentId;
  const layers=state.data?.layers.filter(layer=>!layer.pixel)??[];
  useEffect(()=>{
    if(state.data)setLayerId(old=>state.data!.layers.some(layer=>layer.id===state.data!.selectedLayer&&!layer.pixel)?state.data!.selectedLayer:state.data!.layers.some(layer=>layer.id===old&&!layer.pixel)?old:state.data!.layers.find(layer=>!layer.pixel)?.id??'');
  },[state.data]);
  useEffect(()=>{setDraft(null);setSelectedId('');setPath('');setError('');},[documentId,layerId]);
  const layer=layers.find(item=>item.id===layerId);
  const preset=catalog?.presets.find(item=>item.id===draft?.preset);
  useEffect(()=>{if(!preset?.operations.includes('fontAxes'))return;let canceled=false;const load=transport===defaultTransport?(fontsPromise??=transport<VariableFont[]>('motion_fonts')):transport<VariableFont[]>('motion_fonts');void load.then(value=>{if(!canceled)setFonts(value);}).catch(cause=>{fontsPromise=undefined;if(!canceled)setError(String(cause));});return()=>{canceled=true;};},[preset,transport]);
  const matches=useMemo(()=>{
    const search=query.trim().toLocaleLowerCase();return catalog?.presets.filter(item=>(!category||item.category===category)&&(!search||item.id.includes(search)||Object.values(item.names).some(name=>name.toLocaleLowerCase().includes(search))))??[];
  },[catalog,category,query]);
  useEffect(()=>setLimit(48),[category,query]);
  const title=(item:MotionPreset)=>item.names[locale]??item.names.en;
  const disabled=busy||state.playing||!layer||layer.locked;
  const chosen=layer?.motions.find(clip=>clip.id===selectedId);
  const edit=(patch:Partial<MotionClip>)=>setDraft(old=>old?{...old,...patch}:old);
  function selectPreset(item:MotionPreset) {
    const next=newMotion(item,state.data?.fps??24,state.frame);
    setSelectedId('');setDraft(next);setPath('');
  }
  function selectClip(clip:MotionClip) {setSelectedId(clip.id);setDraft(structuredClone(clip));setPath(clip.path.map(point=>point.join(', ')).join('\n'));}
  const apply=async(duplicate=false)=>{
    if(!draft||!state.data||!layer||invalidNumbers.current.size)return;
    const points:[number,number][]=[];
    for(const line of path.split('\n').filter(line=>line.trim())) {
      const components=line.split(',').map(value=>value.trim());
      const pair=components.map(Number);
      if(pair.length!==2||components.some(value=>!value)||!pair.every(Number.isFinite)){setError(t.invalidPath);return;}
      points.push([pair[0],pair[1]]);
    }
    const clip={...draft,id:duplicate?crypto.randomUUID():draft.id,path:points};
    if(!await request({action:'setMotion',documentId:state.data.documentId,layer:layer.id,clip}))return;
    setSelectedId(clip.id);setDraft(clip);setTab('clips');
  };
  const seek=(value:number)=>{
    pendingSeek.current=Math.max(0,Math.min((state.data?.duration??1)-1,Math.round(value)));
    if(seeking.current)return;seeking.current=true;
    void(async()=>{try{while(pendingSeek.current!==null&&alive.current){const next=pendingSeek.current;pendingSeek.current=null;await request({action:'seek',frame:next},true);}}finally{seeking.current=false;}})();
  };
  const number=(label:string,value:number,change:(value:number)=>void,min:number,max:number,step=1,unit?:string)=><MotionNumber key={label} label={label} value={value} change={change} min={min} max={max} step={step} unit={unit} disabled={disabled} reportInvalid={reportInvalid}/>;
  const curve:Curve=draft?.curve??preset?.curve??curveDefaults.power;
  const curveLabel=(key:string)=>key in t?t[key as keyof typeof t]:key;
  const has=(...operations:string[])=>preset?.operations.some(operation=>operations.includes(operation))||preset?.actors?.some(actor=>actor.keys.some(key=>operations.some(op=>op in key.values||(op==='tint'&&'red' in key.values))))||false;
  return <div className="motion-panel" onKeyDown={event=>event.stopPropagation()}>
    <div className="motion-panel-toolbar">
      <div className="motion-transport" role="toolbar" aria-label={t.title}>
        <button aria-label={state.playing?t.pause:t.play} title={state.playing?t.pause:t.play} disabled={!state.data||busy} onClick={()=>void request({action:state.playing?'pause':'play'})}>{state.playing?'Ⅱ':'▶'}</button>
        <button aria-label={t.stop} title={t.stop} disabled={!state.preview||busy} onClick={()=>void request({action:'stop'})}>■</button>
        <input aria-label={t.frame} type="number" min={1} max={state.data?.duration??1} value={state.frame+1} disabled={!state.data||busy} onChange={event=>{if(event.target.value)seek(Number(event.target.value)-1);}}/>
        <span className="motion-time">{(state.frame/(state.data?.fps??24)).toFixed(2)} {t.seconds}</span>
        <button className="motion-info" aria-label={t.info} title={t.info}>ⓘ</button>
      </div>
      <input className="motion-scrubber" type="range" aria-label={t.frame} min={0} max={(state.data?.duration??1)-1} value={state.frame} disabled={!state.data||busy} onChange={event=>seek(Number(event.target.value))}/>
      <label className="motion-layer"><span>{t.layer}</span><select aria-label={t.layer} value={layerId} disabled={busy||state.playing||!layers.length} onChange={event=>void request({action:'select',layer:event.target.value})}>{layers.map(layer=><option key={layer.id} value={layer.id}>{layer.name}{layer.locked?' 🔒':''}</option>)}</select></label>
    </div>
    <div className="motion-tabs" role="tablist" aria-label={t.title}>{(['catalog','clips'] as const).map(item=><button key={item} role="tab" aria-selected={tab===item} onClick={()=>setTab(item)}>{t[item]} <small>{item==='catalog'?catalog?.presets.length??0:layer?.motions.length??0}</small></button>)}</div>
    <div className="motion-panel-content">
<PanelBlock id="motion.library" title={tab === 'catalog' ? t.catalog : t.clips}>      {tab==='catalog'?<section className="motion-library" aria-label={t.catalog}>
        <div className="motion-search"><input type="search" aria-label={t.search} placeholder={t.search} value={query} onChange={event=>setQuery(event.target.value)}/><select aria-label={t.catalog} value={category} onChange={event=>setCategory(event.target.value)}><option value="">{t.all} · {matches.length}</option>{catalog?.categories.map(item=><option key={item.id} value={item.id}>{item.names[locale]??item.names.en}</option>)}</select></div>
        <div className="motion-preset-list" role="listbox" aria-label={t.catalog}>{matches.slice(0,limit).map(item=><button key={item.id} role="option" aria-selected={draft?.preset===item.id} className="motion-preset" title={`${title(item)} · ${item.durationMs} ms`} onClick={()=>selectPreset(item)}><span className="motion-preset-mark" aria-hidden="true">{item.category.startsWith('tx-')?'Aa':item.category==='easing'?'∿':item.category==='spring'?'↝':'◇'}</span><span><strong>{title(item)}</strong><small>{catalog?.categories.find(category=>category.id===item.category)?.names[locale]} · {item.durationMs} ms</small></span></button>)}</div>
        {limit<matches.length&&<button className="motion-more" onClick={()=>setLimit(value=>value+48)}>{t.more} · {matches.length-limit}</button>}
      </section>:<section className="motion-clip-list" aria-label={t.clips}>
        {!state.data?<div className="motion-empty">{t.noDocument}</div>:!layer?<div className="motion-empty">{t.vectorOnly}</div>:!layer.motions.length?<div className="motion-empty">{t.empty}</div>:layer.motions.map((clip,index)=>{
          const item=catalog?.presets.find(preset=>preset.id===clip.preset);
          return <div key={clip.id} className="motion-clip" data-selected={selectedId===clip.id}>
            <input aria-label={`${t.enabled} · ${item?title(item):clip.preset}`} type="checkbox" checked={clip.enabled} disabled={disabled} onChange={event=>void request({action:'setMotion',documentId,layer:layer.id,clip:{...clip,enabled:event.target.checked}})}/>
            <button className="motion-clip-name" aria-pressed={selectedId===clip.id} onClick={()=>selectClip(clip)}><strong>{item?title(item):clip.preset}</strong><small>{clip.start+1}—{clip.start+clip.duration*clip.repeats}</small></button>
            <button aria-label={t.up} title={t.up} disabled={disabled||index===0} onClick={()=>void request({action:'moveMotion',documentId,layer:layer.id,id:clip.id,position:index-1})}>↑</button>
            <button aria-label={t.down} title={t.down} disabled={disabled||index===layer.motions.length-1} onClick={()=>void request({action:'moveMotion',documentId,layer:layer.id,id:clip.id,position:index+1})}>↓</button>
          </div>;
        })}
      </section>}
</PanelBlock>      {draft&&preset&&<section key={draft.id} className="motion-parameters" aria-label={title(preset)}>
        <header><strong>{title(preset)}</strong><button className="motion-info" aria-label={motionBehavior(preset,locale)} title={motionBehavior(preset,locale)}>ⓘ</button><a href={catalog?.reference} target="_blank" rel="noreferrer" aria-label={t.reference} title={t.reference}>↗</a></header>
<PanelBlock id="motion.timing" title={panelBlockLabels[locale].timing}>        <div className="motion-parameter-grid">
          {number(t.start,draft.start+1,value=>edit({start:value-1}),1,18000)}
          {number(t.duration,draft.duration,value=>edit({duration:value}),1,18000)}
          {number(t.repeats,draft.repeats,value=>edit({repeats:value}),1,1000)}
          {number(t.strength,draft.strength*100,value=>edit({strength:value/100}),0,400,1,'%')}
          {number(t.distance,draft.distance,value=>edit({distance:value}),-8192,8192,1,'px')}
          {number(t.angle,draft.angle,value=>edit({angle:value}),-36000,36000,1,'°')}
          {number(t.frequency,draft.frequency,value=>edit({frequency:value}),.1,30,.1)}
          {number(t.stagger,draft.stagger*100,value=>edit({stagger:value/100}),0,95,1,'%')}
          {number(`${t.pivot} X`,draft.pivot[0]*100,value=>edit({pivot:[value/100,draft.pivot[1]]}),-200,300,1,'%')}
          {number(`${t.pivot} Y`,draft.pivot[1]*100,value=>edit({pivot:[draft.pivot[0],value/100]}),-200,300,1,'%')}
          {has('repeat')&&number(t.copies,draft.copies??4,value=>edit({copies:value}),1,24)}
          {number(t.seed,draft.seed,value=>edit({seed:value}),0,4294967295)}
        </div>
        <label className="motion-field"><span>{t.target}</span><select aria-label={t.target} value={draft.objects.length?'selected':'layer'} disabled={disabled} onChange={event=>edit({objects:event.target.value==='selected'?(state.data?.selectedObjects??[]):[]})}><option value="layer">{t.wholeLayer}</option><option value="selected" disabled={!state.data?.selectedObjects.length}>{t.selected} · {state.data?.selectedObjects.length??0}</option></select></label>
        <label className="motion-field"><span>{t.order}</span><select aria-label={t.order} value={draft.order} disabled={disabled} onChange={event=>edit({order:event.target.value})}>{['forward','reverse','center','edges','random','grid','alternating','spiral'].map(order=><option key={order} value={order}>{order==='reverse'?t.reverseOrder:curveLabel(order)}</option>)}</select></label>
        <div className="motion-checks"><label><input type="checkbox" checked={draft.reverse} disabled={disabled} onChange={event=>edit({reverse:event.target.checked})}/>{t.reverse}</label><label><input type="checkbox" checked={draft.yoyo} disabled={disabled} onChange={event=>edit({yoyo:event.target.checked})}/>{t.yoyo}</label></div>
</PanelBlock>        {has('cube','meshPlane','depthStack','coverflow')&&<PanelBlock id="motion.depth" title={<>3D {draft.extruded&&<span className="motion-info" title={t.bevelInfo}>ⓘ</span>}</>}><div className="motion-parameter-grid">
          {number(t.meshDepth,draft.meshDepth??80,value=>edit({meshDepth:value}),0,4096,1,'px')}
          {number(t.cameraDistance,draft.cameraDistance??600,value=>edit({cameraDistance:value}),64,8192,1,'px')}
          {number(t.rotationX,draft.rotationX??-20,value=>edit({rotationX:value}),-36000,36000,1,'°')}
          {number(t.rotationY,draft.rotationY??25,value=>edit({rotationY:value}),-36000,36000,1,'°')}
          {number(t.rotationZ,draft.rotationZ??0,value=>edit({rotationZ:value}),-36000,36000,1,'°')}
          {draft.extruded&&number(t.bevelWidth,draft.bevelWidth??0,value=>edit({bevelWidth:value}),0,128,.1,'px')}
          {!draft.extruded&&number(t.copies,draft.copies??4,value=>edit({copies:value}),1,24)}
        </div>{['tx-layered-depth','tx-motext-falloff'].includes(preset.id)&&<label className="motion-checks"><input type="checkbox" checked={draft.extruded??false} disabled={disabled} onChange={event=>edit({extruded:event.target.checked})}/>{t.extruded}</label>}</PanelBlock>}
        {has('fontAxes')&&<PanelBlock id="motion.font-axes" title={<>{t.fontAxes} <span className="motion-info" title={t.fontInfo}>ⓘ</span></>}>
          <label className="motion-field"><span>{t.variableFont}</span><select aria-label={t.variableFont} value={draft.variableFont??''} disabled={disabled} onChange={event=>{const font=fonts.find(font=>font.family===event.target.value);const axes=(draft.fontAxes?.length?draft.fontAxes:defaultAxes(preset.id)).filter(axis=>!font||font.axes.some(a=>a.tag===axis.tag)).map(axis=>{const limits=font?.axes.find(a=>a.tag===axis.tag);return limits?{...axis,from:Math.max(limits.minimum,Math.min(limits.maximum,axis.from)),to:Math.max(limits.minimum,Math.min(limits.maximum,axis.to))}:axis;});edit({variableFont:event.target.value,fontAxes:axes.length?axes:font?[{tag:font.axes[0].tag,from:font.axes[0].minimum,to:font.axes[0].maximum}]:defaultAxes(preset.id)});}}><option value="">{t.autoFont}</option>{fonts.filter((font,index,list)=>list.findIndex(other=>other.family===font.family)===index).map(font=><option key={font.family} value={font.family}>{font.family}</option>)}</select></label>
          {(draft.fontAxes?.length?draft.fontAxes:defaultAxes(preset.id)).map((axis,index)=>{const font=fonts.find(font=>font.family===draft.variableFont);const limits=font?.axes.find(a=>a.tag===axis.tag);const axes=draft.fontAxes?.length?draft.fontAxes:defaultAxes(preset.id);const update=(patch:Partial<typeof axis>)=>edit({fontAxes:axes.map((old,i)=>i===index?{...old,...patch}:old)});return <div className="motion-parameter-grid" key={axis.tag}>{number(`${axis.tag} · ${t.from}`,axis.from,value=>update({from:value}),limits?.minimum??-100000,limits?.maximum??100000,.1)}{number(`${axis.tag} · ${t.to}`,axis.to,value=>update({to:value}),limits?.minimum??-100000,limits?.maximum??100000,.1)}{font&&<select aria-label={`${t.axis} ${index+1}`} value={axis.tag} disabled={disabled} onChange={event=>{const axis=font.axes.find(a=>a.tag===event.target.value)!;update({tag:axis.tag,from:axis.minimum,to:axis.maximum});}}>{font.axes.filter(a=>a.tag===axis.tag||!axes.some(existing=>existing.tag===a.tag)).map(a=><option key={a.tag} value={a.tag}>{a.tag}</option>)}</select>}<button disabled={disabled||axes.length===1} onClick={()=>edit({fontAxes:axes.filter((_,i)=>i!==index)})}>{t.delete}</button></div>;})}
          <button disabled={disabled||!fonts.find(f=>f.family===draft.variableFont)?.axes.some(a=>!(draft.fontAxes??[]).some(existing=>existing.tag===a.tag))} onClick={()=>{const font=fonts.find(f=>f.family===draft.variableFont);const axis=font?.axes.find(a=>!(draft.fontAxes??[]).some(existing=>existing.tag===a.tag));if(axis)edit({fontAxes:[...(draft.fontAxes??[]),{tag:axis.tag,from:axis.minimum,to:axis.maximum}]});}}>{t.addAxis}</button>
        </PanelBlock>}
        {has('kanjiStroke')&&<PanelBlock id="motion.kanji-strokes" title={<>{t.program} <span className="motion-info" title={t.strokeInfo}>ⓘ</span></>}><div className="motion-parameter-grid">{number(t.strokeGap,draft.strokeGapMs??100,value=>edit({strokeGapMs:value}),0,1000,1,'ms')}{number(t.strokeWidth,draft.strokeWidth??4,value=>edit({strokeWidth:value}),.5,20,.1)}</div></PanelBlock>}
        <PanelBlock id="motion.curve" title={<>{t.curve}</>}>
          <label className="motion-check"><input type="checkbox" checked={draft.curve===null} disabled={disabled} onChange={event=>edit({curve:event.target.checked?null:structuredClone(preset.curve)})}/>{t.presetCurve}</label>
          <select aria-label={t.curve} value={curve.kind} disabled={disabled||draft.curve===null} onChange={event=>edit({curve:structuredClone(curveDefaults[event.target.value])})}>{Object.keys(curveDefaults).map(kind=><option key={kind} value={kind}>{curveLabel(kind)}</option>)}</select>
          {draft.curve&&<div className="motion-parameter-grid">
            {'mode' in curve&&<label className="motion-field"><span>{t.mode}</span><select aria-label={t.mode} disabled={disabled} value={String(curve.mode)} onChange={event=>edit({curve:{...curve,mode:event.target.value}})}>{['in','out','inOut'].map(mode=><option key={mode} value={mode}>{curveLabel(mode)}</option>)}</select></label>}
            {Object.entries(curve).filter(([key,value])=>key!=='kind'&&typeof value==='number').map(([key,value])=>number(curveLabel(key),Number(value),number=>edit({curve:{...curve,[key]:number}}),...(curveRanges[key]??[0,2000,.01] as [number,number,number])))}
            {curve.kind==='bezier'&&Array.isArray(curve.points)&&curve.points.map((value,index)=>number(['X1','Y1','X2','Y2'][index],Number(value),number=>{const points=[...(curve.points as number[])];points[index]=number;edit({curve:{...curve,points}});},index%2===0?0:-8,index%2===0?1:8,.01))}
          </div>}
        </PanelBlock>
        {(has('blur','glow','particles','confetti','snow','embers','fireworks','coalesce','hue','tint','highlight','sweep')||preset.category.startsWith('tx-'))&&<PanelBlock id="motion.program" title={<>{t.program}</>}><div className="motion-parameter-grid">
          {has('blur')&&number(t.blur,draft.blur,value=>edit({blur:value}),0,64,1,'px')}
          {has('glow')&&number(t.glow,draft.glow,value=>edit({glow:value}),0,64,1,'px')}
          {has('particles','confetti','snow','embers','fireworks','coalesce')&&number(t.particles,draft.particleCount,value=>edit({particleCount:value}),0,128)}
          <label className="motion-number"><span>{t.color}</span><ColorPickerPopover locale={locale} label={t.color} color={[draft.color[0],draft.color[1],draft.color[2]]} disabled={disabled} onChange={color=>edit({color:[...color,255]})}/></label>
        </div></PanelBlock>}
        {has('textSwap')&&<label className="motion-field"><span>{t.alternateText}</span><input aria-label={t.alternateText} value={draft.alternateText} disabled={disabled} maxLength={8192} onChange={event=>edit({alternateText:event.target.value})}/></label>}
        {has('counter')&&<div className="motion-parameter-grid">{number(t.from,draft.numberFrom,value=>edit({numberFrom:value}),-1e12,1e12)}{number(t.to,draft.numberTo,value=>edit({numberTo:value}),-1e12,1e12)}</div>}
        {has('morph')&&<label className="motion-field"><span>{t.morphTarget}</span><textarea aria-label={t.morphTarget} rows={3} maxLength={32768} value={draft.morphTarget??''} disabled={disabled} onChange={event=>edit({morphTarget:event.target.value})}/></label>}
        {has('arc','follow')&&<label className="motion-field"><span>{t.path}<span className="motion-info" title={t.pathHint}> ⓘ</span></span><textarea aria-label={t.path} rows={3} value={path} disabled={disabled} onChange={event=>setPath(event.target.value)}/></label>}
        <div className="motion-actions"><button disabled={disabled||hasInvalidNumber} onClick={()=>void apply()}>{busy?t.working:chosen?t.apply:t.add}</button>{chosen&&<><button disabled={disabled||hasInvalidNumber} onClick={()=>void apply(true)}>{t.duplicate}</button><button disabled={disabled} onClick={()=>void request({action:'removeMotion',documentId,layer:layerId,id:chosen.id}).then(success=>{if(success){setDraft(null);setSelectedId('');}})}>{t.delete}</button></>}</div>
      </section>}
    </div>
    {layer?.locked&&<div className="motion-status">{t.locked}</div>}
    {error&&<div className="motion-error" role="alert">{error}</div>}
  </div>;
}
