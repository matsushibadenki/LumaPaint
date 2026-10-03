import { useRef, useState } from 'react';
import { editGuides, type DocumentSnapshot, type GuideEdit } from '../bridge';
import type { Locale } from '../i18n';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
const labels = {
  ja: {title:'選択ガイド',position:'位置',horizontal:'水平',vertical:'垂直',dx:'移動 X',dy:'移動 Y',apply:'移動',mixed:'混在',invalid:'有効な数値を入力してください。',locked:'ガイドはロックされています。',step:'矢印キー',largeStep:'Shift＋矢印',hint:'Shift＋ドラッグで追加選択。矢印キーの移動量は右側の設定で変更できます。'},
  en: {title:'Selected guides',position:'Position',horizontal:'Horizontal',vertical:'Vertical',dx:'Move X',dy:'Move Y',apply:'Move',mixed:'Mixed',invalid:'Enter a valid number.',locked:'Guides are locked.',step:'Arrow keys',largeStep:'Shift＋arrow',hint:'Shift-drag to add guides. Set keyboard movement increments on the right.'},
  'zh-CN': {title:'所选参考线',position:'位置',horizontal:'水平',vertical:'垂直',dx:'移动 X',dy:'移动 Y',apply:'移动',mixed:'混合',invalid:'请输入有效数字。',locked:'参考线已锁定。',step:'方向键',largeStep:'Shift＋方向键',hint:'Shift 拖动可添加参考线。在右侧设置键盘移动增量。'},
};
function IncrementField({label,value,disabled,onCommit}:{label:string;value:number;disabled:boolean;onCommit:(text:string)=>void}) {
  const [draft,setDraft]=useState<string|null>(null);
  return <label>{label}<input type="number" step="any" aria-label={label} disabled={disabled} value={draft??String(Number(value.toFixed(4)))} onChange={e=>setDraft(e.target.value)} onBlur={()=>{if(draft!==null){onCommit(draft);setDraft(null);}}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){e.preventDefault();setDraft(null);}}}/></label>;
}
export function GuideOptions({document,locale,enabled,onUpdate,onError}:{document:DocumentSnapshot;locale:Locale;enabled:boolean;onUpdate:(d:DocumentSnapshot)=>void;onError:(e:string)=>void}) {
  const t=labels[locale],unit=useMeasurementUnit(),symbol=unitSymbols[unit],factor=pixelsPerMeasurement(unit,document.resolution);
  const selected=document.guides.items.filter(g=>document.guides.selected.includes(g.id));
  const first=selected[0];
  const axis=first?.axis && selected.every(g=>g.axis===first.axis)?first.axis:null;
  const origin=document.guides.origin[axis==='vertical'?0:1];
  const mixed=selected.some(g=>g.position!==first?.position);
  const [draft,setDraft]=useState<string|null>(null),[dx,setDx]=useState('0'),[dy,setDy]=useState('0'),[busy,setBusy]=useState(false);
  const pending=useRef(false);
  const disabled=!enabled||busy||document.guides.locked;
  const run=async (edit:GuideEdit)=>{if(disabled||pending.current)return;pending.current=true;setBusy(true);try{onUpdate(await editGuides(edit));setDraft(null);setDx('0');setDy('0');}catch(e){onError(String(e));}finally{pending.current=false;setBusy(false);}};
  const commit=()=>{if(draft===null)return;const value=Number(draft);if(!draft.trim()||!Number.isFinite(value)){onError(t.invalid);return;}if(axis)void run({action:'position',axis,position:value*factor+origin});};
  return <div className="selection-options" role="group" aria-label={t.title} title={t.hint}>
    <span className="selection-kind">{t.title}<small>{selected.length} · {axis?t[axis]:t.mixed}</small></span>
    {axis && <label>{t.position} ({symbol})<input type="number" step="any" aria-label={`${t.position} (${symbol})`} disabled={disabled} placeholder={mixed?t.mixed:undefined} value={draft??(mixed?'':String(Number(((first.position-origin)/factor).toFixed(3))))} onChange={e=>setDraft(e.target.value)} onBlur={commit} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){e.preventDefault();setDraft(null);}}}/></label>}
    <label>{t.dx} ({symbol})<input type="number" step="any" disabled={disabled} value={dx} onChange={e=>setDx(e.target.value)}/></label>
    <label>{t.dy} ({symbol})<input type="number" step="any" disabled={disabled} value={dy} onChange={e=>setDy(e.target.value)}/></label>
    <button disabled={disabled} onClick={()=>{const delta:[number,number]=[Number(dx)*factor,Number(dy)*factor];if(!dx.trim()||!dy.trim()||delta.some(v=>!Number.isFinite(v))){onError(t.invalid);return;}if(delta.some(v=>v!==0))void run({action:'moveSelected',delta});}}>{t.apply}</button>
    {[t.step,t.largeStep].map((label,index)=><IncrementField key={label+unit} label={`${label} (${symbol})`} value={document.guides.nudge[index]/factor} disabled={disabled} onCommit={text=>{const value=Number(text)*factor;if(!text.trim()||!Number.isFinite(value)||value<=0||value>1_000_000){onError(t.invalid);return;}if(value===document.guides.nudge[index])return;const delta:[number,number]=[...document.guides.nudge];delta[index]=value;void run({action:'increments',delta});}}/>)}
  </div>;
}
