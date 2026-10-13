import {useRef,useState,type ReactNode,type PointerEvent} from 'react';
import {dragExposure,type ExposureRange,type ExposureDragMode} from '../timeline-view';
export function ExposureClip({exposure,min,max,zoom,height,current,thumbnail,label,startLabel,endLabel,disabled,selected=false,offset=0,moveMin=min,moveMax=max,onPrepare,onPreview,onSelect,onCommit,children}:{exposure:ExposureRange;min:number;max:number;zoom:number;height:number;current:boolean;thumbnail:boolean;label:string;startLabel:string;endLabel:string;disabled:boolean;selected?:boolean;offset?:number;moveMin?:number;moveMax?:number;onPrepare?:(mode:ExposureDragMode,event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>boolean;onPreview?:(delta:number|null)=>void;onSelect:(event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean})=>void;onCommit:(range:ExposureRange,mode:ExposureDragMode)=>void;children:ReactNode}){
 const drag=useRef<{x:number;mode:ExposureDragMode;original:ExposureRange}|null>(null);
 const [draft,setDraft]=useState<ExposureRange|null>(null);
 const suppressClick=useRef(false);
 const shown=draft??{...exposure,start:exposure.start+offset,end:exposure.end+offset};
 const bounds=(mode:ExposureDragMode)=>mode==='move'?[moveMin,moveMax]:[min,max];
 function begin(e:PointerEvent<HTMLButtonElement>,mode:ExposureDragMode){e.stopPropagation();if(disabled||e.button!==0||onPrepare?.(mode,e)===false)return;drag.current={x:e.clientX,mode,original:exposure};e.currentTarget.setPointerCapture(e.pointerId);}
 function move(e:PointerEvent<HTMLButtonElement>){const d=drag.current;if(!d)return;const [lo,hi]=bounds(d.mode);const range=dragExposure(d.original,(e.clientX-d.x)/zoom,d.mode,lo,hi);setDraft(range);if(d.mode==='move')onPreview?.(range.start-d.original.start);}
 function end(e:PointerEvent<HTMLButtonElement>){const d=drag.current;drag.current=null;setDraft(null);onPreview?.(null);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(!d)return;const [lo,hi]=bounds(d.mode);const range=dragExposure(d.original,(e.clientX-d.x)/zoom,d.mode,lo,hi);if(range.start!==d.original.start||range.end!==d.original.end){suppressClick.current=d.mode==='move';onCommit(range,d.mode);}}
 function cancel(){drag.current=null;setDraft(null);onPreview?.(null);suppressClick.current=false;}
 function key(e:React.KeyboardEvent<HTMLButtonElement>,mode:ExposureDragMode){if(e.key==='Escape'){e.preventDefault();e.stopPropagation();cancel();return;}if(disabled||!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();e.stopPropagation();if(onPrepare?.(mode,{shiftKey:false,metaKey:false,ctrlKey:false})===false)return;const [lo,hi]=bounds(mode);const range=dragExposure(exposure,(e.key==='ArrowLeft'?-1:1)*(e.shiftKey?10:1),mode,lo,hi);if(range.start!==exposure.start||range.end!==exposure.end)onCommit(range,mode);}
 const events=(mode:ExposureDragMode)=>({onPointerDown:(e:PointerEvent<HTMLButtonElement>)=>begin(e,mode),onPointerMove:move,onPointerUp:end,onPointerCancel:cancel,onLostPointerCapture:()=>{if(drag.current)cancel();},onKeyDown:(e:React.KeyboardEvent<HTMLButtonElement>)=>key(e,mode)});
 return <div className="timeline-cel timeline-exposure" data-selected={selected} data-current={current} data-thumbnail={thumbnail} data-dragging={!!draft} style={{left:shown.start*zoom,width:Math.max(3,(shown.end-shown.start)*zoom-2),height}}>
  <button className="timeline-exposure-face" aria-label={label} title={label} onClick={e=>{e.stopPropagation();if(suppressClick.current){suppressClick.current=false;return;}onSelect(e);}} {...events('move')}>{children}</button>
  <button className="timeline-exposure-handle" data-edge="start" aria-label={startLabel} title={startLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('start')}/>
  <button className="timeline-exposure-handle" data-edge="end" aria-label={endLabel} title={endLabel} disabled={disabled} onClick={e=>e.stopPropagation()} {...events('end')}/>
  {draft&&<span className="timeline-exposure-range">{shown.start+1}–{shown.end}</span>}
 </div>;
}
