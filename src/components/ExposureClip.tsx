import {useEffect,useRef,useState,type ReactNode,type PointerEvent} from 'react';
import {dragExposure,timelineEdgeScrollSpeed,exposureDragGesture,snapExposureDelta,type ExposureSnapPlan,type ExposureRange,type ExposureDragMode} from '../timeline-view';
export function ExposureClip({exposure,min,max,zoom,height,current,thumbnail,label,startLabel,endLabel,disabled,selected=false,preview,endMax=max,offset=0,moveMin=min,moveMax=max,onPrepare,onPreview,onDragging,onSelect,onCommit,revision,reordering=false,reorderMax=max,snapEnabled=false,getSnapPlan,onReorderPreview,onReorder,children}:{exposure:ExposureRange;min:number;max:number;zoom:number;height:number;current:boolean;thumbnail:boolean;label:string;startLabel:string;endLabel:string;disabled:boolean;selected?:boolean;preview?:ExposureRange;endMax?:number;offset?:number;moveMin?:number;moveMax?:number;revision?:number;reordering?:boolean;reorderMax?:number;snapEnabled?:boolean;getSnapPlan?:(mode:ExposureDragMode)=>ExposureSnapPlan;onReorderPreview?:(frame:number|null)=>void;onReorder?:(frame:number)=>void;onPrepare?:(mode:ExposureDragMode,event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>boolean;onPreview?:(delta:number|null,mode:ExposureDragMode)=>void;onDragging?:(active:boolean)=>void;onSelect:(event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>void;onCommit:(range:ExposureRange,mode:ExposureDragMode)=>void;children:ReactNode}){
 const drag=useRef<{x:number;mode:ExposureDragMode;original:ExposureRange;root:HTMLElement|null;scroll:number;clientX:number;reordering:boolean;bypass:boolean;snap:boolean;plan:ExposureSnapPlan}|null>(null);
 const [draft,setDraft]=useState<ExposureRange|null>(null);
 const [snapped,setSnapped]=useState(false);
 const suppressClick=useRef(false);
 const animation=useRef<number|null>(null);
 const live=useRef({zoom,bounds:(mode:ExposureDragMode)=>mode==='move'?[moveMin,moveMax]:[min,mode==='end'?endMax:max],onPreview,onReorderPreview});
 live.current={zoom,bounds:(mode)=>mode==='move'?[moveMin,moveMax]:[min,mode==='end'?endMax:max],onPreview,onReorderPreview};
 function stopScroll(){if(animation.current!==null)cancelAnimationFrame(animation.current);animation.current=null;}
 useEffect(()=>()=>stopScroll(),[]);
 useEffect(()=>{if(drag.current)cancel();},[exposure.id,exposure.start,exposure.end,exposure.cel,disabled,reordering,revision]);
 function updateDraft(){
  const d=drag.current;if(!d)return;
  const [lo,hi]=d.reordering?[0,reorderMax+d.original.end-d.original.start]:live.current.bounds(d.mode);
  if(!exposureDragGesture(d.clientX-d.x,(d.root?.scrollLeft??d.scroll)-d.scroll)){setDraft(d.original);setSnapped(false);live.current.onPreview?.(null,d.mode);return d.original;}
  const raw=(d.clientX-d.x+(d.root?.scrollLeft??d.scroll)-d.scroll)/live.current.zoom;
  const minimum=d.mode==='end'?d.original.start+1-d.original.end:lo-d.original.start;
  const maximum=d.mode==='start'?d.original.end-1-d.original.start:hi-d.original.end;
  const result=snapExposureDelta(raw,d.plan,live.current.zoom,minimum,maximum,d.snap&&!d.bypass&&!d.reordering,exposureDragGesture(d.clientX-d.x,(d.root?.scrollLeft??d.scroll)-d.scroll));
  const range=dragExposure(d.original,result.delta,d.mode,lo,hi);setDraft(range);setSnapped(result.snapped);
  live.current.onPreview?.(d.mode==='end'?range.end-d.original.end:range.start-d.original.start,d.mode);
  if(d.mode==='move'){if(d.reordering)live.current.onReorderPreview?.((range.start+range.end)/2);}return range;
 }
 function scrollTick(time:number,last=time){const d=drag.current;if(!d)return;const root=d.root;if(root){const rect=root.getBoundingClientRect();const speed=timelineEdgeScrollSpeed(d.clientX,rect.left,rect.left+root.clientWidth);const before=root.scrollLeft;root.scrollLeft+=speed*Math.min(32,Math.max(0,time-last))/1000;if(root.scrollLeft!==before)updateDraft();}animation.current=requestAnimationFrame(next=>scrollTick(next,time));}
 const shown=preview??draft??{...exposure,start:exposure.start+offset,end:exposure.end+offset};
 const bounds=(mode:ExposureDragMode)=>mode==='move'?[moveMin,moveMax]:[min,mode==='end'?endMax:max];
 function begin(e:PointerEvent<HTMLButtonElement>,mode:ExposureDragMode){suppressClick.current=false;e.stopPropagation();if(disabled||e.button!==0||onPrepare?.(mode,e)===false)return;const root=e.currentTarget.closest<HTMLElement>('.timeline-tracks');drag.current={x:e.clientX,mode,original:exposure,root,scroll:root?.scrollLeft??0,clientX:e.clientX,reordering:mode==='move'&&reordering,bypass:e.altKey,snap:snapEnabled,plan:snapEnabled?getSnapPlan?.(mode)??{targets:[],edges:[]}:{targets:[],edges:[]}};setDraft(exposure);setSnapped(false);e.currentTarget.setPointerCapture(e.pointerId);onDragging?.(true);animation.current=requestAnimationFrame(time=>scrollTick(time));}
 function move(e:PointerEvent<HTMLButtonElement>){const d=drag.current;if(!d)return;d.clientX=e.clientX;d.bypass=e.altKey;updateDraft();}
 function end(e:PointerEvent<HTMLButtonElement>){const d=drag.current;if(!d)return;d.clientX=e.clientX;d.bypass=e.altKey;const range=updateDraft()!;suppressClick.current=d.mode==='move'&&exposureDragGesture(d.clientX-d.x,(d.root?.scrollLeft??d.scroll)-d.scroll,range.start!==d.original.start);stopScroll();drag.current=null;setDraft(null);setSnapped(false);onPreview?.(null,d.mode);onReorderPreview?.(null);onDragging?.(false);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(d.reordering){if(suppressClick.current)onReorder?.((range.start+range.end)/2);}else if(range.start!==d.original.start||range.end!==d.original.end||d.mode==='end'&&exposureDragGesture(d.clientX-d.x,(d.root?.scrollLeft??d.scroll)-d.scroll)){onCommit(range,d.mode);}}
 function cancel(){const wasDragging=drag.current!==null;stopScroll();drag.current=null;setDraft(null);setSnapped(false);onPreview?.(null,'move');onReorderPreview?.(null);onDragging?.(false);if(wasDragging)suppressClick.current=true;}
 function key(e:React.KeyboardEvent<HTMLButtonElement>,mode:ExposureDragMode){if(e.key==='Escape'){e.preventDefault();e.stopPropagation();cancel();return;}if(disabled||!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();e.stopPropagation();if(onPrepare?.(mode,{shiftKey:false,metaKey:false,ctrlKey:false})===false)return;const [lo,hi]=bounds(mode);const range=dragExposure(exposure,(e.key==='ArrowLeft'?-1:1)*(e.shiftKey?10:1),mode,lo,hi);if(range.start!==exposure.start||range.end!==exposure.end)onCommit(range,mode);}
 const active=draft!==null;
 useEffect(()=>{
  if(!active)return;
  const keys=(event:KeyboardEvent)=>{if(!drag.current)return;if(event.key==='Escape'){event.preventDefault();event.stopPropagation();cancel();}else if(event.key==='Alt'){drag.current.bypass=event.altKey;updateDraft();}};
  window.addEventListener('blur',cancel);window.addEventListener('keydown',keys,true);window.addEventListener('keyup',keys,true);
  return()=>{window.removeEventListener('blur',cancel);window.removeEventListener('keydown',keys,true);window.removeEventListener('keyup',keys,true);};
 },[active]);
 const events=(mode:ExposureDragMode)=>({onPointerDown:(e:PointerEvent<HTMLButtonElement>)=>begin(e,mode),onPointerMove:move,onPointerUp:end,onPointerCancel:cancel,onLostPointerCapture:()=>{if(drag.current)cancel();},onKeyDown:(e:React.KeyboardEvent<HTMLButtonElement>)=>key(e,mode)});
 return <div className="timeline-cel timeline-exposure" data-reordering={reordering} data-selected={selected} data-current={current} data-thumbnail={thumbnail} data-dragging={!!draft} data-snapped={snapped} style={{left:shown.start*zoom,width:Math.max(3,(shown.end-shown.start)*zoom-2),height}}>
  <button className="timeline-exposure-face" aria-label={label} title={label} onClick={e=>{e.stopPropagation();if(suppressClick.current){suppressClick.current=false;return;}onSelect(e);}} {...events('move')}>{children}</button>
  <button className="timeline-exposure-handle" data-edge="start" aria-label={startLabel} title={startLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('start')}/>
  <button className="timeline-exposure-handle" data-edge="end" aria-label={endLabel} title={endLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('end')}/>
  {draft&&<span className="timeline-exposure-range">{shown.start+1}–{shown.end}</span>}
 </div>;
}
