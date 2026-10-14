import {useMemo,useState} from 'react';
import type {Locale} from '../i18n';
import type {ExposureRange} from '../timeline-view';
export const batchLabels={
 ja:{title:'露出の選択編集',selected:'選択',all:'全選択',clear:'選択解除',ripple:'リップル',move:'まとめて移動',delta:'移動量',hold:'保持長さ',setHold:'長さを変更',remove:'選択露出を削除',reorder:'並べ替え',dragReorder:'ドラッグで並べ替え',before:'挿入先',last:'最後へ',frame:'フレーム',hint:'⌘/Ctrlクリックで追加・解除、Shiftクリックで範囲選択。本体ドラッグで選択をまとめて移動。リップル移動は選択の先頭以降すべて、長さ変更は後続の露出も追従します。ドラッグ並べ替えをオンにすると挿入線の位置へ並べ替えます。Escapeでキャンセル。並べ替えはセルと保持長さを移し、隙間は位置ごとに保ちます。'},
 en:{title:'Edit exposures',selected:'Selected',all:'Select all',clear:'Clear selection',ripple:'Ripple',move:'Move selection',delta:'Offset',hold:'Hold length',setHold:'Set length',remove:'Delete selected exposures',reorder:'Reorder',dragReorder:'Drag to reorder',before:'Insert before',last:'To end',frame:'Frame',hint:'⌘/Ctrl-click toggles selection; Shift-click selects a range. Drag the body to move the selection. Ripple move shifts everything from the first selected exposure; length changes shift subsequent exposures. Enable drag reordering to drop at the insertion line; Escape cancels. Reordering moves cels with their hold lengths, keeping gaps by slot.'},
 'zh-CN':{title:'编辑选中曝光',selected:'已选',all:'全选',clear:'取消选择',ripple:'波纹',move:'移动所选',delta:'偏移',hold:'保持长度',setHold:'修改长度',remove:'删除所选曝光',reorder:'重新排序',dragReorder:'拖动排序',before:'插入到之前',last:'移至最后',frame:'帧',hint:'⌘/Ctrl单击添加或取消选择，Shift单击选择范围。拖动主体整体移动。波纹移动会移动首个所选曝光及之后全部曝光；修改长度时后续曝光跟随。启用拖动排序，将曝光放到插入线处；Escape取消。排序携带画格及保持长度，间隙按位置保留。'},
};
export function ExposureBatchControls({locale,frames,exposures,ripple,setRipple,dragReorder=false,setDragReorder,disabled,onSelectAll,onClear,onEdit}:{locale:Locale;frames:number[];exposures:ExposureRange[];dragReorder?:boolean;setDragReorder?:(value:boolean)=>void;ripple:boolean;setRipple:(value:boolean)=>void;disabled:boolean;onSelectAll:()=>void;onClear:()=>void;onEdit:(edit:Record<string,unknown>)=>void}){
 const t=batchLabels[locale];const[offset,setOffset]=useState('1'),[length,setLength]=useState('1'),[before,setBefore]=useState('end');const valid=(s:string,min:number,max:number)=>!!s.trim()&&Number.isInteger(Number(s))&&Number(s)>=min&&Number(s)<=max;
 const available=useMemo(()=>{const selected=new Set(frames);return exposures.filter(e=>!selected.has(e.start));},[exposures,frames]);const target=before==='end'||available.some(e=>String(e.start)===before)?before:'end';
 return <div className="timeline-batch-tools" role="toolbar" aria-label={t.title}>
  <span>{t.selected}: {frames.length}</span><span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span>
  <button disabled={disabled||!exposures.length} onClick={onSelectAll}>{t.all}</button><button disabled={disabled||!frames.length} onClick={onClear}>{t.clear}</button>
  {setDragReorder&&<button type="button" aria-label={t.dragReorder} title={t.dragReorder} aria-pressed={dragReorder} disabled={disabled||exposures.length<2} onClick={()=>setDragReorder(!dragReorder)}>⇄</button>}
  <label className="timeline-check"><input type="checkbox" aria-label={t.ripple} checked={ripple} disabled={disabled} onChange={e=>setRipple(e.target.checked)}/>{t.ripple}</label>
  <label>{t.delta}<input aria-label={t.delta} type="number" min={-18000} max={18000} value={offset} onChange={e=>setOffset(e.target.value)}/></label><button disabled={disabled||!frames.length||!valid(offset,-18000,18000)||Number(offset)===0} onClick={()=>onEdit({kind:'shift',delta:Number(offset),ripple})}>{t.move}</button>
  <label>{t.hold}<input aria-label={t.hold} type="number" min={1} max={18000} value={length} onChange={e=>setLength(e.target.value)}/></label><button disabled={disabled||!frames.length||!valid(length,1,18000)} onClick={()=>onEdit({kind:'hold',length:Number(length),ripple})}>{t.setHold}</button>
  <label>{t.before}<select aria-label={t.before} value={target} onChange={e=>setBefore(e.target.value)}><option value="end">{t.last}</option>{available.map(e=><option key={e.start} value={e.start}>{t.frame} {e.start+1}</option>)}</select></label><button disabled={disabled||!frames.length||frames.length===exposures.length} onClick={()=>onEdit({kind:'reorder',before:target==='end'?null:Number(target)})}>{t.reorder}</button>
  <button disabled={disabled||!frames.length} onClick={()=>onEdit({kind:'delete',ripple})}>{t.remove}</button>
 </div>;
}
