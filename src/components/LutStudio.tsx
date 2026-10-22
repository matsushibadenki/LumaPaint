import {useEffect,useRef,useState, type PointerEvent} from 'react';
import {listen} from '@tauri-apps/api/event';
import type {Locale} from '../i18n';
import {defaultGrade,lutEditor,lutLabels,lutPreview,type LutGrade,type LutSnapshot} from '../lut-editor';
import {CompactSlider} from './CompactSlider';
import {Icon} from './Icon';
import './lut-studio.css';

export function LutStudio({locale,open,onClose}:{locale:Locale;open:boolean;onClose:()=>void}) {
 const t=lutLabels[locale], [snapshot,setSnapshot]=useState<LutSnapshot|null>(null),[draft,setDraft]=useState(defaultGrade),[busy,setBusy]=useState(false),[operation,setOperation]=useState(''),[error,setError]=useState('');
 const [top,setTop]=useState(44),[split,setSplit]=useState(50),[channel,setChannel]=useState(0),[point,setPoint]=useState(2),[angle,setAngle]=useState(35);
 const pending=useRef<Promise<boolean>|null>(null);
 const gate=useRef(false), current=useRef(snapshot), edits=useRef(draft), mounted=useRef(true), drag=useRef<number|null>(null);
 current.current=snapshot;edits.current=draft;
 useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;};},[]);
 const dirty=()=>!!current.current&&JSON.stringify(edits.current)!==JSON.stringify(current.current.grade);
 function run(action:string,grade?:LutGrade):Promise<boolean> {
  const previous=pending.current;
  const task=(async()=>{
   if(previous)await previous;
   if(action==='update'&&JSON.stringify(grade)===JSON.stringify(current.current?.grade))return true;
   gate.current=true;setBusy(true);setOperation(action);setError('');
   try {
    const value=await lutEditor(action,current.current?.revision,grade);
    if(mounted.current){
     current.current=value;setSnapshot(value);
     // An edit made while the worker was saving must not be overwritten by its reply.
     if(action!=='update'||JSON.stringify(edits.current)===JSON.stringify(grade)){
      edits.current=value.grade;setDraft(value.grade);
     }
    }
    return true;
   }catch(e){if(mounted.current)setError(String(e));return false;}
   finally{gate.current=false;if(mounted.current){setBusy(false);setOperation('');}}
  })();
  pending.current=task;
  return task;
 }
 async function commit(next=edits.current){if(current.current&&(gate.current||JSON.stringify(next)!==JSON.stringify(current.current.grade)))return run('update',next);return true;}
 async function action(name:string){if(await commit())await run(name);}
 async function close(){if(await commit())onClose();}
 const closeRef=useRef(close);closeRef.current=close;
 useEffect(()=>{if(open){void run('get');document.querySelector<HTMLButtonElement>('.lut-header button:last-child')?.focus();}},[open]);
 useEffect(()=>{
  const header=document.querySelector('.application-bar');if(!header)return;
  const update=()=>setTop(header.getBoundingClientRect().bottom);const observer=new ResizeObserver(update);observer.observe(header);update();return()=>observer.disconnect();
 },[]);
 useEffect(()=>{const key=(e:KeyboardEvent)=>{if(open&&e.key==='Escape'){e.preventDefault();e.stopPropagation();void closeRef.current();}};window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);},[open]);
 useEffect(()=>{let disposed=false;let unlisten:(()=>void)|undefined;void listen<number>('lut-editor-changed',event=>{if(!gate.current&&!dirty()&&event.payload!==(current.current?.revision))void run('get');}).then(fn=>{if(disposed)fn();else unlisten=fn;}).catch(()=>{});return()=>{disposed=true;unlisten?.();};},[]);
 const set=(patch:Partial<LutGrade>)=>{const next={...edits.current,...patch};edits.current=next;setDraft(next);};
 function setKnot(value:number,index=point){const curves=edits.current.curves.map(c=>[...c]);curves[channel][index]=Math.max(0,Math.min(1,value));set({curves});}
 function move(e:PointerEvent<SVGSVGElement>){if(drag.current===null)return;const r=e.currentTarget.getBoundingClientRect();setKnot(1-(e.clientY-r.top)/r.height,drag.current);}
 function preset(index:number){const next={...defaultGrade(),name:edits.current.name,size:edits.current.size};if(index===1){next.warmth=.45;next.saturation=1.1;}if(index===2){next.warmth=-.45;next.contrast=1.08;}if(index===3)next.saturation=0;if(index===4){next.curves[0]=[.08,.28,.5,.72,.94];next.saturation=.8;}setDraft(next);edits.current=next;void commit(next);}
 const blocked=busy&&operation!=='update';
 const keys=['exposure','contrast','saturation','warmth','strength'] as const;
 const limits={exposure:[-4,4,.05],contrast:[0,2,.01],saturation:[0,2,.01],warmth:[-1,1,.01],strength:[0,1,.01]};
 const color=['#d8dde5','#f17a7a','#72c48a','#7eaeef'][channel];
 const yaw=angle*Math.PI/180,project=(v:number[])=>{const [r,g,b]=v.map(n=>n-.5);const x=r*Math.cos(yaw)-b*Math.sin(yaw),z=r*Math.sin(yaw)+b*Math.cos(yaw);return [120+x*125,108-g*125+z*45,z];};
 const dots=(snapshot?.samples??[]).map((v,i)=>({v,i,p:project(v)})).sort((a,b)=>a.p[2]-b.p[2]);
 const corners=Array.from({length:8},(_,i)=>project([i&1,(i>>1)&1,(i>>2)&1]));
 return <section id="lut-studio-overlay" className="lut-studio" hidden={!open} style={{top}} role="dialog" aria-label={t.title} aria-busy={busy}>
  <header className="lut-header"><Icon name="lut"/><strong>{t.title}</strong><span title={t.note} aria-label={t.note} tabIndex={0}>ⓘ</span><div className="lut-spacer"/>
   <button disabled={blocked||!snapshot} title={t.import} aria-label={t.import} onClick={()=>void action('import')}><Icon name="open"/><span>{t.import}</span></button>
   <button disabled={blocked||!snapshot} title={t.export} aria-label={t.export} onClick={()=>void action('export')}><Icon name="save"/><span>{t.export}</span><small>.cube</small></button>
   <button disabled={blocked} title={t.close} aria-label={t.close} onClick={()=>void close()}>×</button>
  </header>
  {error&&<div className="lut-error" role="alert">{error}<button disabled={blocked} onClick={()=>void run('get')}>{t.reload}</button></div>}
  <div className="lut-layout">
   <aside className="lut-controls"><fieldset disabled={blocked||!snapshot}>
    <label className="lut-name"><span>{t.name}</span><input aria-label={t.name} maxLength={120} value={draft.name} onChange={e=>set({name:e.target.value})} onBlur={()=>void commit()} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></label>
    <div className="lut-history"><button title={t.undo} aria-label={t.undo} disabled={!snapshot?.canUndo} onClick={()=>void action('undo')}><Icon name="undo"/></button><button title={t.redo} aria-label={t.redo} disabled={!snapshot?.canRedo} onClick={()=>void action('redo')}><Icon name="redo"/></button><button onClick={()=>void action('reset')}>{t.reset}</button></div>
    <label className="lut-setting"><span>{t.preset}</span><select aria-label={t.preset} value="" onChange={e=>preset(Number(e.target.value))}><option value="" disabled>—</option>{t.presets.map((name,i)=><option key={name} value={i}>{name}</option>)}</select></label>
    {keys.map(key=><label className="lut-control" key={key}><span>{t[key]}</span><output>{draft[key].toFixed(2)}{key==='exposure'?' EV':''}</output><CompactSlider aria-label={t[key]} min={limits[key][0]} max={limits[key][1]} step={limits[key][2]} value={draft[key]} onChange={e=>set({[key]:Number(e.target.value)})} onPointerUp={()=>void commit()} onKeyUp={()=>void commit()} onBlur={()=>void commit()}/></label>)}
    <div className="lut-curve-heading"><strong>{t.curves}</strong><span title={t.curveHint} tabIndex={0}>ⓘ</span><div className="lut-channels">{[t.master,'R','G','B'].map((name,i)=><button key={name} aria-pressed={channel===i} onClick={()=>setChannel(i)}>{name}</button>)}</div></div>
    <svg className="lut-curve" viewBox="0 0 200 200" role="img" aria-label={t.curves} onPointerDown={e=>{if(blocked||!snapshot)return;const rect=e.currentTarget.getBoundingClientRect();const index=Math.max(0,Math.min(4,Math.round((e.clientX-rect.left)/rect.width*4)));drag.current=index;setPoint(index);e.currentTarget.setPointerCapture(e.pointerId);move(e);}} onPointerMove={move} onPointerUp={()=>{drag.current=null;void commit();}} onPointerCancel={()=>{drag.current=null;if(snapshot){edits.current=snapshot.grade;setDraft(snapshot.grade);}}}>
     {[0,50,100,150,200].map(v=><path key={v} d={`M${v} 0V200 M0 ${v}H200`} className="lut-grid-line"/>)}<path d="M0 200L200 0" className="lut-diagonal"/>
     <polyline fill="none" stroke={color} strokeWidth="2" points={draft.curves[channel].map((v,i)=>`${i*50},${200-v*200}`).join(' ')}/>
     {draft.curves[channel].map((v,i)=><circle key={i} cx={i*50} cy={200-v*200} r={i===point?5:3.5} fill={color}/>)}
    </svg>
    <div className="lut-knots"><label>{t.input}<select aria-label={t.input} value={point} onChange={e=>setPoint(Number(e.target.value))}>{[0,25,50,75,100].map((v,i)=><option key={v} value={i}>{v}%</option>)}</select></label><label>{t.output}<input type="number" aria-label={t.output} min={0} max={100} step={1} value={Math.round(draft.curves[channel][point]*100)} onChange={e=>{if(Number.isFinite(e.target.valueAsNumber))setKnot(e.target.valueAsNumber/100);}} onBlur={()=>void commit()} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/>%</label></div>
   </fieldset></aside>
   <main className="lut-preview-area">
    <div className="lut-preview-toolbar"><button disabled={blocked||!snapshot} onClick={()=>void action('photo')}><Icon name="image"/>{t.photo}</button><button disabled={blocked||!snapshot?.hasPhoto} onClick={()=>void action('chart')}>{t.chart}</button><span role="status">{busy?t.working:''}</span></div>
    <div className="lut-comparison">
     {snapshot&&<><img alt={t.before} src={lutPreview('before',snapshot.revision)}/><img className="lut-after" alt={t.after} src={lutPreview('after',snapshot.revision)} style={{clipPath:`inset(0 0 0 ${split}%)`}}/><div className="lut-divider" style={{left:`${split}%`}}/></>}
     <span className="lut-before-label">{t.before}</span><span className="lut-after-label">{t.after}</span>
    </div>
    <CompactSlider aria-label={t.compare} value={split} onChange={e=>setSplit(Number(e.target.value))}/>
    <div className="lut-preview-caption"><span>{snapshot?.hasPhoto?t.photo:t.chart}</span><span>sRGB · RGB 0–1</span></div>
   </main>
   <aside className="lut-lattice"><h3>{t.cube}</h3><svg viewBox="0 0 240 220" role="img" aria-label={t.cube}>
    {corners.flatMap((p,i)=>[1,2,4].filter(bit=>(i&bit)===0).map(bit=><line key={`${i}-${bit}`} x1={p[0]} y1={p[1]} x2={corners[i|bit][0]} y2={corners[i|bit][1]} className="lut-grid-line"/>))}
    {dots.map(({v,i,p})=><circle key={i} cx={p[0]} cy={p[1]} r="2.8" fill={`rgb(${v.map(c=>Math.round(Math.max(0,Math.min(1,c))*255)).join(' ')})`}/>)}
   </svg><CompactSlider aria-label={t.rotate} min={0} max={360} value={angle} onChange={e=>setAngle(Number(e.target.value))}/>
   <label className="lut-setting"><span>{t.size}</span><select disabled={blocked||!snapshot} aria-label={t.size} value={draft.size} onChange={e=>{const next={...edits.current,size:Number(e.target.value)};set({size:next.size});void commit(next);}}>{[17,33,65].map(n=><option key={n} value={n}>{n} × {n} × {n}</option>)}</select></label>
   <dl><dt>{t.base}</dt><dd>{snapshot?.baseName??t.identity}</dd><dt>{t.output}</dt><dd>.cube · 3D LUT</dd></dl>
   </aside>
  </div>
 </section>;
}
