import {useEffect,useId,useMemo,useRef,useState} from 'react';
import type {Locale} from '../i18n';
import {snapTimelineFrame,type TimelineKey} from '../timeline-view';
import {GRAPH,graphEditTimeDelta,graphDragEdit,graphPoint,graphValue,graphPath,graphMarkers,graphBatch,graphTimeDelta,graphValueDelta,type KeyBatchEdit,graphFullView,graphConstrainView,graphZoomView,graphPanView,graphFitView,type GraphViewport,type GraphAxis} from '../timeline-graph';
const labels={
 ja:{axis:'拡大縮小する軸',both:'時間と値',time:'時間',value:'値',zoomIn:'グラフを拡大',zoomOut:'グラフを縮小',fit:'グラフ全体を表示',fitSelection:'選択キーを表示',pan:'グラフの表示範囲を移動',viewHint:'＋／−で拡大縮小、0で全体、Fで選択キーを表示。軸を選んでCommand／Ctrl＋ホイールでポインター位置を中心に拡大縮小。✥または中ボタンドラッグで表示範囲を移動。移動モードでは矢印でも移動（Shiftで大きく）。Escapeで移動を取消。',title:'値グラフ',empty:'キーなし',valueMode:'値をドラッグ',timeMode:'時刻をドラッグ',freeMode:'時刻と値をドラッグ',all:'すべてのキーを選択',remove:'選択キーを削除',interpolation:'選択キーの補間',mixed:'混在',hold:'保持',linear:'リニア',ease:'イーズ',count:'選択キー数',hint:'Command / Ctrl＋クリックで選択を切替、Shift＋クリックで範囲選択、Command / Ctrl＋Aですべて選択。↕は値、↔は時刻、↗は時刻と値を一括調整。↑↓で値、Option＋←→で時刻（Shiftで10）。Deleteで削除。ドラッグ中のOptionで吸着を一時解除、Escapeで取消。'},
 en:{axis:'Graph zoom axis',both:'Time and value',time:'Time',value:'Value',zoomIn:'Zoom graph in',zoomOut:'Zoom graph out',fit:'Fit entire graph',fitSelection:'Fit selected keys',pan:'Pan graph view',viewHint:'Plus / Minus zooms, 0 fits all, F fits selected keys. Choose an axis; Command / Ctrl + wheel zooms around the pointer. ✥ or middle-button drag pans. Arrow keys pan in pan mode (Shift: larger steps). Escape cancels a pan.',title:'Value graph',empty:'No keys',valueMode:'Drag values',timeMode:'Drag timing',freeMode:'Drag timing and values',all:'Select all keys',remove:'Delete selected keys',interpolation:'Selected key interpolation',mixed:'Mixed',hold:'Hold',linear:'Linear',ease:'Ease',count:'Selected keys',hint:'Command / Ctrl + click toggles selection; Shift + click selects a range; Command / Ctrl + A selects all. ↕ edits values; ↔ edits timing; ↗ edits both together. Up / Down edits values; Alt / Option + Left / Right edits timing (Shift: 10). Delete removes keys. Alt / Option bypasses snapping while dragging; Escape cancels.'},
 'zh-CN':{axis:'曲线缩放轴',both:'时间和数值',time:'时间',value:'数值',zoomIn:'放大曲线',zoomOut:'缩小曲线',fit:'显示全部曲线',fitSelection:'显示所选关键帧',pan:'移动曲线显示范围',viewHint:'＋／−缩放，0显示全部，F显示所选关键帧。选择坐标轴后按Command／Ctrl＋滚轮以指针为中心缩放。✥或鼠标中键拖动可平移。平移模式下也可用方向键（Shift增大步长）。Escape取消平移。',title:'数值曲线',empty:'无关键帧',valueMode:'拖动数值',timeMode:'拖动时间',freeMode:'同时拖动时间和数值',all:'选择全部关键帧',remove:'删除所选关键帧',interpolation:'所选关键帧插值',mixed:'混合',hold:'保持',linear:'线性',ease:'缓动',count:'所选关键帧数',hint:'Command / Ctrl＋单击切换选择，Shift＋单击选择范围，Command / Ctrl＋A全选。↕调整数值，↔调整时间，↗同时调整时间和数值。上下键修改数值，Alt / Option＋左右键修改时间（Shift为10）。Delete删除。拖动时按住Alt / Option暂时关闭吸附，Escape取消。'},
};
const equalKeys=(a:readonly TimelineKey[],b:readonly TimelineKey[])=>a.length===b.length&&a.every((k,i)=>k.frame===b[i].frame&&k.interpolation===b[i].interpolation&&Math.fround(k.value)===Math.fround(b[i].value));
export function TimelineGraph({locale,keys,duration,frame,base,min,max,disabled,revision,property,onSelect,onBatch,snapEnabled=false,getSnapTargets}:{locale:Locale;keys:readonly TimelineKey[];duration:number;frame:number;base:number;min:number;max:number;disabled:boolean;revision:number;property:string;onSelect:(frame:number)=>void;onBatch:(frames:number[],edit:KeyBatchEdit)=>Promise<void>;snapEnabled?:boolean;getSnapTargets?:(from:number,selected:number[])=>number[]}){
 const clipId=useId().replace(/:/g,''),t=labels[locale],root=useRef<HTMLDivElement>(null),svg=useRef<SVGSVGElement>(null);
 const drag=useRef<{key:TimelineKey;frames:number[];delta:number;valueDelta:number;x:number;y:number;clientX:number;clientY:number;mode:'value'|'time'|'free';targets:number[];bypass:boolean;zoom:number}|null>(null);
 const [manualView,setView]=useState<GraphViewport|null>(null),[axis,setAxis]=useState<GraphAxis>('both'),[panning,setPanning]=useState(false);
 const panDrag=useRef<{view:GraphViewport;prior:GraphViewport|null;x:number;y:number}|null>(null);
 const [mode,setMode]=useState<'value'|'time'|'free'|'pan'>('value'),[snapped,setSnapped]=useState(false),[selected,setSelected]=useState<number[]>([]),[activeFrame,setActive]=useState<number|null>(null),[draft,setDraft]=useState<TimelineKey[]|null>(null),[pending,setPending]=useState(false);
 const anchor=useRef<number|null>(null),previous=useRef(keys),ignoreSeek=useRef<number|null>(null),plan=useRef<{keys:TimelineKey[];frames:number[];active:number|null;seek:number|null}|null>(null),alive=useRef(true);
 const fullView=useMemo(()=>graphFullView(keys,base,duration),[keys,base,duration]),limits={min,max},view=manualView?graphConstrainView(manualView,duration,limits):fullView,range=view,shown=draft??keys,active=shown.find(k=>k.frame===((activeFrame??frame)+(draft&&drag.current&&drag.current.mode!=='value'?drag.current.delta:0))),index=keys.findIndex(k=>k.frame===(activeFrame??frame)),selectedSet=new Set(selected),blocked=disabled||pending;
 const chosen=keys.filter(k=>selectedSet.has(k.frame)),interpolation=chosen.length&&chosen.every(k=>k.interpolation===chosen[0].interpolation)?chosen[0].interpolation:'';
 function cancel(){if(panDrag.current){setView(panDrag.current.prior);panDrag.current=null;}setPanning(false);drag.current=null;setDraft(null);setSnapped(false);}
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 useEffect(()=>{cancel();},[revision,disabled,duration]);
 useEffect(()=>{
  if(plan.current&&equalKeys(keys,plan.current.keys)){setSelected(plan.current.frames);setActive(plan.current.active);anchor.current=plan.current.active;ignoreSeek.current=plan.current.seek;plan.current=null;}
  else if(!equalKeys(keys,previous.current)){setSelected([]);setActive(null);anchor.current=null;plan.current=null;}
  previous.current=keys;
 },[keys]);
 useEffect(()=>{if(ignoreSeek.current===frame){ignoreSeek.current=null;return;}ignoreSeek.current=null;if(!drag.current&&!plan.current&&keys.some(k=>k.frame===frame)&&!selected.includes(frame)){setSelected([frame]);setActive(frame);anchor.current=frame;}},[frame]);
 async function commit(frames:number[],edit:KeyBatchEdit){
  if(blocked||!frames.length)return;
  if((edit.kind==='shiftTime'||edit.kind==='shiftValue')&&!edit.delta)return;
  const next=graphBatch(keys,frames,edit),nextFrames=edit.kind==='delete'?[]:frames.map(f=>f+graphEditTimeDelta(edit)),nextActive=edit.kind==='delete'?null:(activeFrame??frames[0])+graphEditTimeDelta(edit);
  if(equalKeys(keys,next))return;
  const current={keys:next,frames:nextFrames,active:nextActive,seek:edit.kind==='shiftTime'||edit.kind==='move'?Math.min(...nextFrames):null};plan.current=current;setPending(true);
  try{await onBatch(frames,edit);}catch{if(alive.current){plan.current=null;setSelected([]);setActive(null);}}
  finally{if(alive.current){setPending(false);}}
 }
 function select(at:number,extend=false,toggle=false){
  const key=keys[at];if(!key)return [];
  let frames:number[];
  if(toggle)frames=selected.includes(key.frame)?selected.filter(f=>f!==key.frame):[...selected,key.frame];
  else if(extend&&anchor.current!==null){const a=anchor.current;frames=keys.filter(k=>k.frame>=Math.min(a,key.frame)&&k.frame<=Math.max(a,key.frame)).map(k=>k.frame);}
  else frames=selected.includes(key.frame)?selected:[key.frame];
  setSelected(frames);setActive(key.frame);if(!extend)anchor.current=key.frame;if(frames.includes(key.frame))onSelect(key.frame);return frames;
 }
 function update(){
  const d=drag.current;if(!d)return;const box=svg.current!.getBoundingClientRect();let delta=0,valueDelta=0;
  if(d.mode!=='time'){
   const y=graphPoint(d.key.frame,d.key.value,duration,range,view).y+(d.clientY-d.y)/box.height*GRAPH.height;
   valueDelta=graphValueDelta(d.clientY===d.y?0:graphValue(y,range,min,max)-d.key.value,keys,d.frames,min,max);
  }
  if(d.mode!=='value'){
   const dx=d.clientX-d.x,raw=dx/d.zoom;let target=raw,best=Infinity,hit=false;
   for(const f of d.frames){const result=snapTimelineFrame(f+raw,d.targets,d.zoom,0,duration-1,snapEnabled&&!d.bypass,Math.abs(dx)>3);if(result.snapped&&Math.abs(result.frame-f-raw)<best){target=result.frame-f;best=Math.abs(target-raw);hit=true;}}
   delta=graphTimeDelta(target,d.frames,keys,duration);setSnapped(hit&&delta===target);
  }
  d.delta=delta;d.valueDelta=valueDelta;setDraft(graphBatch(keys,d.frames,graphDragEdit(d.mode,delta,valueDelta)));
 }
 useEffect(()=>{
  if(!draft&&!panning)return;
  const blur=()=>cancel(),listener=(e:KeyboardEvent)=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();cancel();}else if(e.key==='Alt'&&drag.current&&drag.current.mode!=='value'){drag.current.bypass=e.altKey;update();}};
  window.addEventListener('blur',blur);window.addEventListener('keydown',listener,true);window.addEventListener('keyup',listener,true);
  return()=>{window.removeEventListener('blur',blur);window.removeEventListener('keydown',listener,true);window.removeEventListener('keyup',listener,true);};
 },[!!draft,panning]);
 function zoom(factor:number,x=.5,y=.5){if(drag.current||panDrag.current||pending)return;setView(graphZoomView(view,factor,axis,x,y,duration,limits));}
 useEffect(()=>{
  const element=svg.current;if(!element)return;
  const wheel=(event:WheelEvent)=>{
   if(!event.ctrlKey&&!event.metaKey)return;
   event.preventDefault();event.stopPropagation();
   const box=element.getBoundingClientRect(),x=((event.clientX-box.left)/box.width*GRAPH.width-GRAPH.left)/(GRAPH.width-GRAPH.left-GRAPH.right),y=1-((event.clientY-box.top)/box.height*GRAPH.height-GRAPH.top)/(GRAPH.height-GRAPH.top-GRAPH.bottom);
   zoom(Math.exp(-Math.max(-200,Math.min(200,event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?box.height:1)))*.004),x,y);
  };
  element.addEventListener('wheel',wheel,{passive:false});return()=>element.removeEventListener('wheel',wheel);
 },[view.start,view.end,view.min,view.max,axis,duration,min,max,pending]);
 const visible=shown.filter(k=>k.frame>=view.start&&k.frame<=view.end&&k.value>=view.min&&k.value<=view.max);
 const viewing=!!draft||panning||pending,tickDigits=Math.min(5,Math.max(0,2-Math.floor(Math.log10(range.max-range.min))));
 return <div className="timeline-graph" ref={root} tabIndex={0} role="group" aria-label={`${t.title} · ${property}`} aria-disabled={blocked} data-shortcut-scope="timeline-graph" data-mode={mode} data-snapped={snapped} data-selection-count={selected.length} data-view-start={view.start} data-view-end={view.end} data-view-min={view.min} data-view-max={view.max} onKeyDown={e=>{
  if(e.target instanceof HTMLSelectElement||e.target instanceof HTMLButtonElement)return;
  if(e.key==='Escape'){cancel();e.stopPropagation();return;}
  if(!viewing&&['+','=','-','0','f','F'].includes(e.key)&&!e.metaKey&&!e.ctrlKey&&!e.altKey){e.preventDefault();e.stopPropagation();if(e.key==='0')setView(null);else if(e.key.toLowerCase()==='f'){if(chosen.length)setView(graphFitView(chosen,base,duration,limits));}else zoom(e.key==='-'?1/1.5:1.5);return;}
  if(mode==='pan'&&!viewing&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();e.stopPropagation();const step=e.shiftKey?.5:.1;setView(graphPanView(view,(e.key==='ArrowLeft'?-1:e.key==='ArrowRight'?1:0)*(view.end-view.start)*step,(e.key==='ArrowDown'?-1:e.key==='ArrowUp'?1:0)*(view.max-view.min)*step,duration,limits));return;}
  if(!keys.length||blocked||drag.current||panning)return;
  if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='a'){e.preventDefault();e.stopPropagation();setSelected(keys.map(k=>k.frame));setActive(keys[0].frame);anchor.current=keys[0].frame;return;}
  if(['Delete','Backspace'].includes(e.key)){e.preventDefault();e.stopPropagation();void commit(selected,{kind:'delete'});return;}
  if(e.altKey&&['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();e.stopPropagation();const direction=e.key==='ArrowLeft'?-1:1;void commit(selected,{kind:'shiftTime',delta:graphTimeDelta(direction*(e.shiftKey?10:1),selected,keys,duration,direction)});return;}
  if(['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){e.preventDefault();e.stopPropagation();const at=e.key==='Home'?0:e.key==='End'?keys.length-1:Math.max(0,Math.min(keys.length-1,index<0?(e.key==='ArrowLeft'?keys.length-1:0):index+(e.key==='ArrowLeft'?-1:1)));if(e.shiftKey)select(at,true);else{setSelected([keys[at].frame]);setActive(keys[at].frame);anchor.current=keys[at].frame;onSelect(keys[at].frame);}}
  if(['ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();e.stopPropagation();void commit(selected,{kind:'shiftValue',delta:graphValueDelta((e.key==='ArrowUp'?1:-1)*(e.shiftKey?10:1),keys,selected,min,max)});}
 }}>
  <div className="timeline-graph-heading"><strong>{t.title}</strong><span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span><button type="button" className="timeline-graph-mode" title={t.valueMode} aria-label={t.valueMode} aria-pressed={mode==='value'} disabled={blocked||!!draft} onClick={()=>setMode('value')}>↕</button><button type="button" className="timeline-graph-mode" title={t.timeMode} aria-label={t.timeMode} aria-pressed={mode==='time'} disabled={blocked||!!draft} onClick={()=>setMode('time')}>↔</button><button type="button" className="timeline-graph-mode" title={t.freeMode} aria-label={t.freeMode} aria-pressed={mode==='free'} disabled={blocked||!!draft} onClick={()=>setMode('free')}>↗</button><output title={active?`${active.frame+1} · ${active.value}`:undefined}>{active?`${active.frame+1} · ${Number(active.value.toFixed(2))}`:keys.length?'':t.empty}</output></div>
  <div className="timeline-graph-batch"><button type="button" title={t.all} aria-label={t.all} disabled={blocked||!keys.length||!!draft} onClick={()=>{setSelected(keys.map(k=>k.frame));setActive(keys[0].frame);anchor.current=keys[0].frame;root.current?.focus({preventScroll:true});}}>⊞</button><output aria-label={t.count}>{selected.length}</output><select aria-label={t.interpolation} title={t.interpolation} value={interpolation} disabled={blocked||!chosen.length||!!draft} onChange={e=>void commit(selected,{kind:'interpolation',interpolation:e.target.value as TimelineKey['interpolation']})}><option value="" disabled>{t.mixed}</option><option value="hold">{t.hold}</option><option value="linear">{t.linear}</option><option value="ease">{t.ease}</option></select><button type="button" title={t.remove} aria-label={t.remove} disabled={blocked||!chosen.length||!!draft} onClick={()=>void commit(selected,{kind:'delete'})}>−</button></div>
  <div className="timeline-graph-view"><button type="button" title={t.pan} aria-label={t.pan} aria-pressed={mode==='pan'} disabled={viewing} onClick={()=>setMode(mode==='pan'?'value':'pan')}>✥</button><select title={t.axis} aria-label={t.axis} value={axis} disabled={viewing} onChange={e=>setAxis(e.target.value as GraphAxis)}><option value="both">{t.both}</option><option value="time">{t.time}</option><option value="value">{t.value}</option></select><button type="button" title={t.zoomOut} aria-label={t.zoomOut} disabled={viewing} onClick={()=>zoom(1/1.5)}>−</button><button type="button" title={t.zoomIn} aria-label={t.zoomIn} disabled={viewing} onClick={()=>zoom(1.5)}>＋</button><button type="button" title={t.fit} aria-label={t.fit} disabled={viewing} onClick={()=>setView(null)}>⤢</button><button type="button" title={t.fitSelection} aria-label={t.fitSelection} disabled={viewing||!chosen.length} onClick={()=>setView(graphFitView(chosen,base,duration,limits))}>⌖</button><span className="timeline-info" title={t.viewHint} aria-label={t.viewHint}>ⓘ</span></div>
  <svg ref={svg} viewBox={`0 0 ${GRAPH.width} ${GRAPH.height}`} role="img" aria-label={`${t.title} · ${property}`} onPointerDown={e=>{
   if((e.button===1||mode==='pan'&&e.button===0)&&!viewing){e.preventDefault();e.stopPropagation();root.current?.focus({preventScroll:true});panDrag.current={view,prior:manualView,x:e.clientX,y:e.clientY};setPanning(true);e.currentTarget.setPointerCapture(e.pointerId);return;}
   if(blocked||panning||mode==='pan'||e.button!==0||!keys.length)return;e.preventDefault();e.stopPropagation();root.current?.focus({preventScroll:true});const box=e.currentTarget.getBoundingClientRect(),x=(e.clientX-box.left)/box.width*GRAPH.width,y=(e.clientY-box.top)/box.height*GRAPH.height;
   let nearest=-1,distance=144;keys.forEach((k,i)=>{if(k.frame<view.start||k.frame>view.end||k.value<view.min||k.value>view.max)return;const p=graphPoint(k.frame,k.value,duration,range,view),d=(p.x-x)**2+(p.y-y)**2;if(d<distance){nearest=i;distance=d;}});if(nearest<0){setSelected([]);setActive(null);return;}
   const frames=select(nearest,e.shiftKey,e.metaKey||e.ctrlKey);if(e.shiftKey||e.metaKey||e.ctrlKey)return;
   const key=keys[nearest];drag.current={key,frames,delta:0,valueDelta:0,x:e.clientX,y:e.clientY,clientX:e.clientX,clientY:e.clientY,mode,targets:mode!=='value'&&snapEnabled?(getSnapTargets?.(key.frame,frames)??[]).filter(f=>!frames.includes(f)):[],bypass:e.altKey,zoom:box.width/GRAPH.width*(GRAPH.width-GRAPH.left-GRAPH.right)/(view.end-view.start)};setDraft([...keys]);setSnapped(false);e.currentTarget.setPointerCapture(e.pointerId);
  }} onPointerMove={e=>{const pan=panDrag.current;if(pan){const box=e.currentTarget.getBoundingClientRect();setView(graphPanView(pan.view,-(e.clientX-pan.x)/box.width*GRAPH.width/(GRAPH.width-GRAPH.left-GRAPH.right)*(pan.view.end-pan.view.start),(e.clientY-pan.y)/box.height*GRAPH.height/(GRAPH.height-GRAPH.top-GRAPH.bottom)*(pan.view.max-pan.view.min),duration,limits));return;}const d=drag.current;if(!d)return;d.clientX=e.clientX;d.clientY=e.clientY;d.bypass=e.altKey;update();}} onPointerUp={e=>{if(panDrag.current){panDrag.current=null;setPanning(false);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);return;}const d=drag.current;if(!d)return;cancel();if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);void commit(d.frames,graphDragEdit(d.mode,d.delta,d.valueDelta));}} onPointerCancel={cancel} onLostPointerCapture={cancel}>
   {[0,.5,1].map(n=>{const y=GRAPH.top+n*(GRAPH.height-GRAPH.top-GRAPH.bottom);return <g key={n}><line x1={GRAPH.left} x2={GRAPH.width-GRAPH.right} y1={y} y2={y} className="timeline-graph-grid"/><text x={GRAPH.left-4} y={y+3} textAnchor="end">{Number((range.max-n*(range.max-range.min)).toFixed(tickDigits))}</text></g>;})}
   <defs><clipPath id={clipId}><rect x={GRAPH.left} y={GRAPH.top} width={GRAPH.width-GRAPH.left-GRAPH.right} height={GRAPH.height-GRAPH.top-GRAPH.bottom}/></clipPath></defs>
   <g clipPath={`url(#${clipId})`}><path d={graphPath(shown,duration,range,base,view)} className="timeline-graph-curve"/>
   <line className="timeline-graph-playhead" x1={graphPoint(frame,base,duration,range,view).x} x2={graphPoint(frame,base,duration,range,view).x} y1={GRAPH.top} y2={GRAPH.height-GRAPH.bottom}/>
   {graphMarkers(visible.length,visible.findIndex(k=>k.frame===active?.frame)).map(i=>{const key=visible[i],p=graphPoint(key.frame,key.value,duration,range,view),source=keys.find(k=>k.frame===key.frame);const picked=draft&&drag.current&&drag.current.mode!=='value'?drag.current.frames.includes(key.frame-drag.current.delta):selectedSet.has(source?.frame??key.frame);return <circle key={key.frame} data-frame={key.frame} cx={p.x} cy={p.y} r={picked?4:2.5} data-selected={picked}/>;})}
   </g><text x={GRAPH.left} y={GRAPH.height-5}>{Math.min(duration,Math.ceil(view.start)+1)}</text><text x={GRAPH.width-GRAPH.right} y={GRAPH.height-5} textAnchor="end">{Math.min(duration,Math.floor(view.end)+1)}</text>
  </svg>
 </div>;
}
