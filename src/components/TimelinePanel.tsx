import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Locale } from '../i18n';
import { Icon } from './Icon';
import {OnionControls,onionLabels,type OnionSettings} from './OnionControls';
import type { MotionClip, MotionCatalog } from '../motion-types';
import {ExposureBatchControls} from './ExposureBatchControls';
import { ExposureClip } from './ExposureClip';
import {CelLibraryControls,celLabels,type CelInfo} from './CelLibraryControls';
import { visibleFrames, visibleKeys, overlapsFrames, sampleTimelineKey,exposureMoveBounds,selectExposureFrames,selectedExposurePositions,type ExposureRange } from '../timeline-view';

type Transport = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
const defaultTransport:Transport=(command,args)=>invoke(command,args);
type Property = 'x'|'y'|'scaleX'|'scaleY'|'rotation'|'opacity';
type Interpolation = 'linear'|'hold'|'ease';
type Key = {frame:number;value:number;interpolation:Interpolation};
type Layer = {id:string;name:string;pixel:boolean;locked:boolean;cels:number[];celLibrary:CelInfo[];exposures:ExposureRange[];channels:Partial<Record<Property,Key[]>>;motions:MotionClip[]};
type Data = {documentId:number;revision:number;selectedLayer?:string;fps:number;duration:number;looping:boolean;onion:OnionSettings;layers:Layer[]};
type State = {exposureSelection?:[string,number[]]|null;frame:number;playing:boolean;preview:boolean;data:Data|null;dataUnchanged?:boolean};
const properties:Property[]=['x','y','scaleX','scaleY','rotation','opacity'];
const labels={
  ja:{play:'再生',pause:'一時停止',stop:'作画に戻る',first:'最初のフレーム',previous:'前のフレーム',next:'次のフレーム',last:'最後のフレーム',frame:'フレーム',duration:'長さ',loop:'ループ',zoom:'目盛り幅',pixel:'コマ撮り',vector:'キーフレーム',capture:'作画を記録',blank:'空白コマ',duplicate:'次に複製',edit:'コマを編集',remove:'削除',key:'キーを追加 / 更新',interpolation:'補間',linear:'リニア',hold:'ホールド',ease:'イーズイン・アウト',x:'位置 X',y:'位置 Y',scaleX:'横倍率',scaleY:'縦倍率',rotation:'回転',opacity:'不透明度',preview:'プレビュー中',drawing:'作画中',empty:'ドキュメントを開いてアニメーションを作成します。',pixelHint:'作画を記録し、別のフレームでコマを追加します。記録したコマは次のコマまで表示されます。「コマを編集」で読み込んだ後、修正して再度記録してください。',vectorHint:'プロパティごとにキーを設定します。位置は元の作画からの移動量、回転・倍率の中心はキャンバス中央です。キーをドラッグして時刻を変更できます。',locked:'レイヤーはロックされています。',value:'値',apply:'設定',hint:'コマ・キーを選択して編集',busy:'更新中…'},
  en:{play:'Play',pause:'Pause',stop:'Return to drawing',first:'First frame',previous:'Previous frame',next:'Next frame',last:'Last frame',frame:'Frame',duration:'Duration',loop:'Loop',zoom:'Frame width',pixel:'Stop motion',vector:'Keyframes',capture:'Record drawing',blank:'Blank cel',duplicate:'Duplicate next',edit:'Edit cel',remove:'Delete',key:'Add / Update key',interpolation:'Interpolation',linear:'Linear',hold:'Hold',ease:'Ease in / out',x:'Position X',y:'Position Y',scaleX:'Scale X',scaleY:'Scale Y',rotation:'Rotation',opacity:'Opacity',preview:'Preview',drawing:'Drawing',empty:'Open a document to create an animation.',pixelHint:'Record a drawing, then add cels at other frames. Each cel holds until the next. Use Edit cel to load a recorded drawing, then record your changes again.',vectorHint:'Set keys for each property. Position is an offset from the artwork; scale and rotation pivot around the canvas center. Drag keys to change their time.',locked:'This layer is locked.',value:'Value',apply:'Set',hint:'Select a cel or key to edit',busy:'Updating…'},
  'zh-CN':{play:'播放',pause:'暂停',stop:'返回绘画',first:'第一帧',previous:'上一帧',next:'下一帧',last:'最后一帧',frame:'帧',duration:'时长',loop:'循环',zoom:'帧宽度',pixel:'逐帧动画',vector:'关键帧',capture:'记录绘画',blank:'空白帧',duplicate:'复制到下一帧',edit:'编辑帧',remove:'删除',key:'添加 / 更新关键帧',interpolation:'插值',linear:'线性',hold:'保持',ease:'缓入缓出',x:'位置 X',y:'位置 Y',scaleX:'水平缩放',scaleY:'垂直缩放',rotation:'旋转',opacity:'不透明度',preview:'预览中',drawing:'绘画中',empty:'打开文档以创建动画。',pixelHint:'记录绘画，然后在其他帧添加内容。每帧保持至下一帧。使用“编辑帧”载入已记录的绘画，修改后再次记录。',vectorHint:'分别设置各属性的关键帧。位置是相对原图的偏移；缩放和旋转以画布中心为基准。拖动关键帧可更改时间。',locked:'图层已锁定。',value:'数值',apply:'设置',hint:'选择帧或关键帧以编辑',busy:'正在更新…'},
};
function sample(layer:Layer|undefined,p:Property,frame:number):number {
  return sampleTimelineKey(layer?.channels[p]??[],frame,p==='scaleX'||p==='scaleY'||p==='opacity'?100:0);
}
export function TimelinePanel({locale,transport=defaultTransport}:{locale:Locale;transport?:Transport}) {
  const t=labels[locale],ot=onionLabels[locale],ct=celLabels[locale];
  const [thumbnailSize,setThumbnailSize]=useState(56);
  const [visibleLayers,setVisibleLayers]=useState<string[]>([]);
  const [thumbnails,setThumbnails]=useState<Record<string,{token:string;blank:boolean}>>({});
  const [catalog,setCatalog]=useState<MotionCatalog|null>(null);
  const motionNames=useMemo(()=>new Map(catalog?.presets.map(p=>[p.id,p.names[locale]??p.names.en])??[]),[catalog,locale]);
  const tracks=useRef<HTMLDivElement>(null);
  const [viewport,setViewport]=useState({left:0,width:500});
  const motionDrag=useRef<{x:number;layer:string;clip:MotionClip}|null>(null);
  useEffect(()=>{let alive=true;void transport<MotionCatalog>('motion_catalog').then(c=>{if(alive)setCatalog(c);}).catch(()=>{});return()=>{alive=false;};},[transport]);
  const [state,setState]=useState<State>({frame:0,playing:false,preview:false,data:null});
  const [frameInput,setFrameInput]=useState('1');
  const frameEditing=useRef(false);
  useEffect(()=>{frameEditing.current=false;setFrameInput(String(state.frame+1));},[state.data?.documentId]);
  const currentDocument=useRef<number|undefined>(undefined);
  currentDocument.current=state.data?.documentId;
  const [layerId,setLayerId]=useState('');
  const [selection,setSelection]=useState<{layer:string;frames:number[];anchor:number|null}>({layer:'',frames:[],anchor:null});
  const selectionRef=useRef(selection);
  const selectedIdentities=useRef<{documentId:number;layer:string;ids:string[];anchor:string|null}|null>(null);
  const retainedSelection=useRef<{documentId:number;revision:number;selection:[string,number[]]}|null>(null);
  const previousSelectionData=useRef<Data|null>(null);
  const [ripple,setRipple]=useState(false);
  const [capturedExposure,setCapturedExposure]=useState<{layer:string;start:number}|null>(null);
  const [groupPreview,setGroupPreview]=useState<{layer:string;source:number;delta:number}|null>(null);
  function selectExposures(layer:string,frames:number[],anchor:number|null){const next={layer,frames,anchor};const positions=new Map(state.data?.layers.find(l=>l.id===layer)?.exposures.map(e=>[e.start,e.id]));selectedIdentities.current=state.data&&frames.length?{documentId:state.data.documentId,layer,ids:frames.map(f=>positions.get(f)).filter((id):id is string=>!!id),anchor:positions.get(anchor??-1)??null}:null;selectionRef.current=next;setSelection(next);}
  useEffect(()=>{const next=state.data,previous=previousSelectionData.current,pending=retainedSelection.current;const old=selectionRef.current;
    const identities=selectedIdentities.current;
    if(next&&identities?.documentId===next.documentId&&identities.ids.length){const mapped=selectedExposurePositions(next.layers.find(l=>l.id===identities.layer)?.exposures??[],identities.ids,identities.anchor);const value={layer:identities.layer,...mapped};selectionRef.current=value;setSelection(value);}
    else if(next&&pending?.documentId===next.documentId&&pending.revision===next.revision)selectExposures(pending.selection[0],pending.selection[1],pending.selection[1][0]??null);
    else if(next&&previous?.documentId===next.documentId){const before=new Map(previous.layers.find(l=>l.id===old.layer)?.exposures.map(e=>[e.start,e.cel]));const after=new Map(next.layers.find(l=>l.id===old.layer)?.exposures.map(e=>[e.start,e.cel]));const frames=old.frames.filter(f=>before.has(f)&&before.get(f)===after.get(f));selectExposures(old.layer,frames,frames.includes(old.anchor??-1)?old.anchor:frames[0]??null);}
    else selectExposures('',[],null);
    previousSelectionData.current=next;retainedSelection.current=null;setGroupPreview(null);setCapturedExposure(null);
  },[state.data?.documentId,state.data?.revision]);
  useEffect(()=>{if(selectionRef.current.layer!==layerId)selectExposures('',[],null);},[layerId]);
  const [property,setProperty]=useState<Property>('x');
  const [value,setValue]=useState('0');
  const [interpolation,setInterpolation]=useState<Interpolation>('linear');
  const [fps,setFps]=useState('24'),[duration,setDuration]=useState('120');
  const [zoom,setZoom]=useState(16),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const alive=useRef(true),serial=useRef(Promise.resolve()),scrub=useRef<number|null>(null);
  const drag=useRef<{x:number;frame:number;layer:string;property?:Property;key?:Key}|null>(null);
  const pendingSeek=useRef<number|null>(null),seeking=useRef(false),movedKey=useRef(false);
  const command=useCallback((request:Record<string,unknown>,quiet=false):Promise<void>=>{
    const run=async()=>{
      if(!alive.current)return;
      if(!quiet){setBusy(true);setError('');}
      try {
        const result=await transport<State>('timeline',{request});
        if(alive.current&&result.data&&result.exposureSelection)retainedSelection.current={documentId:result.data.documentId,revision:result.data.revision,selection:result.exposureSelection};
        if(alive.current)setState(old=>result.data===null&&(result.dataUnchanged??request.action==='tick')&&old.frame===result.frame&&old.playing===result.playing&&old.preview===result.preview?old:({...result,data:(result.dataUnchanged??request.action==='tick')?(result.data??old.data):result.data}));
      }catch(e){if(alive.current){setError(String(e));if(request.action==='get'||request.action==='refresh')setState({frame:0,playing:false,preview:false,data:null});}}
      finally{if(!quiet&&alive.current)setBusy(false);}
    };
    const next=serial.current.then(run,run);serial.current=next;return next;
  },[transport]);
  useEffect(()=>{
    alive.current=true;void command({action:'get'});
    const events=['documents-changed','timeline-changed'];
    let refresh:ReturnType<typeof setTimeout>;
    const listeners=transport===defaultTransport?events.map(event=>getCurrentWebviewWindow().listen(event,()=>{clearTimeout(refresh);refresh=setTimeout(()=>void command({action:'refresh'},true),30);}).catch(()=>()=>{})):[];
    return()=>{const documentId=currentDocument.current;alive.current=false;clearTimeout(refresh);listeners.forEach(p=>void p.then(unlisten=>unlisten()).catch(()=>{}));if(documentId!==undefined)void serial.current.then(()=>transport('timeline',{request:{action:'stopMotion',documentId}})).catch(()=>{});};
  },[command,transport]);
  useEffect(()=>{
    if(!state.playing)return;
    let canceled=false,timer:ReturnType<typeof setTimeout>;
    async function tick(){await command({action:'tick'},true);if(!canceled)timer=setTimeout(()=>void tick(),16);}
    void tick();return()=>{canceled=true;clearTimeout(timer);};
  },[state.playing,command]);
  useEffect(()=>{if(state.data){setFps(String(state.data.fps));setDuration(String(state.data.duration));setLayerId(id=>(state.data!.layers.some(l=>l.id===state.data!.selectedLayer)?state.data!.selectedLayer:undefined)??(state.data!.layers.some(l=>l.id===id)?id:state.data!.layers.at(-1)?.id??''));}},[state.data]);
  function selectLayer(id:string){flushKeyEdit();valueEditing.current=null;setLayerId(id);if(id!==layerId)void command({action:'select',layer:id});}
  // Keep an in-progress numeric draft stable while asynchronous edit replies arrive.
  const valueEditing=useRef<string|null>(null);
  const keyEdit=useRef<{request:Record<string,unknown>;documentId:number;timer:ReturnType<typeof setTimeout>}|null>(null);
  const flushKeyEdit=useCallback(()=>{
    const pending=keyEdit.current;if(!pending)return;
    clearTimeout(pending.timer);keyEdit.current=null;
    if(currentDocument.current===pending.documentId)void command(pending.request,true);
  },[command]);
  useEffect(()=>()=>{if(keyEdit.current)clearTimeout(keyEdit.current.timer);},[]);
  const layer=state.data?.layers.find(l=>l.id===layerId);
  const editContext=`${state.data?.documentId}:${layerId}:${property}:${state.frame}`;
  useEffect(()=>{if(valueEditing.current===editContext)return;setValue(String(sample(layer,property,state.frame)));const key=layer?.channels[property]?.find(k=>k.frame===state.frame);if(key)setInterpolation(key.interpolation);},[layer,property,state.frame,editContext]);
  function updateExistingKey(text:string,mode:Interpolation){
    if(keyEdit.current){clearTimeout(keyEdit.current.timer);keyEdit.current=null;}
    if(!state.data||!layer||layer.locked||state.playing||!layer.channels[property]?.some(k=>k.frame===state.frame)||!text.trim())return;
    const numeric=Number(text);
    const [min,max]=property==='opacity'?[0,100]:property==='scaleX'||property==='scaleY'?[0.1,10000]:property==='rotation'?[-36000,36000]:[-65536,65536];
    if(!Number.isFinite(numeric)||numeric<min||numeric>max)return;
    keyEdit.current={documentId:state.data.documentId,request:{action:'key',documentId:state.data?.documentId,layer:layerId,frame:state.frame,property,value:numeric,interpolation:mode},timer:setTimeout(flushKeyEdit,120)};
  }
  const data=state.data;
  const celNames=useMemo(()=>new Map(data?.layers.flatMap(l=>(l.celLibrary??[]).map(c=>[`${l.id}:${c.id}`,c.name] as const))??[]),[data]);
  useEffect(()=>setThumbnails({}),[data?.documentId]);
  useEffect(()=>{drag.current=null;motionDrag.current=null;pendingSeek.current=null;scrub.current=null;movedKey.current=false;},[data?.documentId]);
  const seek=(frame:number)=>{
    if(!Number.isFinite(frame))return;
    flushKeyEdit();valueEditing.current=null;
    pendingSeek.current=Math.max(0,Math.min((data?.duration??1)-1,Math.round(frame)));
    if(seeking.current)return;
    seeking.current=true;
    void (async()=>{try{while(pendingSeek.current!==null&&alive.current){const next=pendingSeek.current;pendingSeek.current=null;await command({action:'seek',frame:next},true);}}finally{seeking.current=false;}})();
  };
  useEffect(()=>{
    const element=tracks.current;if(!element)return;
    const measure=()=>setViewport(old=>old.left===element.scrollLeft&&old.width===element.clientWidth?old:{left:element.scrollLeft,width:element.clientWidth});
    measure();const observer=new ResizeObserver(measure);observer.observe(element);return()=>observer.disconnect();
  },[data?.documentId,data?.duration,zoom]);
  const range=visibleFrames(viewport.left,viewport.width,zoom,data?.duration??120);
  const timelineWidth=Math.max(500,(data?.duration??120)*zoom);
  useEffect(()=>{
    const root=tracks.current;if(!root)return;
    const visible=new Set<string>();
    const observer=new IntersectionObserver(entries=>{for(const entry of entries){const id=(entry.target as HTMLElement).dataset.layerId!;if(entry.isIntersecting)visible.add(id);else visible.delete(id);}const next=[...visible].sort();setVisibleLayers(old=>old.join('|')===next.join('|')?old:next);},{root});
    root.querySelectorAll('[data-layer-id]').forEach(node=>observer.observe(node));
    return()=>observer.disconnect();
  },[data?.documentId,data?.layers.map(l=>l.id).join('|'),thumbnailSize]);
  useEffect(()=>{
    let canceled=false;
    if(!data||!thumbnailSize){setThumbnails({});return;}
    const cels=data.layers.filter(l=>l.pixel&&(visibleLayers.includes(l.id)||l.id===layerId)).flatMap(l=>(l.exposures??[]).filter(e=>overlapsFrames(e.start,e.end,range)).map(e=>[l.id,e.start] as [string,number]));
    void(async()=>{
      const next:Record<string,{token:string;blank:boolean}>={};
      try{for(let start=0;start<cels.length&&!canceled;start+=64){
        const result=await transport<Array<{layer:string;frame:number;token:string;blank:boolean}>>('timeline_cel_thumbnails',{request:{documentId:data.documentId,cels:cels.slice(start,start+64)}});
        for(const item of result)next[`${item.layer}:${item.frame}`]={token:item.token,blank:item.blank};
        if(!canceled)setThumbnails({...next});
      }}catch(e){if(!canceled)setError(String(e));}
    })();
    return()=>{canceled=true;};
  },[data,range.from,range.to,visibleLayers,layerId,thumbnailSize,transport]);

  const loadLibraryThumbnails=useCallback((ids:number[])=>transport<Array<{frame:number;token:string;blank:boolean}>>('timeline_cel_thumbnails',{request:{documentId:data?.documentId,cels:ids.map(id=>[layerId,id]),library:true}}),[transport,data?.documentId,layerId]);
  const selectedStarts=useMemo(()=>new Set(selection.frames),[selection.frames]);
  const selectedFrames=selection.layer===layerId?selection.frames:[];
  const moveBounds=useMemo(()=>exposureMoveBounds(layer?.exposures??[],selectedFrames,data?.duration??1,ripple),[layer?.exposures,selectedFrames,data?.duration,ripple]);
  function editBatch(edit:Record<string,unknown>,frames=selectionRef.current.frames){
    if(!data||selectionRef.current.layer!==layerId||!frames.length)return;
    void command({action:'exposureBatch',documentId:data.documentId,revision:data.revision,layer:layerId,frames,edit}).then(()=>{setGroupPreview(null);void command({action:'refresh'},true);});
  }
  function prepareExposure(id:string,frame:number,mode:string,event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean}){
    if(event.shiftKey||event.metaKey||event.ctrlKey)return false;
    const old=selectionRef.current;if(mode!=='move'||old.layer!==id||!old.frames.includes(frame))selectExposures(id,[frame],frame);
    if(id!==layerId)selectLayer(id);return true;
  }
  function chooseExposure(id:string,frame:number,event:{shiftKey:boolean;metaKey:boolean;ctrlKey:boolean}){
    const item=data?.layers.find(l=>l.id===id);if(!item)return;
    const old=selectionRef.current;
    const frames=selectExposureFrames(item.exposures??[],old.layer===id?old.frames:[],frame,old.layer===id?old.anchor:null,event.shiftKey,event.metaKey||event.ctrlKey);
    selectExposures(id,frames,event.shiftKey&&old.layer===id?old.anchor:frame);selectLayer(id);
    if(!event.shiftKey&&!event.metaKey&&!event.ctrlKey)seek(frame);
  }
  const disabled=busy||state.playing||!layer||layer.locked;
  const step=Math.max(1,Math.ceil(60/zoom));
  function selectFrame(event:React.PointerEvent<HTMLDivElement>) {
    const frame=Math.max(0,Math.min((data?.duration??1)-1,Math.floor((event.clientX-event.currentTarget.getBoundingClientRect().left)/zoom)));
    if(scrub.current===frame)return;scrub.current=frame;
    // Coalesce pointer movement while an IPC seek is in flight.
    if(event.type==='pointerdown')event.currentTarget.setPointerCapture(event.pointerId);seek(frame);
  }
  const keyAt=layer?.channels[property]?.some(k=>k.frame===state.frame);
  const celAt=layer?.cels.includes(state.frame);
  return <div className="timeline-panel" onKeyDown={e=>{e.stopPropagation();if((e.target as HTMLElement).closest('input,select,textarea'))return;if(!layer?.pixel)return;if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='a'){e.preventDefault();selectExposures(layerId,layer.exposures.map(x=>x.start),layer.exposures[0]?.start??null);}if(e.key==='Escape'){selectExposures('',[],null);setGroupPreview(null);}if((e.key==='Delete'||e.key==='Backspace')&&!disabled&&selectedFrames.length){e.preventDefault();editBatch({kind:'delete',ripple});}}}>
    <div className="timeline-toolbar">
      <button className="timeline-onion-toggle" aria-label={ot.title} title={ot.info} aria-pressed={data?.onion.enabled??false} disabled={!data||busy} onClick={()=>data&&void command({action:'onion',documentId:data.documentId,settings:{...data.onion,enabled:!data.onion.enabled}})}>◉ {ot.title}</button>
      <label>{ot.thumbnails}<select className="timeline-thumbnail-size" value={thumbnailSize} onChange={e=>setThumbnailSize(Number(e.target.value))}><option value={0}>{ot.none}</option>{[32,56,72].map(size=><option key={size} value={size}>{size}</option>)}</select></label>
      <div className="timeline-transport">
        <button title={t.first} aria-label={t.first} disabled={!data||busy} onClick={()=>seek(0)}>⏮</button>
        <button title={t.previous} aria-label={t.previous} disabled={!data||busy} onClick={()=>seek(state.frame-1)}>‹</button>
        <button className="timeline-play" title={state.playing?t.pause:t.play} aria-label={state.playing?t.pause:t.play} disabled={!data||busy} onClick={()=>void command({action:state.playing?'pause':'play'})}>{state.playing?'Ⅱ':'▶'}</button>
        <button title={t.next} aria-label={t.next} disabled={!data||busy} onClick={()=>seek(state.frame+1)}>›</button>
        <button title={t.last} aria-label={t.last} disabled={!data||busy} onClick={()=>seek((data?.duration??1)-1)}>⏭</button>
      </div>
      <label>{t.frame}<input aria-label={t.frame} type="number" min={1} max={data?.duration??1} value={frameEditing.current?frameInput:String(state.frame+1)} disabled={!data||busy} onFocus={()=>{frameEditing.current=true;setFrameInput(String(state.frame+1));}} onChange={e=>{setFrameInput(e.target.value);if(e.target.value)seek(Number(e.target.value)-1);}} onBlur={()=>{frameEditing.current=false;setFrameInput(String(state.frame+1));}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></label>
      <span className="timeline-time">{(state.frame/(data?.fps??24)).toFixed(2)} s</span>
      <label>fps<input aria-label="fps" type="number" min={1} max={120} value={fps} onChange={e=>setFps(e.target.value)}/></label>
      <label>{t.duration}<input type="number" min={1} max={18000} value={duration} onChange={e=>setDuration(e.target.value)}/></label>
      <button disabled={!data||busy||state.playing} onClick={()=>void command({action:'settings',fps:Number(fps),duration:Number(duration),looping:data?.looping??true})}>{t.apply}</button>
      <label className="timeline-check"><input type="checkbox" checked={data?.looping??true} disabled={!data||busy} onChange={e=>void command({action:'settings',fps:data!.fps,duration:data!.duration,looping:e.target.checked})}/>{t.loop}</label>
      <label>{t.zoom}<select value={zoom} onChange={e=>setZoom(Number(e.target.value))}>{[4,8,16,24,32].map(n=><option key={n} value={n}>{n}</option>)}</select></label>
      <button disabled={!state.preview||busy} onClick={()=>void command({action:'stop'})}>{t.stop}</button>
      <span className="timeline-mode" data-preview={state.preview}>{busy?t.busy:state.preview?t.preview:t.drawing}</span>
    </div>
    {data&&<div className="timeline-edit-toolbar">{layer?.pixel?<div className="timeline-edit-tools" role="toolbar" aria-label={layer?.pixel?t.pixel:t.vector}><button className="tool-button" title={t.capture} aria-label={t.capture} disabled={disabled||state.preview} onClick={()=>void command({action:'capture',layer:layerId,frame:state.frame,blank:false})}><Icon name="animationRecord"/></button><button className="tool-button" title={t.blank} aria-label={t.blank} disabled={disabled} onClick={()=>void command({action:'capture',layer:layerId,frame:state.frame,blank:true})}><Icon name="document"/></button><button className="tool-button" title={t.duplicate} aria-label={t.duplicate} disabled={disabled||!layer.exposures.some(e=>e.start<=state.frame&&e.end>state.frame)||state.frame>=data.duration-1} onClick={()=>void command({action:'duplicate',layer:layerId,from:state.frame,frame:state.frame+1})}><Icon name="animationDuplicate"/></button><button className="tool-button" title={t.edit} aria-label={t.edit} disabled={disabled||!layer.exposures.some(e=>e.start<=state.frame&&e.end>state.frame)} onClick={()=>void command({action:'load',layer:layerId,frame:state.frame})}><Icon name="brush"/></button><button className="tool-button" title={t.remove} aria-label={t.remove} disabled={disabled||!celAt} onClick={()=>void command({action:'removeExposure',documentId:data.documentId,layer:layerId,frame:state.frame})}><Icon name="animationDelete"/></button></div>:<div className="timeline-edit-tools" role="toolbar" aria-label={layer?.pixel?t.pixel:t.vector}><button className="tool-button" title={t.key} aria-label={t.key} disabled={disabled||value.trim()===''||!Number.isFinite(Number(value))} onClick={()=>void command({action:'key',documentId:state.data?.documentId,layer:layerId,frame:state.frame,property,value:Number(value),interpolation})}><Icon name="animationKey"/></button><button className="tool-button" title={t.remove} aria-label={t.remove} disabled={disabled||!keyAt} onClick={()=>void command({action:'remove',layer:layerId,frame:state.frame,property})}><Icon name="animationDelete"/></button></div>}</div>}
    {data&&layer?.pixel&&<ExposureBatchControls locale={locale} frames={selectedFrames} exposures={layer.exposures??[]} ripple={ripple} setRipple={setRipple} disabled={disabled} onSelectAll={()=>selectExposures(layerId,layer.exposures.map(e=>e.start),layer.exposures[0]?.start??null)} onClear={()=>selectExposures('',[],null)} onEdit={editBatch}/>}
    {data?<div className="timeline-body">
      <div className="timeline-tracks" aria-label={t.hint} ref={tracks} onScroll={e=>{const element=e.currentTarget;setViewport(old=>old.left===element.scrollLeft&&old.width===element.clientWidth?old:{left:element.scrollLeft,width:element.clientWidth});}}>
        <div className="timeline-sheet" style={{width:180+timelineWidth}}>
          <div className="timeline-row timeline-ruler"><div className="timeline-track-name">{t.frame}</div><div className="timeline-lane" style={{width:timelineWidth}} onPointerDown={selectFrame} onPointerMove={e=>{if(e.buttons===1)selectFrame(e);}} onPointerUp={e=>{if(scrub.current!==null)seek(scrub.current);scrub.current=null;if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}}>
            {Array.from({length:Math.max(0,Math.ceil(range.to/step)-Math.ceil(range.from/step))},(_,i)=>(Math.ceil(range.from/step)+i)*step).map(frame=><span className="timeline-tick" key={frame} style={{left:frame*zoom}}>{frame+1}</span>)}
          </div></div>
          {data.layers.slice().reverse().map(item=><div key={item.id} className="timeline-layer" data-layer-id={item.id} data-selected={layerId===item.id}>
            <div className="timeline-row" data-pixel={item.pixel} style={item.pixel&&thumbnailSize?{minHeight:thumbnailSize+26}:undefined}><button className="timeline-track-name" onClick={()=>selectLayer(item.id)}><span>{item.pixel?'▣':'◇'} {item.name}{item.locked?' 🔒':''}</span><small>{item.pixel?t.pixel:t.vector}</small></button><div className="timeline-lane" style={{width:timelineWidth}} onClick={e=>{selectLayer(item.id);seek(Math.floor((e.clientX-e.currentTarget.getBoundingClientRect().left)/zoom));}}>
              {item.pixel?(item.exposures??[]).map((exposure,index)=>(overlapsFrames(exposure.start,exposure.end,range)||(capturedExposure?.layer===item.id&&capturedExposure.start===exposure.start))?<ExposureClip key={`${data.documentId}:${item.id}:${exposure.id??exposure.start}`} exposure={exposure} min={item.exposures[index-1]?.end??0} max={ripple?exposure.end+18000-(item.exposures.at(-1)?.end??data.duration):(item.exposures[index+1]?.start??data.duration)} selected={selection.layer===item.id&&selectedStarts.has(exposure.start)} offset={groupPreview?.layer===item.id&&groupPreview.source!==exposure.start&&(ripple?exposure.start>=(selection.frames[0]??Infinity):selectedStarts.has(exposure.start))?groupPreview.delta:0} moveMin={exposure.start+(selection.layer===item.id&&selectedStarts.has(exposure.start)?moveBounds.min:(item.exposures[index-1]?.end??0)-exposure.start)} moveMax={exposure.end+(selection.layer===item.id&&selectedStarts.has(exposure.start)?moveBounds.max:(ripple?exposure.end+18000-(item.exposures.at(-1)?.end??data.duration):(item.exposures[index+1]?.start??data.duration))-exposure.end)} onDragging={active=>setCapturedExposure(active?{layer:item.id,start:exposure.start}:null)} onPrepare={(mode,event)=>prepareExposure(item.id,exposure.start,mode,event)} onPreview={delta=>setGroupPreview(delta===null?null:{layer:item.id,source:exposure.start,delta})} zoom={zoom} height={thumbnailSize?thumbnailSize+16:25} current={state.frame>=exposure.start&&state.frame<exposure.end} thumbnail={thumbnailSize>0} disabled={busy||state.playing||item.locked} label={`${item.name} · ${t.frame} ${exposure.start+1} · ${celNames.get(`${item.id}:${exposure.cel}`)??exposure.cel} · ${exposure.start+1}–${exposure.end}`} startLabel={`${ct.resizeStart} · ${exposure.start+1}`} endLabel={`${ct.resizeEnd} · ${exposure.end}`} onSelect={event=>chooseExposure(item.id,exposure.start,event)} onCommit={(next,mode)=>{if(mode==='move')editBatch({kind:'shift',delta:next.start-exposure.start,ripple});else if(mode==='end'&&ripple)editBatch({kind:'hold',length:next.end-next.start,ripple:true},[exposure.start]);else void command({action:'editExposure',documentId:data.documentId,layer:item.id,from:exposure.start,start:next.start,end:next.end});}}>
{thumbnailSize>0&&thumbnails[`${item.id}:${exposure.start}`]&&!thumbnails[`${item.id}:${exposure.start}`].blank?<img className="timeline-cel-thumbnail" src={convertFileSrc(thumbnails[`${item.id}:${exposure.start}`].token,'cel-preview')} alt="" draggable={false} style={{height:thumbnailSize}}/>:<span className="timeline-cel-placeholder" aria-hidden="true">{thumbnailSize>0?'▧':'●'}</span>}<span className="timeline-cel-number"><span className="timeline-cel-name" title={celNames.get(`${item.id}:${exposure.cel}`)}>{celNames.get(`${item.id}:${exposure.cel}`)??exposure.cel}</span><small>{exposure.start+1}</small></span></ExposureClip>:null):<span className="timeline-vector-bar"/>}
            </div></div>
            {!item.pixel&&(item.motions??[]).map(clip=>{const name=motionNames.get(clip.preset)??clip.preset;return <div className="timeline-row timeline-motion-row" key={clip.id}><button className="timeline-track-name" title={name} onClick={()=>{selectLayer(item.id);seek(clip.start);}}>{name}</button><div className="timeline-lane" style={{width:timelineWidth}}>{(overlapsFrames(clip.start,clip.start+clip.duration*clip.repeats,range)||motionDrag.current?.clip.id===clip.id&&motionDrag.current.layer===item.id)&&<button className="timeline-motion-clip" data-enabled={clip.enabled} data-current={state.frame>=clip.start&&state.frame<clip.start+clip.duration*clip.repeats} aria-label={`${name} · ${t.frame} ${clip.start+1}`} title={`${name} · ${clip.start+1}–${clip.start+clip.duration*clip.repeats}`} style={{left:clip.start*zoom,width:Math.max(8,clip.duration*clip.repeats*zoom-2)}} onClick={()=>{if(movedKey.current){movedKey.current=false;return;}selectLayer(item.id);seek(clip.start);}} onPointerDown={e=>{if(busy||state.playing||item.locked)return;motionDrag.current={x:e.clientX,layer:item.id,clip};e.currentTarget.setPointerCapture(e.pointerId);}} onPointerCancel={()=>{motionDrag.current=null;}} onPointerUp={e=>{const drag=motionDrag.current;motionDrag.current=null;if(!drag)return;const start=Math.max(0,Math.min(18000-drag.clip.duration*drag.clip.repeats,drag.clip.start+Math.round((e.clientX-drag.x)/zoom)));if(start!==drag.clip.start){movedKey.current=true;void command({action:'setMotion',documentId:data.documentId,layer:drag.layer,clip:{...drag.clip,start}});}if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}}>{name}</button>}</div></div>;})}
            {!item.pixel&&layerId===item.id&&properties.map(p=><div className="timeline-row timeline-property-row" key={p}><button className="timeline-track-name" data-selected={property===p} onClick={()=>setProperty(p)}>{t[p]}<span>{sample(item,p,state.frame)}</span></button><div className="timeline-lane" style={{width:timelineWidth}} onClick={e=>{setProperty(p);seek(Math.floor((e.clientX-e.currentTarget.getBoundingClientRect().left)/zoom));}}>
              {visibleKeys(item.channels[p]??[],range,drag.current?.layer===item.id&&drag.current.property===p?drag.current.key:undefined).map(key=><button key={key.frame} className="timeline-key" data-current={key.frame===state.frame&&property===p} title={`${t[p]} · ${key.frame+1} · ${key.value}`} aria-label={`${t[p]} · ${t.frame} ${key.frame+1}`} style={{left:key.frame*zoom}} onClick={e=>{e.stopPropagation();if(movedKey.current){movedKey.current=false;return;}setProperty(p);seek(key.frame);}} onPointerDown={e=>{if(disabled)return;e.stopPropagation();drag.current={x:e.clientX,frame:key.frame,layer:item.id,property:p,key};e.currentTarget.setPointerCapture(e.pointerId);}} onPointerUp={e=>{const d=drag.current;drag.current=null;if(!d||!d.property||!d.key)return;const frame=Math.max(0,Math.min(data.duration-1,d.frame+Math.round((e.clientX-d.x)/zoom)));if(frame!==d.frame){movedKey.current=true;void command({action:'moveKey',layer:d.layer,from:d.frame,frame,property:d.property});}if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}}>{key.interpolation==='hold'?'■':'◆'}</button>)}
            </div></div>)}
          </div>)}
          <div className="timeline-playhead" style={{left:180+state.frame*zoom}} aria-hidden="true"/>
        </div>
      </div>
      <aside className="timeline-inspector">
        <div className="timeline-inspector-title"><strong>{layer?.name??t.hint}</strong><span>{layer?.pixel?t.pixel:t.vector}</span></div>
        {layer?.locked?<p>{t.locked}</p>:layer?.pixel?<>
          <span className="timeline-info" title={t.pixelHint} aria-label={t.pixelHint}>ⓘ</span>
        </>:layer?<>
          <label>{t.value}<select value={property} onChange={e=>{flushKeyEdit();valueEditing.current=null;setProperty(e.target.value as Property);}}>{properties.map(p=><option key={p} value={p}>{t[p]}</option>)}</select><input aria-label={t[property]} type="number" step="0.1" value={value} disabled={disabled} onFocus={()=>{valueEditing.current=editContext;}} onChange={e=>{setValue(e.target.value);updateExistingKey(e.target.value,interpolation);}} onBlur={()=>{valueEditing.current=null;if(keyAt&&!keyEdit.current)setValue(String(sample(layer,property,state.frame)));flushKeyEdit();if(keyAt)void command({action:'refresh'},true);}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></label>
          <label>{t.interpolation}<select value={interpolation} disabled={disabled} onChange={e=>{const mode=e.target.value as Interpolation;setInterpolation(mode);updateExistingKey(value,mode);flushKeyEdit();}}>{(['linear','hold','ease'] as const).map(i=><option key={i} value={i}>{t[i]}</option>)}</select></label>
          <p>{t.vectorHint}</p>
        </>:null}
        {layer?.pixel&&data&&<CelLibraryControls key={`library:${data.documentId}:${layerId}`} locale={locale} loadThumbnails={loadLibraryThumbnails} revision={data.revision} library={layer.celLibrary??[]} exposure={(layer.exposures??[]).find(e=>e.start<=state.frame&&e.end>state.frame)} frame={state.frame} duration={data.duration} disabled={disabled} onCommand={request=>void command({...request,documentId:data.documentId,layer:layerId})}/>}
        {layer?.pixel&&data&&<OnionControls key={`onion:${data.documentId}:${layerId}`} locale={locale} settings={data.onion} frame={state.frame} disabled={busy} onChange={settings=>void command({action:'onion',documentId:data.documentId,settings})}/>}
      </aside>
    </div>:<p className="timeline-empty">{t.empty}</p>}
    {error&&<div className="timeline-error" role="alert">{error}</div>}
  </div>;
}
