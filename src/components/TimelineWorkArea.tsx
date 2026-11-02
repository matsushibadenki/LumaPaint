import {useEffect,useRef,useState} from 'react';
import type {Locale} from '../i18n';
export type WorkArea={start:number;end:number;enabled:boolean};
export const workAreaLabels={
 ja:{title:'ワークエリア',start:'ワークエリアの開始',end:'ワークエリアの終了',setStart:'現在フレームを開始に設定',setEnd:'現在フレームを終了に設定',reset:'ワークエリアをリセット',hint:'有効な範囲だけを再生・ループします。目盛り上の両端をドラッグ、矢印キーで1フレーム調整。終了フレームを含みます。作画・書き出し範囲は変わりません。'},
 en:{title:'Work area',start:'Work area start',end:'Work area end',setStart:'Set start to current frame',setEnd:'Set end to current frame',reset:'Reset work area',hint:'Play and loop within the enabled range. Drag the ruler handles or use arrow keys to adjust one frame. The end frame is included. Drawing and export ranges stay unchanged.'},
 'zh-CN':{title:'工作区域',start:'工作区域起点',end:'工作区域终点',setStart:'将当前帧设为起点',setEnd:'将当前帧设为终点',reset:'重置工作区域',hint:'仅播放和循环启用的范围。拖动标尺两端或用方向键逐帧调整。包含结束帧。不改变绘画和导出范围。'},
};
export function TimelineWorkAreaControls({locale,area,duration,frame,disabled,onChange}:{locale:Locale;area:WorkArea|null;duration:number;frame:number;disabled:boolean;onChange:(area:WorkArea|null)=>void}){
 const t=workAreaLabels[locale],value=area??{start:0,end:duration,enabled:false};
 const [start,setStart]=useState(String(value.start+1)),[end,setEnd]=useState(String(value.end));
 useEffect(()=>{setStart(String(value.start+1));setEnd(String(value.end));},[value.start,value.end,duration]);
 function apply(){const a=Number(start)-1,b=Number(end);if(Number.isInteger(a)&&Number.isInteger(b)&&a>=0&&a<b&&b<=duration){if(a!==value.start||b!==value.end)onChange({...value,start:a,end:b});}else{setStart(String(value.start+1));setEnd(String(value.end));}}
 return <fieldset className="timeline-work-controls" disabled={disabled} aria-label={t.title}>
  <label className="timeline-check"><input type="checkbox" aria-label={t.title} checked={value.enabled} onChange={e=>onChange({...value,enabled:e.target.checked})}/>{t.title}</label>
  <span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span>
  <button type="button" title={t.setStart} aria-label={t.setStart} onClick={()=>onChange({...value,start:frame,end:Math.max(value.end,frame+1)})}>[</button>
  <input type="number" aria-label={t.start} min={1} max={Number(end)} value={start} onChange={e=>setStart(e.target.value)} onBlur={apply} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){setStart(String(value.start+1));setEnd(String(value.end));}}}/>
  <span>–</span>
  <input type="number" aria-label={t.end} min={Number(start)} max={duration} value={end} onChange={e=>setEnd(e.target.value)} onBlur={apply} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){setStart(String(value.start+1));setEnd(String(value.end));}}}/>
  <button type="button" title={t.setEnd} aria-label={t.setEnd} onClick={()=>onChange({...value,start:Math.min(value.start,frame),end:frame+1})}>]</button>
  <button type="button" title={t.reset} aria-label={t.reset} disabled={!area} onClick={()=>onChange(null)}>↺</button>
 </fieldset>;
}
export function TimelineWorkAreaRuler({locale,area,duration,zoom,disabled,revision,onChange}:{locale:Locale;area:WorkArea|null;duration:number;zoom:number;disabled:boolean;revision:number;onChange:(area:WorkArea)=>void}){
 const t=workAreaLabels[locale],drag=useRef<{edge:'start'|'end';x:number;area:WorkArea;scroll:number;root:HTMLElement|null}|null>(null),[draft,setDraft]=useState<WorkArea|null>(null);
 function cancel(){drag.current=null;setDraft(null);}
 useEffect(()=>{cancel();},[revision,disabled,area?.start,area?.end,area?.enabled]);
 useEffect(()=>{if(!draft)return;const key=(e:KeyboardEvent)=>{if(e.key==='Escape'){e.stopPropagation();e.preventDefault();cancel();}};window.addEventListener('blur',cancel);window.addEventListener('keydown',key,true);return()=>{window.removeEventListener('blur',cancel);window.removeEventListener('keydown',key,true);};},[!!draft]);
 if(!area)return null;
 const value=draft??area;
 function range(x:number){const d=drag.current!;const delta=Math.round((x-d.x+(d.root?.scrollLeft??d.scroll)-d.scroll)/zoom);return {...d.area,[d.edge]:d.edge==='start'?Math.max(0,Math.min(d.area.end-1,d.area.start+delta)):Math.max(d.area.start+1,Math.min(duration,d.area.end+delta))};}
 return <div className="timeline-work-range" data-enabled={area.enabled} style={{left:value.start*zoom,width:(value.end-value.start)*zoom}}>
  {(['start','end'] as const).map(edge=><button key={edge} className="timeline-work-handle" data-edge={edge} aria-label={t[edge]} title={`${t[edge]} · ${edge==='start'?value.start+1:value.end}`} disabled={disabled} onClick={e=>e.stopPropagation()} onPointerDown={e=>{e.stopPropagation();if(e.button!==0||disabled)return;const root=e.currentTarget.closest<HTMLElement>('.timeline-tracks');drag.current={edge,x:e.clientX,area,root,scroll:root?.scrollLeft??0};setDraft(area);e.currentTarget.setPointerCapture(e.pointerId);}} onPointerMove={e=>{if(drag.current){e.stopPropagation();setDraft(range(e.clientX));}}} onPointerUp={e=>{e.stopPropagation();if(!drag.current)return;const next=range(e.clientX);cancel();if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(next.start!==area.start||next.end!==area.end)onChange(next);}} onPointerCancel={cancel} onLostPointerCapture={cancel} onKeyDown={e=>{if(!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();e.stopPropagation();const step=(e.key==='ArrowLeft'?-1:1)*(e.shiftKey?10:1);const next={...area,[edge]:edge==='start'?Math.max(0,Math.min(area.end-1,area.start+step)):Math.max(area.start+1,Math.min(duration,area.end+step))};if(next[edge]!==area[edge])onChange(next);}}/>) }
 </div>;
}
