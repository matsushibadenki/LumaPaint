import {useEffect,useId,useLayoutEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {colorPickerVisibilityEvent} from './ColorPickerPopover';
import type {Locale} from '../i18n';
import type {TimelineSnapFilters} from '../timeline-view';
import {defaultTimelineSnapFilters} from '../timeline-view';
import {Icon} from './Icon';

export const timelineSnapFilterLabels={
 ja:{title:'吸着先',open:'スナップ対象を選択',all:'すべて',playhead:'再生ヘッド',keys:'キーフレーム',exposures:'コマの境界',motions:'クリップの境界',bounds:'時間軸の端',hint:'キー・コマ・クリップの移動と伸縮に共通の吸着先です。磁石で有効・無効、Optionで一時解除。設定はこのパネルを閉じるまで保持します。',keep:'吸着先を1種類以上選択してください。'},
 en:{title:'Snap targets',open:'Choose snap targets',all:'All',playhead:'Playhead',keys:'Keyframes',exposures:'Cel boundaries',motions:'Clip boundaries',bounds:'Timeline boundaries',hint:'Shared targets for key, exposure and clip moves or trims. Use the magnet to toggle snapping; hold Alt / Option to bypass. Settings last until this panel closes.',keep:'Keep at least one target selected.'},
 'zh-CN':{title:'吸附目标',open:'选择吸附目标',all:'全部',playhead:'播放头',keys:'关键帧',exposures:'画格边界',motions:'片段边界',bounds:'时间轴边界',hint:'关键帧、曝光和片段的移动或修剪共用这些目标。磁铁切换吸附，按住Alt / Option暂时关闭。设置保留至此面板关闭。',keep:'请至少选择一种目标。'},
};
const kinds=['playhead','keys','exposures','motions','bounds'] as const;
export function TimelineSnapControls({locale,label,enabled,filters,disabled,onToggle,onChange}:{locale:Locale;label:string;enabled:boolean;filters:TimelineSnapFilters;disabled:boolean;onToggle:()=>void;onChange:(filters:TimelineSnapFilters)=>void}){
 const t=timelineSnapFilterLabels[locale],id=useId(),root=useRef<HTMLDivElement>(null),trigger=useRef<HTMLButtonElement>(null);
 const popup=useRef<HTMLDivElement>(null),announced=useRef(false);
 const [position,setPosition]=useState({left:16,top:16});
 const [open,setOpen]=useState(false),count=kinds.filter(kind=>filters[kind]).length;
 useEffect(()=>{
  if(!open)return;
  const outside=(event:PointerEvent)=>{const target=event.target as Node;if(!root.current?.contains(target)&&!popup.current?.contains(target))close();};
  const blur=()=>close();
  const escape=(event:KeyboardEvent)=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();trigger.current?.focus();}};
  window.addEventListener('pointerdown',outside,true);window.addEventListener('blur',blur);window.addEventListener('keydown',escape,true);
  return()=>{window.removeEventListener('pointerdown',outside,true);window.removeEventListener('blur',blur);window.removeEventListener('keydown',escape,true);};
 },[open]);
 // Share the existing native-canvas popup occlusion channel.
 function announceClose(){if(announced.current){announced.current=false;window.dispatchEvent(new CustomEvent(colorPickerVisibilityEvent,{detail:{open:false}}));}}
 function close(){announceClose();setOpen(false);}
 useLayoutEffect(()=>()=>announceClose(),[]);
 useLayoutEffect(()=>{if(disabled)close();},[disabled]);
 useLayoutEffect(()=>{
  if(!open)return;
  const place=()=>{
   const rect=trigger.current?.getBoundingClientRect(),box=popup.current;if(!rect||!box)return;
   const left=Math.max(16,Math.min(rect.right-box.offsetWidth,window.innerWidth-box.offsetWidth-16));
   const below=rect.bottom+4,top=below+box.offsetHeight<=window.innerHeight-16?below:Math.max(16,rect.top-box.offsetHeight-4);
   setPosition({left,top});
  };
  place();popup.current?.querySelector<HTMLInputElement>('input:not(:disabled)')?.focus({preventScroll:true});
  window.addEventListener('resize',place);window.addEventListener('scroll',place,true);
  return()=>{window.removeEventListener('resize',place);window.removeEventListener('scroll',place,true);};
 },[open]);
 useLayoutEffect(()=>{
  if(!open||!popup.current)return;
  const box=popup.current.getBoundingClientRect();announced.current=true;
  window.dispatchEvent(new CustomEvent(colorPickerVisibilityEvent,{detail:{open:true,rect:{left:box.left,top:box.top,right:box.right,bottom:box.bottom}}}));
 },[open,position]);
 return <div className="timeline-snap-controls" ref={root} onKeyDown={event=>{if(event.key==='Escape'&&open){event.preventDefault();event.stopPropagation();close();trigger.current?.focus();}}}>
  <button type="button" className="timeline-snap-toggle" aria-label={label} title={label} aria-pressed={enabled} disabled={disabled} onClick={onToggle}><Icon name="magneticLasso"/></button>
  <button type="button" className="timeline-snap-options" ref={trigger} aria-label={t.open} title={`${t.open} · ${kinds.filter(kind=>filters[kind]).map(kind=>t[kind]).join(' / ')}`} data-custom={count!==kinds.length} aria-haspopup="dialog" aria-expanded={open} aria-controls={id} disabled={disabled} onClick={()=>{if(open)close();else setOpen(true);}}><Icon name="chevronDown"/></button>
  {open&&createPortal(<div id={id} ref={popup} className="timeline-snap-popover" role="dialog" aria-label={t.title} style={{left:position.left,top:position.top}}>
   <div className="timeline-snap-heading"><strong>{t.title}</strong><span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span><button type="button" disabled={count===kinds.length} onClick={()=>onChange({...defaultTimelineSnapFilters})}>{t.all}</button></div>
   {kinds.map(kind=><label key={kind} title={filters[kind]&&count===1?t.keep:undefined}><input type="checkbox" checked={filters[kind]} disabled={filters[kind]&&count===1} onChange={event=>onChange({...filters,[kind]:event.target.checked})}/><span>{t[kind]}</span></label>)}
  </div>,document.body)}
 </div>;
}
