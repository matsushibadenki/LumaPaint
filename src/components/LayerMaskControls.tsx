import { maskLinkLabels } from './MaskLinkButton';
import { useEffect, useRef, useState } from 'react';
import { createLayerMask, transformLayerMask, type LayerEffects, type LayerSnapshot, type MaskKind } from '../bridge';
import type { Locale } from '../i18n';
const labels={
 ja:{linked:'本体とマスクを連動',linkedHint:'本体全体を移動・変形するとマスクも追従します。',unlinkedHint:'本体とマスクを別々に移動・変形できます。再リンクしても位置は変わりません。',moveX:'横に移動 (px)',moveY:'縦に移動 (px)',scale:'拡大率 (%)',angle:'回転 (°)',apply:'移動・変形を適用',origin:'拡大・回転の基準は原稿の左上です。リンク中は本体も一緒に変形します。',tiledTransform:'タイル文書でマスクを変形するにはリンクを解除してください。',title:'レイヤーマスク',kind:'種類',pixel:'ピクセルマスク',vector:'ベクターマスク',add:'マスクを作成',replace:'選択範囲から置き換え',remove:'マスクを削除',enabled:'有効',invert:'反転',density:'濃度',hint:'選択範囲内を表示。未選択なら全体を表示します。置き換えは既存のマスクを上書きします。',tiled:'タイル文書では全体を表示するマスクを作成します。',description:'どのレイヤーにも、両種類のマスクを使えます。'},
 en:{linked:'Link layer and mask',linkedHint:'The mask follows moves and transforms of the entire layer.',unlinkedHint:'Move and transform the layer and mask independently. Relinking keeps their current positions.',moveX:'Move X (px)',moveY:'Move Y (px)',scale:'Scale (%)',angle:'Rotation (°)',apply:'Apply move / transform',origin:'Scale and rotation use the canvas top-left as origin. When linked, the layer transforms too.',tiledTransform:'Unlink the mask to transform it in tiled documents.',title:'Layer mask',kind:'Type',pixel:'Pixel mask',vector:'Vector mask',add:'Create mask',replace:'Replace from selection',remove:'Delete mask',enabled:'Enabled',invert:'Invert',density:'Density',hint:'Reveal the selection, or everything when there is no selection. Replace overwrites the current mask.',tiled:'Tiled documents create a mask revealing the entire canvas.',description:'Choose either mask type on any layer.'},
 'zh-CN':{linked:'链接图层与蒙版',linkedHint:'移动或变换整个图层时，蒙版随之变化。',unlinkedHint:'图层与蒙版可以独立移动或变换。重新链接不会改变当前位置。',moveX:'水平移动 (px)',moveY:'垂直移动 (px)',scale:'缩放 (%)',angle:'旋转 (°)',apply:'应用移动 / 变换',origin:'缩放与旋转以画布左上角为原点。链接时图层也会一起变换。',tiledTransform:'在瓦片文档中变换蒙版前请先取消链接。',title:'图层蒙版',kind:'类型',pixel:'像素蒙版',vector:'矢量蒙版',add:'创建蒙版',replace:'从选区替换',remove:'删除蒙版',enabled:'启用',invert:'反转',density:'浓度',hint:'显示选区内部。没有选区时显示全部。替换会覆盖当前蒙版。',tiled:'瓦片文档创建显示整个画布的蒙版。',description:'任意图层均可选择这两种蒙版。'},
};
export function LayerMaskControls({locale,layer,enabled,onCommit,onSnapshot,onBusyChange}:{locale:Locale;layer:LayerSnapshot;enabled:boolean;onCommit:(id:string,effects:LayerEffects)=>void|Promise<void>;onBusyChange?:(busy:boolean)=>void|Promise<void>;onSnapshot:(snapshot:import('../bridge').DocumentSnapshot)=>void}){
 const t=labels[locale],mask=layer.effects?.mask;
 const [kind,setKind]=useState<MaskKind>(mask?.kind??'pixel');
 const [busy,setBusy]=useState(false),[error,setError]=useState('');
 const running=useRef(false);
 async function run(operation:()=>void|Promise<void>){
  if(running.current||!enabled||layer.locked)return;
  running.current=true;setBusy(true);setError('');
  try{await onBusyChange?.(true);await operation();}catch(e){setError(String(e));}
  finally{try{await onBusyChange?.(false);}catch(e){setError(String(e));}running.current=false;setBusy(false);}
 }
 useEffect(()=>{setKind(mask?.kind??'pixel');setError('');},[layer.id,mask?.kind]);
 const [values,setValues]=useState(['0','0','100','0']);
 useEffect(()=>setValues(['0','0','100','0']),[layer.id]);
 const editable=enabled&&!layer.locked&&!busy;
 async function create(){await run(async()=>onSnapshot(await createLayerMask(layer.id,kind)));}
 const linked=mask?.linked??true;
 const validTransform=values.every(v=>v.trim()!==''&&Number.isFinite(Number(v)))&&Number(values[2])>=1&&Number(values[2])<=10000;
 async function transform(){
  if(!editable||!validTransform)return;
  const [x,y,scale,angle]=values.map(Number),r=angle*Math.PI/180,k=scale/100,c=Math.cos(r)*k,s=Math.sin(r)*k;
  await run(async()=>{onSnapshot(await transformLayerMask(layer.id,[c,s,-s,c,x,y]));setValues(['0','0','100','0']);});
 }
 const patch=(changes:Partial<NonNullable<typeof mask>>)=>{if(mask&&layer.effects)void run(()=>onCommit(layer.id,{...layer.effects!,mask:{...mask,...changes}}));};
 return <fieldset lang={locale} className="layer-mask-controls" disabled={!editable} aria-busy={busy}>
  <legend>{t.title}</legend><p>{t.description}</p>
  <label><span>{t.kind}</span><select aria-label={t.title} value={kind} onChange={e=>setKind(e.currentTarget.value as MaskKind)}><option value="pixel">{t.pixel}</option><option value="vector">{t.vector}</option></select></label>
  <button type="button" onClick={()=>void create()}>{mask?t.replace:t.add}</button>
  {mask&&<><label className="mask-enabled-toggle" title={maskLinkLabels[locale][linked?'unlink':'link']}><input type="checkbox" checked={linked} onChange={e=>patch({linked:e.currentTarget.checked})}/>{t.linked}</label><p>{linked?t.linkedHint:t.unlinkedHint}</p><strong>{mask.kind==='pixel'?t.pixel:t.vector}</strong><label className="mask-enabled-toggle"><input type="checkbox" checked={mask.enabled} onChange={e=>patch({enabled:e.currentTarget.checked})}/>{t.enabled}</label><label className="mask-enabled-toggle"><input type="checkbox" checked={mask.inverted} onChange={e=>patch({inverted:e.currentTarget.checked})}/>{t.invert}</label><label><span>{t.density} (%)</span><input type="number" aria-label={t.density} min={0} max={100} value={Math.round(mask.density*100)} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value)&&value>=0&&value<=100)patch({density:value/100});}}/></label><button type="button" onClick={()=>layer.effects&&void run(()=>onCommit(layer.id,{...layer.effects!,mask:null}))}>{t.remove}</button></>}
  {mask&&<details className="mask-transform-controls"><summary>{t.apply}</summary>
    {[t.moveX,t.moveY,t.scale,t.angle].map((label,index)=><label key={index}><span>{label}</span><input type="number" aria-label={label} step="any" min={index===2?1:undefined} max={index===2?10000:undefined} value={values[index]} onChange={e=>{const value=e.currentTarget.value;setValues(current=>current.map((v,i)=>i===index?value:v));}}/></label>)}
    <p>{layer.rasterBlendMode&&linked?t.tiledTransform:t.origin}</p><button type="button" disabled={!validTransform||!!layer.rasterBlendMode&&linked} onClick={()=>void transform()}>{t.apply}</button>
  </details>}
  <p>{layer.rasterBlendMode?t.tiled:t.hint}</p>{error&&<p role="alert">{error}</p>}
 </fieldset>;
}
