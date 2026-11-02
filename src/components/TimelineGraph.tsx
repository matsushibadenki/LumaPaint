import {useEffect,useMemo,useRef,useState} from 'react';
import type {Locale} from '../i18n';
import type {TimelineKey} from '../timeline-view';
import {GRAPH,graphRange,graphPoint,graphValue,graphPath,graphMarkers} from '../timeline-graph';
const labels={
 ja:{title:'値グラフ',empty:'キーなし',hint:'キーを上下にドラッグして値を編集。←→でキー選択、↑↓で値を1ずつ変更（Shiftで10）。Escapeで取消。時刻はタイムラインで変更します。'},
 en:{title:'Value graph',empty:'No keys',hint:'Drag a key vertically to edit its value. Left / Right selects keys; Up / Down changes values by 1 (Shift: 10). Escape cancels. Edit timing in the timeline.'},
 'zh-CN':{title:'数值曲线',empty:'无关键帧',hint:'上下拖动关键帧修改数值。左右键选择关键帧，上下键每次修改1（Shift为10）。Escape取消。时间在时间轴中调整。'},
};
export function TimelineGraph({locale,keys,duration,frame,base,min,max,disabled,revision,property,onSelect,onCommit}:{locale:Locale;keys:readonly TimelineKey[];duration:number;frame:number;base:number;min:number;max:number;disabled:boolean;revision:number;property:string;onSelect:(frame:number)=>void;onCommit:(key:TimelineKey)=>void}){
 const t=labels[locale],root=useRef<HTMLDivElement>(null),svg=useRef<SVGSVGElement>(null),drag=useRef<{key:TimelineKey;value:number;y:number}|null>(null);
 const [selected,setSelected]=useState<number|null>(null),[draft,setDraft]=useState<TimelineKey|null>(null);
 const range=useMemo(()=>graphRange(keys,base),[keys,base]),index=keys.findIndex(k=>k.frame===(selected??frame)),active=draft??keys[index];
 const shown=useMemo(()=>draft?keys.map(k=>k.frame===draft.frame?draft:k):keys,[keys,draft]);
 function cancel(){drag.current=null;setDraft(null);}
 useEffect(()=>{cancel();},[revision,disabled,duration]);
 useEffect(()=>{if(!drag.current&&keys.some(k=>k.frame===frame))setSelected(frame);},[frame,keys]);
 useEffect(()=>{if(!draft)return;const blur=()=>cancel(),escape=(e:KeyboardEvent)=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();cancel();}};window.addEventListener('blur',blur);window.addEventListener('keydown',escape,true);return()=>{window.removeEventListener('blur',blur);window.removeEventListener('keydown',escape,true);};},[!!draft]);
 function value(y:number){const box=svg.current!.getBoundingClientRect(),d=drag.current!;return graphValue(graphPoint(d.key.frame,d.key.value,duration,range).y+(y-d.y)/box.height*GRAPH.height,range,min,max);}
 function select(index:number){const key=keys[index];if(key){setSelected(key.frame);onSelect(key.frame);}}
 return <div className="timeline-graph" ref={root} tabIndex={0} role="group" aria-label={`${t.title} · ${property}`} aria-disabled={disabled} onKeyDown={e=>{
  if(e.key==='Escape'){cancel();e.stopPropagation();return;}if(!keys.length||disabled)return;
  if(['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){e.preventDefault();e.stopPropagation();select(e.key==='Home'?0:e.key==='End'?keys.length-1:Math.max(0,Math.min(keys.length-1,index<0?(e.key==='ArrowLeft'?keys.length-1:0):index+(e.key==='ArrowLeft'?-1:1))));}
  if(['ArrowUp','ArrowDown'].includes(e.key)&&active){e.preventDefault();e.stopPropagation();const value=Math.max(min,Math.min(max,Math.round((active.value+(e.key==='ArrowUp'?1:-1)*(e.shiftKey?10:1))*100)/100));if(value!==active.value)onCommit({...active,value});}
 }}>
  <div className="timeline-graph-heading"><strong>{t.title}</strong><span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span><output>{active?`${active.frame+1} · ${Number(active.value.toFixed(2))}`:keys.length?'':t.empty}</output></div>
  <svg ref={svg} viewBox={`0 0 ${GRAPH.width} ${GRAPH.height}`} role="img" aria-label={`${t.title} · ${property}`} onPointerDown={e=>{
   if(disabled||e.button!==0||!keys.length)return;e.preventDefault();e.stopPropagation();root.current?.focus({preventScroll:true});const box=e.currentTarget.getBoundingClientRect(),x=(e.clientX-box.left)/box.width*GRAPH.width,y=(e.clientY-box.top)/box.height*GRAPH.height;
   let nearest=-1,distance=144;keys.forEach((k,i)=>{const p=graphPoint(k.frame,k.value,duration,range),d=(p.x-x)**2+(p.y-y)**2;if(d<distance){nearest=i;distance=d;}});if(nearest<0)return;
   select(nearest);const key=keys[nearest];drag.current={key,value:key.value,y:e.clientY};setDraft(key);e.currentTarget.setPointerCapture(e.pointerId);
  }} onPointerMove={e=>{const d=drag.current;if(!d)return;d.value=value(e.clientY);setDraft({...d.key,value:d.value});}} onPointerUp={e=>{const d=drag.current;if(!d)return;const next={...d.key,value:d.value};cancel();if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(next.value!==d.key.value)onCommit(next);}} onPointerCancel={cancel} onLostPointerCapture={cancel}>
   {[0,.5,1].map(n=>{const y=GRAPH.top+n*(GRAPH.height-GRAPH.top-GRAPH.bottom);return <g key={n}><line x1={GRAPH.left} x2={GRAPH.width-GRAPH.right} y1={y} y2={y} className="timeline-graph-grid"/><text x={GRAPH.left-4} y={y+3} textAnchor="end">{Number((range.max-n*(range.max-range.min)).toFixed(1))}</text></g>;})}
   <path d={graphPath(shown,duration,range,base)} className="timeline-graph-curve"/>
   <line className="timeline-graph-playhead" x1={graphPoint(frame,base,duration,range).x} x2={graphPoint(frame,base,duration,range).x} y1={GRAPH.top} y2={GRAPH.height-GRAPH.bottom}/>
   {graphMarkers(keys.length,index).map(i=>{const key=shown[i],p=graphPoint(key.frame,key.value,duration,range);return <circle key={key.frame} data-frame={key.frame} cx={p.x} cy={p.y} r={i===index?4:2.5} data-selected={i===index}/>;})}
   <text x={GRAPH.left} y={GRAPH.height-5}>1</text><text x={GRAPH.width-GRAPH.right} y={GRAPH.height-5} textAnchor="end">{duration}</text>
  </svg>
 </div>;
}
