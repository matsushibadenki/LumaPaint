import {useEffect,useRef,useState} from 'react';
import type {Locale} from '../i18n';
import type {ExposureRange} from '../timeline-view';
export type CelInfo={id:number;name:string;uses:number};
export const celLabels={
 ja:{library:'セル',create:'セルを追加',assign:'セルを配置',rename:'セル名',delete:'セルを削除',fork:'セルを分離',exposure:'露出',start:'開始',end:'終了',apply:'区間を変更',remove:'露出を削除',resizeStart:'保持開始をドラッグ',resizeEnd:'保持終了をドラッグ',hint:'本体をドラッグして移動、左右の端で保持区間を変更。矢印キーでも1フレームずつ調整できます。重なる配置はできません。セルの修正は同じセルを使う全露出に反映します。「セルを分離」でこの露出だけを修正できます。',uses:'配置'},
 en:{library:'Cels',create:'Add cel',assign:'Assign cel',rename:'Cel name',delete:'Delete cel',fork:'Make cel unique',exposure:'Exposure',start:'Start',end:'End',apply:'Set range',remove:'Delete exposure',resizeStart:'Drag hold start',resizeEnd:'Drag hold end',hint:'Drag the body to move or the edges to resize. Arrow keys adjust one frame. Exposures cannot overlap. Editing a cel updates all its exposures. Make cel unique before editing one exposure separately.',uses:'uses'},
 'zh-CN':{library:'画格',create:'添加画格',assign:'配置画格',rename:'画格名称',delete:'删除画格',fork:'分离画格',exposure:'曝光',start:'开始',end:'结束',apply:'修改区间',remove:'删除曝光',resizeStart:'拖动保持开始',resizeEnd:'拖动保持结束',hint:'拖动主体移动，拖动两端调整保持区间。方向键每次调整一帧。曝光不能重叠。修改画格会更新所有引用；单独修改前请选择分离画格。',uses:'配置'},
};
export function CelLibraryControls({locale,library,exposure,frame,duration,disabled,onCommand}:{locale:Locale;library:CelInfo[];exposure:ExposureRange|undefined;frame:number;duration:number;disabled:boolean;onCommand:(request:Record<string,unknown>)=>void}){
 const t=celLabels[locale];
 const librarySize=useRef(library.length);
 const [selected,setSelected]=useState(exposure?.cel??library[0]?.id??0);
 const cel=library.find(c=>c.id===selected);
 const [name,setName]=useState(cel?.name??'');
 const [start,setStart]=useState(String((exposure?.start??frame)+1)),[end,setEnd]=useState(String(exposure?.end??frame+1));
 useEffect(()=>{setSelected(library.length>librarySize.current?(library.at(-1)?.id??0):(exposure?.cel??library[0]?.id??0));librarySize.current=library.length;},[exposure?.cel,library.length]);
 useEffect(()=>{setName(cel?.name??'');},[cel?.id,cel?.name]);
 useEffect(()=>{setStart(String((exposure?.start??frame)+1));setEnd(String(exposure?.end??frame+1));},[exposure?.start,exposure?.end,frame]);
 return <fieldset className="timeline-cel-library" disabled={disabled}>
  <legend>{t.library} <span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span></legend>
  <select aria-label={t.library} value={selected} onChange={e=>setSelected(Number(e.target.value))}>{!library.length&&<option value={0}>—</option>}{library.map(c=><option key={c.id} value={c.id}>{c.name} · {c.uses} {t.uses}</option>)}</select>
  <div className="timeline-cel-actions"><button onClick={()=>onCommand({action:'createCel'})}>{t.create}</button><button disabled={!cel||disabled} onClick={()=>onCommand({action:'assignCel',frame,cel:selected})}>{t.assign}</button></div>
  <label>{t.rename}<input aria-label={t.rename} maxLength={64} value={name} disabled={!cel||disabled} onChange={e=>setName(e.target.value)} onBlur={()=>{if(cel&&name.trim()&&name!==cel.name)onCommand({action:'renameCel',cel:selected,name:name.trim()});else setName(cel?.name??'');}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></label>
  <div className="timeline-cel-actions"><button disabled={!cel||cel.uses>0||disabled} onClick={()=>onCommand({action:'deleteCel',cel:selected})}>{t.delete}</button><button disabled={!exposure||disabled} onClick={()=>onCommand({action:'forkCel',frame})}>{t.fork}</button></div>
  {exposure&&<><div className="timeline-onion-grid"><label>{t.start}<input aria-label={`${t.exposure} · ${t.start}`} type="number" min={1} max={duration} value={start} onChange={e=>setStart(e.target.value)}/></label><label>{t.end}<input aria-label={`${t.exposure} · ${t.end}`} type="number" min={1} max={duration} value={end} onChange={e=>setEnd(e.target.value)}/></label></div><div className="timeline-cel-actions"><button disabled={disabled||!start.trim()||!end.trim()||!Number.isInteger(Number(start))||!Number.isInteger(Number(end))||Number(start)<1||Number(end)<Number(start)||Number(end)>duration} onClick={()=>onCommand({action:'editExposure',from:exposure.start,start:Number(start)-1,end:Number(end)})}>{t.apply}</button><button onClick={()=>onCommand({action:'removeExposure',frame:exposure.start})}>{t.remove}</button></div></>}
 </fieldset>;
}
