import {useEffect,useRef,useState,type ReactNode,type PointerEvent} from 'react';
import {dragExposure,timelineEdgeScrollSpeed,type ExposureRange,type ExposureDragMode} from '../timeline-view';
export function ExposureClip({exposure,min,max,zoom,height,current,thumbnail,label,startLabel,endLabel,disabled,selected=false,offset=0,moveMin=min,moveMax=max,onPrepare,onPreview,onDragging,onSelect,onCommit,children}:{exposure:ExposureRange;min:number;max:number;zoom:number;height:number;current:boolean;thumbnail:boolean;label:string;startLabel:string;endLabel:string;disabled:boolean;selected?:boolean;offset?:number;moveMin?:number;moveMax?:number;onPrepare?:(mode:ExposureDragMode,event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>boolean;onPreview?:(delta:number|null)=>void;onDragging?:(active:boolean)=>void;onSelect:(event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>void;onCommit:(range:ExposureRange,mode:ExposureDragMode)=>void;children:ReactNode}){
 const drag=useRef<{x:number;mode:ExposureDragMode;original:ExposureRange;root:HTMLElement|null;scroll:number;clientX:number}|null>(null);
 const [draft,setDraft]=useState<ExposureRange|null>(null);
 const suppressClick=useRef(false);
 const animation=useRef<number|null>(null);
 const live=useRef({zoom,bounds:(mode:ExposureDragMode)=>mode==='move'?[moveMin,moveMax]:[min,max],onPreview});
 live.current={zoom,bounds:(mode)=>mode==='move'?[moveMin,moveMax]:[min,max],onPreview};
 function stopScroll(){if(animation.current!==null)cancelAnimationFrame(animation.current);animation.current=null;}
 useEffect(()=>()=>stopScroll(),[]);
 useEffect(()=>{if(drag.current)cancel();},[exposure.id,exposure.start,exposure.end,exposure.cel,disabled]);
 function updateDraft(){const d=drag.current;if(!d)return;const [lo,hi]=live.current.bounds(d.mode);const range=dragExposure(d.original,(d.clientX-d.x+(d.root?.scrollLeft??d.scroll)-d.scroll)/live.current.zoom,d.mode,lo,hi);setDraft(range);if(d.mode==='move')live.current.onPreview?.(range.start-d.original.start);return range;}
 function scrollTick(time:number,last=time){const d=drag.current;if(!d)return;const root=d.root;if(root){const rect=root.getBoundingClientRect();const speed=timelineEdgeScrollSpeed(d.clientX,rect.left,rect.left+root.clientWidth);const before=root.scrollLeft;root.scrollLeft+=speed*Math.min(32,Math.max(0,time-last))/1000;if(root.scrollLeft!==before)updateDraft();}animation.current=requestAnimationFrame(next=>scrollTick(next,time));}
 const shown=draft??{...exposure,start:exposure.start+offset,end:exposure.end+offset};
 const bounds=(mode:ExposureDragMode)=>mode==='move'?[moveMin,moveMax]:[min,max];
 function begin(e:PointerEvent<HTMLButtonElement>,mode:ExposureDragMode){e.stopPropagation();if(disabled||e.button!==0||onPrepare?.(mode,e)===false)return;const root=e.currentTarget.closest<HTMLElement>('.timeline-tracks');drag.current={x:e.clientX,mode,original:exposure,root,scroll:root?.scrollLeft??0,clientX:e.clientX};e.currentTarget.setPointerCapture(e.pointerId);onDragging?.(true);animation.current=requestAnimationFrame(time=>scrollTick(time));}
 function move(e:PointerEvent<HTMLButtonElement>){const d=drag.current;if(!d)return;d.clientX=e.clientX;updateDraft();}
 function end(e:PointerEvent<HTMLButtonElement>){const d=drag.current;if(!d)return;d.clientX=e.clientX;const range=updateDraft()!;stopScroll();drag.current=null;setDraft(null);onPreview?.(null);onDragging?.(false);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(range.start!==d.original.start||range.end!==d.original.end){suppressClick.current=d.mode==='move';onCommit(range,d.mode);}}
 function cancel(){stopScroll();drag.current=null;setDraft(null);onPreview?.(null);onDragging?.(false);suppressClick.current=false;}
 function key(e:React.KeyboardEvent<HTMLButtonElement>,mode:ExposureDragMode){if(e.key==='Escape'){e.preventDefault();e.stopPropagation();cancel();return;}if(disabled||!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();e.stopPropagation();if(onPrepare?.(mode,{shiftKey:false,metaKey:false,ctrlKey:false})===false)return;const [lo,hi]=bounds(mode);const range=dragExposure(exposure,(e.key==='ArrowLeft'?-1:1)*(e.shiftKey?10:1),mode,lo,hi);if(range.start!==exposure.start||range.end!==exposure.end)onCommit(range,mode);}
 const events=(mode:ExposureDragMode)=>({onPointerDown:(e:PointerEvent<HTMLButtonElement>)=>begin(e,mode),onPointerMove:move,onPointerUp:end,onPointerCancel:cancel,onLostPointerCapture:()=>{if(drag.current)cancel();},onKeyDown:(e:React.KeyboardEvent<HTMLButtonElement>)=>key(e,mode)});
 return <div className="timeline-cel timeline-exposure" data-selected={selected} data-current={current} data-thumbnail={thumbnail} data-dragging={!!draft} style={{left:shown.start*zoom,width:Math.max(3,(shown.end-shown.start)*zoom-2),height}}>
  <button className="timeline-exposure-face" aria-label={label} title={label} onClick={e=>{e.stopPropagation();if(suppressClick.current){suppressClick.current=false;return;}onSelect(e);}} {...events('move')}>{children}</button>
  <button className="timeline-exposure-handle" data-edge="start" aria-label={startLabel} title={startLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('start')}/>
  <button className="timeline-exposure-handle" data-edge="end" aria-label={endLabel} title={endLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('end')}/>
  {draft&&<span className="timeline-exposure-range">{shown.start+1}–{shown.end}</span>}
 </div>;
}
