import { BrushSettings } from './BrushSettings';
import {RetouchControls,defaultRetouchSettings,isRetouch} from './RetouchControls';
import {SelectionPathControls,defaultSelectionPathSettings,isPathSelection} from './SelectionPathControls';
import { PaintBucketControls, defaultPaintBucketSettings } from './PaintBucketControls';
import { CloneStampControls, defaultCloneStampSettings } from './CloneStampControls';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import { useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { defaultVectorText, setTextObject, type Brush, type CanvasTool, type DocumentSnapshot, type TextSettings } from '../bridge';
import { messages, type Locale } from '../i18n';
import { MAX_BRUSH_SIZE } from './BrushControls';
import { workspaceMessages } from '../workspace-i18n';
import { ColorPickerPopover } from './ColorPickerPopover';
import './tool-settings.css';
export const toolSettingsLabels = {
  ja:{preview:'プレビュー',title:'ツール設定',hint:'クリックで選択。選択中のアイコンをクリック、またはダブルクリックで設定。長押し・右クリックでツール切替。',apply:'適用',cancel:'キャンセル',size:'直径・線幅 (px)',hardness:'硬さ (%)',x:'X (px)',y:'Y (px)',width:'幅 (px)',height:'高さ (px)',zoom:'表示倍率 (%)',reset:'表示位置をリセット',font:'フォント',fontSize:'文字サイズ (px)',content:'文字内容',color:'カラー',edit:'キャンバスではドラッグやハンドルで操作できます。',loading:'設定を読み込み中…'},
  en:{preview:'Preview',title:'Tool Settings',hint:'Click to select. Click the active icon or double-click for settings. Hold or right-click to switch tools.',apply:'Apply',cancel:'Cancel',size:'Diameter / stroke (px)',hardness:'Hardness (%)',x:'X (px)',y:'Y (px)',width:'Width (px)',height:'Height (px)',zoom:'Zoom (%)',reset:'Reset view position',font:'Font',fontSize:'Font size (px)',content:'Text',color:'Color',edit:'Use dragging and handles on the canvas.',loading:'Loading settings…'},
  'zh-CN':{preview:'预览',title:'工具设置',hint:'单击选择。再次单击已选图标或双击打开设置。长按或右键切换工具。',apply:'应用',cancel:'取消',size:'直径／线宽 (px)',hardness:'硬度 (%)',x:'X (px)',y:'Y (px)',width:'宽度 (px)',height:'高度 (px)',zoom:'缩放 (%)',reset:'重置视图位置',font:'字体',fontSize:'字号 (px)',content:'文字内容',color:'颜色',edit:'在画布上拖动或使用控制点操作。',loading:'正在读取设置…'},
};
export function ToolSettingsDialog({tool,locale,document:doc,brush:initialBrush,onUpdate,onBrush,onZoom,onClose}:{tool:CanvasTool;locale:Locale;document:DocumentSnapshot;brush?:Brush;onUpdate:(doc:DocumentSnapshot)=>void;onBrush?:(brush:Brush)=>void;onZoom?:(zoom:number)=>void;onClose:()=>void}){
  const measurementUnit=useMeasurementUnit(), factor=pixelsPerMeasurement(measurementUnit,doc.resolution), symbol=unitSymbols[measurementUnit];
  const t=toolSettingsLabels[locale]; const ref=useRef<HTMLDialogElement>(null);
  const [retouchSettings,setRetouchSettings]=useState(defaultRetouchSettings);
  const [pathSettings,setPathSettings]=useState(defaultSelectionPathSettings);
  const [bucketSettings,setBucketSettings]=useState(defaultPaintBucketSettings);
  const [stampSettings,setStampSettings]=useState(defaultCloneStampSettings);
  const [brush,setBrush]=useState<Brush>(initialBrush??{size:16,hardness:1,color:[32,32,32]});
  const [preview,setPreview]=useState(false);const [previewError,setPreviewError]=useState('');const previewQueue=useRef<Promise<unknown>>(Promise.resolve());
  const [zoom,setZoom]=useState(100);const [loading,setLoading]=useState(isTauri());const [busy,setBusy]=useState(false);const [error,setError]=useState('');
  const [bounds,setBounds]=useState(tool==='crop'?[0,0,doc.width,doc.height]:[48,48,100,100]);
  const textTool=tool==='text'||tool==='textVertical'||tool==='textFrame'||tool==='textFrameVertical';
  const shape=['crop','rectangle','ellipse','vectorRectangle','vectorEllipse','imageFrameRectangle','imageFrameEllipse'].includes(tool);
  const view=['zoomIn','zoomOut','hand'].includes(tool);
  const sample=tool==='eyedropper';
  const [text,setText]=useState<TextSettings>(()=>{
    const existing=doc.textObjects.find(o=>doc.selectedVectorObjects.includes(o.id)&&o.editable);
    return existing?structuredClone(existing):{id:null,text:{...defaultVectorText,noColor:initialBrush?.noColor,content:locale==='ja'?'テキスト':locale==='en'?'Text':'文字',pointText:tool==='text'||tool==='textVertical',writingMode:tool.includes('Vertical')?'vertical':'horizontal',boxWidth:tool.includes('Vertical')?100:240,boxHeight:tool.includes('Frame')||tool.includes('Vertical')?240:null},position:[48,48],color:initialBrush?.color??[32,32,32]};
  });
  useEffect(()=>{if(!isTauri())ref.current?.show();let live=true;if(isTauri())void invoke<{brush:Brush;zoom:number;cropBounds?:[number,number,number,number]}>('tool_options').then(v=>{if(live){setBrush(v.brush);setZoom(v.zoom*100);if(tool==='crop'&&v.cropBounds)setBounds(v.cropBounds);if(!text.id)setText(previous=>({...previous,color:v.brush.color}));}}).catch(e=>{if(live)setError(String(e));}).finally(()=>{if(live)setLoading(false);});return()=>{live=false;};},[]);
  useEffect(()=>{
    if(!isTauri())return;
    let cancelled=false;
    const timer=window.setTimeout(()=>{
      const valid=!loading&&ref.current?.querySelector('form')?.checkValidity();
      const request=(isPathSelection(tool)||tool==='cloneStamp'||tool==='paintBucket')||!preview||!valid?null:textTool?{kind:'text',settings:{id:text.id,text:text.text,position:text.position,color:text.color}}:shape?{kind:'numeric',tool,bounds}:view?{kind:'zoom',zoom:zoom/100}:sample?{kind:'sample',x:bounds[0],y:bounds[1]}:{kind:'brush',tool,brush};
      previewQueue.current=previewQueue.current.catch(()=>{}).then(()=>cancelled?undefined:invoke('tool_preview',{request})).then(()=>{if(!cancelled)setPreviewError('');}).catch(error=>{if(!cancelled)setPreviewError(String(error));});
    },preview?140:0);
    return()=>{cancelled=true;window.clearTimeout(timer);};
  },[preview,loading,tool,brush,zoom,text,bounds]);
  const number=(label:string,value:number,min:number,max:number,change:(value:number)=>void)=>{const f=label.includes('(px)')?factor:1;return <label key={`${label}-${symbol}`}>{label.replace('(px)',`(${symbol})`)}<input type="number" required min={min/f} max={max/f} step="any" value={Number.isNaN(value)?'':Number((value/f).toFixed(5))} onChange={e=>change(e.target.valueAsNumber*f)}/></label>;};
  async function apply(){
    if(textTool) onUpdate(await setTextObject(text));
    else if(shape) onUpdate(await invoke<DocumentSnapshot>('numeric_tool',{tool,bounds}));
    else if(sample) await invoke('sample_tool_point',{x:bounds[0],y:bounds[1]});
    else if(view) {if(isTauri())await invoke('set_tool_zoom',{zoom:zoom/100});else onZoom?.(zoom/100);}
    else {if(isTauri()){if(isPathSelection(tool)){await invoke('set_selection_tool_settings',{settings:pathSettings});return;}if(isRetouch(tool))await invoke('set_retouch_settings',{settings:retouchSettings});if(tool==='paintBucket')await invoke('set_paint_bucket_settings',{settings:bucketSettings});if(tool==='cloneStamp')await invoke('set_clone_stamp_settings',{settings:stampSettings});await invoke('set_tool_brush',{brush});}else onBrush?.(brush);}
  }
  return <dialog open={isTauri()} ref={ref} className="tool-settings-dialog" aria-labelledby="tool-settings-title" onCancel={e=>{e.preventDefault();if(!busy)onClose();}}><form onSubmit={async e=>{e.preventDefault();setBusy(true);setError('');try{if(isTauri())await invoke('modal_busy',{busy:true});await previewQueue.current;await apply();if(isTauri())await invoke('modal_busy',{busy:false});onClose();}catch(e){setError(String(e));}finally{setBusy(false);if(isTauri())void invoke('modal_busy',{busy:false}).catch(()=>{});}}}>
    <h3 id="tool-settings-title">{tool==="zoomIn"||tool==="zoomOut"||tool==="hand"?messages[locale][tool]:workspaceMessages[locale][tool]} · {t.title}</h3>
    <fieldset disabled={busy||loading}>
    {loading&&<p role="status">{t.loading}</p>}
    {(shape||sample)&&<div className="tool-settings-grid">{bounds.slice(0,sample?2:4).map((v,i)=>number([t.x,t.y,t.width,t.height][i],v,tool==='crop'?(i<2?0:1):(i<2?-65536:.01),tool==='crop'?(i===0?doc.width-1:i===1?doc.height-1:i===2?doc.width-bounds[0]:doc.height-bounds[1]):65536,n=>setBounds(values=>values.map((v,j)=>i===j?n:v))))}</div>}
    {view&&<>{number(t.zoom,zoom,3.13,64000,setZoom)}{tool==='hand'&&<button type="button" onClick={()=>{void invoke('reset_canvas_pan').catch(e=>setError(String(e)));}}>{t.reset}</button>}</>}
    {isPathSelection(tool)&&<SelectionPathControls locale={locale} tool={tool} details onError={setError} onChange={setPathSettings}/>}
    {!isPathSelection(tool)&&!shape&&!view&&!textTool&&!sample&&<>
      {tool!=='paintBucket'&&number(t.size,brush.size,1,MAX_BRUSH_SIZE,size=>setBrush(v=>({...v,size})))}
      {tool!=='paintBucket'&&number(t.hardness,brush.hardness*100,0,100,n=>setBrush(v=>({...v,hardness:n/100})))}
      {(tool==='brush'||tool==='eraser')&&<BrushSettings brush={brush} locale={locale} onChange={setBrush} eraser={tool==='eraser'}/>}

      {tool==='paintBucket'&&<PaintBucketControls locale={locale} layers={doc.layers} details onError={setError} onChange={setBucketSettings}/>}

      {isRetouch(tool)?<RetouchControls tool={tool} locale={locale} details onError={setError} onChange={setRetouchSettings}/>:tool==='cloneStamp'?<CloneStampControls locale={locale} details onError={setError} onChange={setStampSettings}/>:<div className="tool-settings-color"><span>{t.color}</span><ColorPickerPopover locale={locale} color={brush.color} label={t.color} noColor={brush.noColor} onNone={()=>setBrush(v=>({...v,noColor:true}))} onChange={color=>setBrush(v=>({...v,color,noColor:false}))}/></div>}
    </>}
    {textTool&&<>
      <label>{t.content}<textarea required value={text.text.content} onChange={e=>setText(v=>({...v,text:{...v.text,content:e.target.value}}))}/></label>
      <label>{t.font}<input required value={text.text.fontFamily} onChange={e=>setText(v=>({...v,text:{...v.text,fontFamily:e.target.value}}))}/></label>
      {number(t.fontSize,text.text.fontSize,1,4096,fontSize=>setText(v=>({...v,text:{...v.text,fontSize}})))}
      <div className="tool-settings-grid">{text.position.map((v,i)=>number([t.x,t.y][i],v,-65536,65536,n=>setText(v=>({...v,position:v.position.map((x,j)=>i===j?n:x) as [number,number]}))))}
      {!text.text.pointText&&<>{number(t.width,text.text.boxWidth,1,65536,boxWidth=>setText(v=>({...v,text:{...v.text,boxWidth}})))}{number(t.height,text.text.boxHeight??240,1,65536,boxHeight=>setText(v=>({...v,text:{...v.text,boxHeight}})))}</>}</div>
      <ColorPickerPopover locale={locale} color={text.color} label={t.color} noColor={text.text.noColor} onNone={()=>setText(v=>({...v,text:{...v.text,noColor:true,runs:v.text.runs?.map(run=>({...run,style:{...run.style,noColor:true}}))}}))} onChange={color=>setText(v=>({...v,color,text:{...v.text,noColor:false,runs:v.text.runs?.map(run=>({...run,style:{...run.style,noColor:false,color}}))}}))}/>
    </>}
    </fieldset>{!isRetouch(tool)&&!isPathSelection(tool)&&tool!=='cloneStamp'&&tool!=='paintBucket'&&<label className="tool-preview"><input type="checkbox" checked={preview} disabled={busy||loading||!isTauri()} onChange={e=>setPreview(e.target.checked)}/>{t.preview}</label>}<p className="tool-settings-hint">{t.edit}</p>{(error||previewError)&&<p role="alert">{error||previewError}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button disabled={busy||loading}>{t.apply}</button></footer>
  </form></dialog>;
}
