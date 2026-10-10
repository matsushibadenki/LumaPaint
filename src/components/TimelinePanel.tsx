import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Locale } from '../i18n';
import { Icon } from './Icon';

type Property = 'x'|'y'|'scaleX'|'scaleY'|'rotation'|'opacity';
type Interpolation = 'linear'|'hold'|'ease';
type Key = {frame:number;value:number;interpolation:Interpolation};
type Layer = {id:string;name:string;pixel:boolean;locked:boolean;cels:number[];channels:Partial<Record<Property,Key[]>>};
type Data = {selectedLayer?:string;fps:number;duration:number;looping:boolean;layers:Layer[]};
type State = {frame:number;playing:boolean;preview:boolean;data:Data|null};
const properties:Property[]=['x','y','scaleX','scaleY','rotation','opacity'];
const labels={
  ja:{play:'再生',pause:'一時停止',stop:'作画に戻る',first:'最初のフレーム',previous:'前のフレーム',next:'次のフレーム',last:'最後のフレーム',frame:'フレーム',duration:'長さ',loop:'ループ',zoom:'目盛り幅',pixel:'コマ撮り',vector:'キーフレーム',capture:'作画を記録',blank:'空白コマ',duplicate:'次に複製',edit:'コマを編集',remove:'削除',key:'キーを追加 / 更新',interpolation:'補間',linear:'リニア',hold:'ホールド',ease:'イーズイン・アウト',x:'位置 X',y:'位置 Y',scaleX:'横倍率',scaleY:'縦倍率',rotation:'回転',opacity:'不透明度',preview:'プレビュー中',drawing:'作画中',empty:'ドキュメントを開いてアニメーションを作成します。',pixelHint:'作画を記録し、別のフレームでコマを追加します。記録したコマは次のコマまで表示されます。「コマを編集」で読み込んだ後、修正して再度記録してください。',vectorHint:'プロパティごとにキーを設定します。位置は元の作画からの移動量、回転・倍率の中心はキャンバス中央です。キーをドラッグして時刻を変更できます。',locked:'レイヤーはロックされています。',value:'値',apply:'設定',hint:'コマ・キーを選択して編集',busy:'更新中…'},
  en:{play:'Play',pause:'Pause',stop:'Return to drawing',first:'First frame',previous:'Previous frame',next:'Next frame',last:'Last frame',frame:'Frame',duration:'Duration',loop:'Loop',zoom:'Frame width',pixel:'Stop motion',vector:'Keyframes',capture:'Record drawing',blank:'Blank cel',duplicate:'Duplicate next',edit:'Edit cel',remove:'Delete',key:'Add / Update key',interpolation:'Interpolation',linear:'Linear',hold:'Hold',ease:'Ease in / out',x:'Position X',y:'Position Y',scaleX:'Scale X',scaleY:'Scale Y',rotation:'Rotation',opacity:'Opacity',preview:'Preview',drawing:'Drawing',empty:'Open a document to create an animation.',pixelHint:'Record a drawing, then add cels at other frames. Each cel holds until the next. Use Edit cel to load a recorded drawing, then record your changes again.',vectorHint:'Set keys for each property. Position is an offset from the artwork; scale and rotation pivot around the canvas center. Drag keys to change their time.',locked:'This layer is locked.',value:'Value',apply:'Set',hint:'Select a cel or key to edit',busy:'Updating…'},
  'zh-CN':{play:'播放',pause:'暂停',stop:'返回绘画',first:'第一帧',previous:'上一帧',next:'下一帧',last:'最后一帧',frame:'帧',duration:'时长',loop:'循环',zoom:'帧宽度',pixel:'逐帧动画',vector:'关键帧',capture:'记录绘画',blank:'空白帧',duplicate:'复制到下一帧',edit:'编辑帧',remove:'删除',key:'添加 / 更新关键帧',interpolation:'插值',linear:'线性',hold:'保持',ease:'缓入缓出',x:'位置 X',y:'位置 Y',scaleX:'水平缩放',scaleY:'垂直缩放',rotation:'旋转',opacity:'不透明度',preview:'预览中',drawing:'绘画中',empty:'打开文档以创建动画。',pixelHint:'记录绘画，然后在其他帧添加内容。每帧保持至下一帧。使用“编辑帧”载入已记录的绘画，修改后再次记录。',vectorHint:'分别设置各属性的关键帧。位置是相对原图的偏移；缩放和旋转以画布中心为基准。拖动关键帧可更改时间。',locked:'图层已锁定。',value:'数值',apply:'设置',hint:'选择帧或关键帧以编辑',busy:'正在更新…'},
};
function sample(layer:Layer|undefined,p:Property,frame:number):number {
  const keys=layer?.channels[p]??[];
  if(!keys.length)return p==='scaleX'||p==='scaleY'||p==='opacity'?100:0;
  const right=keys.findIndex(k=>k.frame>frame);
  if(right===0)return keys[0].value;
  if(right<0)return keys[keys.length-1].value;
  const a=keys[right-1],b=keys[right];let t=(frame-a.frame)/(b.frame-a.frame);
  t=a.interpolation==='hold'?0:a.interpolation==='ease'?t*t*(3-2*t):t;
  return Math.round((a.value+(b.value-a.value)*t)*100)/100;
}
export function TimelinePanel({locale}:{locale:Locale}) {
  const t=labels[locale];
  const [state,setState]=useState<State>({frame:0,playing:false,preview:false,data:null});
  const [layerId,setLayerId]=useState('');
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
        const result=await invoke<State>('timeline',{request});
        if(alive.current)setState(old=>request.action==='tick'&&old.frame===result.frame&&old.playing===result.playing&&old.preview===result.preview?old:({...result,data:request.action==='tick'?(result.data??old.data):result.data}));
      }catch(e){if(alive.current){setError(String(e));if(request.action==='get')setState({frame:0,playing:false,preview:false,data:null});}}
      finally{if(!quiet&&alive.current)setBusy(false);}
    };
    const next=serial.current.then(run,run);serial.current=next;return next;
  },[]);
  useEffect(()=>{
    alive.current=true;void command({action:'get'});
    const events=['documents-changed'];
    let refresh:ReturnType<typeof setTimeout>;
    const listeners=events.map(event=>getCurrentWebviewWindow().listen(event,()=>{clearTimeout(refresh);refresh=setTimeout(()=>void command({action:'get'},true),30);}).catch(()=>()=>{}));
    return()=>{alive.current=false;clearTimeout(refresh);listeners.forEach(p=>void p.then(unlisten=>unlisten()).catch(()=>{}));void serial.current.then(()=>invoke('timeline',{request:{action:'stop'}})).catch(()=>{});};
  },[command]);
  useEffect(()=>{
    if(!state.playing)return;
    let canceled=false,timer:ReturnType<typeof setTimeout>;
    async function tick(){await command({action:'tick'},true);if(!canceled)timer=setTimeout(()=>void tick(),16);}
    void tick();return()=>{canceled=true;clearTimeout(timer);};
  },[state.playing,command]);
  useEffect(()=>{if(state.data){setFps(String(state.data.fps));setDuration(String(state.data.duration));setLayerId(id=>(state.data!.layers.some(l=>l.id===state.data!.selectedLayer)?state.data!.selectedLayer:undefined)??(state.data!.layers.some(l=>l.id===id)?id:state.data!.layers.at(-1)?.id??''));}},[state.data]);
  function selectLayer(id:string){setLayerId(id);if(id!==layerId)void command({action:'select',layer:id});}
  const layer=state.data?.layers.find(l=>l.id===layerId);
  useEffect(()=>{setValue(String(sample(layer,property,state.frame)));const key=layer?.channels[property]?.find(k=>k.frame===state.frame);if(key)setInterpolation(key.interpolation);},[layer,property,state.frame]);
  const data=state.data;
  const seek=(frame:number)=>{
    if(!Number.isFinite(frame))return;
    pendingSeek.current=Math.max(0,Math.min((data?.duration??1)-1,Math.round(frame)));
    if(seeking.current)return;
    seeking.current=true;
    void (async()=>{try{while(pendingSeek.current!==null&&alive.current){const next=pendingSeek.current;pendingSeek.current=null;await command({action:'seek',frame:next},true);}}finally{seeking.current=false;}})();
  };
  const timelineWidth=Math.max(500,(data?.duration??120)*zoom);
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
  return <div className="timeline-panel" onKeyDown={e=>e.stopPropagation()}>
    <div className="timeline-toolbar">
      <div className="timeline-transport">
        <button title={t.first} aria-label={t.first} disabled={!data||busy} onClick={()=>seek(0)}>⏮</button>
        <button title={t.previous} aria-label={t.previous} disabled={!data||busy} onClick={()=>seek(state.frame-1)}>‹</button>
        <button className="timeline-play" title={state.playing?t.pause:t.play} aria-label={state.playing?t.pause:t.play} disabled={!data||busy} onClick={()=>void command({action:state.playing?'pause':'play'})}>{state.playing?'Ⅱ':'▶'}</button>
        <button title={t.next} aria-label={t.next} disabled={!data||busy} onClick={()=>seek(state.frame+1)}>›</button>
        <button title={t.last} aria-label={t.last} disabled={!data||busy} onClick={()=>seek((data?.duration??1)-1)}>⏭</button>
      </div>
      <label>{t.frame}<input aria-label={t.frame} type="number" min={1} max={data?.duration??1} value={state.frame+1} disabled={!data||busy} onChange={e=>{if(e.target.value)seek(Number(e.target.value)-1);}}/></label>
      <span className="timeline-time">{(state.frame/(data?.fps??24)).toFixed(2)} s</span>
      <label>fps<input aria-label="fps" type="number" min={1} max={120} value={fps} onChange={e=>setFps(e.target.value)}/></label>
      <label>{t.duration}<input type="number" min={1} max={18000} value={duration} onChange={e=>setDuration(e.target.value)}/></label>
      <button disabled={!data||busy||state.playing} onClick={()=>void command({action:'settings',fps:Number(fps),duration:Number(duration),looping:data?.looping??true})}>{t.apply}</button>
      <label className="timeline-check"><input type="checkbox" checked={data?.looping??true} disabled={!data||busy} onChange={e=>void command({action:'settings',fps:data!.fps,duration:data!.duration,looping:e.target.checked})}/>{t.loop}</label>
      <label>{t.zoom}<select value={zoom} onChange={e=>setZoom(Number(e.target.value))}>{[4,8,16,24,32].map(n=><option key={n} value={n}>{n}</option>)}</select></label>
      <button disabled={!state.preview||busy} onClick={()=>void command({action:'stop'})}>{t.stop}</button>
      <span className="timeline-mode" data-preview={state.preview}>{busy?t.busy:state.preview?t.preview:t.drawing}</span>
    </div>
    {data&&<div className="timeline-edit-toolbar">{layer?.pixel?<div className="timeline-edit-tools" role="toolbar" aria-label={layer?.pixel?t.pixel:t.vector}><button className="tool-button" title={t.capture} aria-label={t.capture} disabled={disabled||state.preview} onClick={()=>void command({action:'capture',layer:layerId,frame:state.frame,blank:false})}><Icon name="animationRecord"/></button><button className="tool-button" title={t.blank} aria-label={t.blank} disabled={disabled} onClick={()=>void command({action:'capture',layer:layerId,frame:state.frame,blank:true})}><Icon name="document"/></button><button className="tool-button" title={t.duplicate} aria-label={t.duplicate} disabled={disabled||!layer.cels.some(f=>f<=state.frame)||state.frame>=data.duration-1} onClick={()=>void command({action:'duplicate',layer:layerId,from:state.frame,frame:state.frame+1})}><Icon name="animationDuplicate"/></button><button className="tool-button" title={t.edit} aria-label={t.edit} disabled={disabled||!layer.cels.some(f=>f<=state.frame)} onClick={()=>void command({action:'load',layer:layerId,frame:state.frame})}><Icon name="brush"/></button><button className="tool-button" title={t.remove} aria-label={t.remove} disabled={disabled||!celAt} onClick={()=>void command({action:'remove',layer:layerId,frame:state.frame,property:null})}><Icon name="animationDelete"/></button></div>:<div className="timeline-edit-tools" role="toolbar" aria-label={layer?.pixel?t.pixel:t.vector}><button className="tool-button" title={t.key} aria-label={t.key} disabled={disabled||value.trim()===''||!Number.isFinite(Number(value))} onClick={()=>void command({action:'key',layer:layerId,frame:state.frame,property,value:Number(value),interpolation})}><Icon name="animationKey"/></button><button className="tool-button" title={t.remove} aria-label={t.remove} disabled={disabled||!keyAt} onClick={()=>void command({action:'remove',layer:layerId,frame:state.frame,property})}><Icon name="animationDelete"/></button></div>}</div>}
    {data?<div className="timeline-body">
      <div className="timeline-tracks" aria-label={t.hint}>
        <div className="timeline-sheet" style={{width:180+timelineWidth}}>
          <div className="timeline-row timeline-ruler"><div className="timeline-track-name">{t.frame}</div><div className="timeline-lane" style={{width:timelineWidth}} onPointerDown={selectFrame} onPointerMove={e=>{if(e.buttons===1)selectFrame(e);}} onPointerUp={e=>{if(scrub.current!==null)seek(scrub.current);scrub.current=null;if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}}>
            {Array.from({length:Math.ceil(data.duration/step)},(_,i)=>i*step).map(frame=><span className="timeline-tick" key={frame} style={{left:frame*zoom}}>{frame+1}</span>)}
          </div></div>
          {data.layers.slice().reverse().map(item=><div key={item.id} className="timeline-layer" data-selected={layerId===item.id}>
            <div className="timeline-row"><button className="timeline-track-name" onClick={()=>selectLayer(item.id)}><span>{item.pixel?'▣':'◇'} {item.name}{item.locked?' 🔒':''}</span><small>{item.pixel?t.pixel:t.vector}</small></button><div className="timeline-lane" style={{width:timelineWidth}} onClick={e=>{selectLayer(item.id);seek(Math.floor((e.clientX-e.currentTarget.getBoundingClientRect().left)/zoom));}}>
              {item.pixel?item.cels.map((frame,index)=><button key={frame} className="timeline-cel" aria-label={`${item.name} · ${t.frame} ${frame+1}`} title={`${t.frame} ${frame+1}`} data-current={state.frame>=frame&&state.frame<(item.cels[index+1]??data.duration)} style={{left:frame*zoom,width:Math.max(4,((item.cels[index+1]??data.duration)-frame)*zoom-2)}} onClick={e=>{e.stopPropagation();selectLayer(item.id);seek(frame);}}>● <span>{frame+1}</span></button>):<span className="timeline-vector-bar"/>}
            </div></div>
            {!item.pixel&&layerId===item.id&&properties.map(p=><div className="timeline-row timeline-property-row" key={p}><button className="timeline-track-name" data-selected={property===p} onClick={()=>setProperty(p)}>{t[p]}<span>{sample(item,p,state.frame)}</span></button><div className="timeline-lane" style={{width:timelineWidth}} onClick={e=>{setProperty(p);seek(Math.floor((e.clientX-e.currentTarget.getBoundingClientRect().left)/zoom));}}>
              {(item.channels[p]??[]).map(key=><button key={key.frame} className="timeline-key" data-current={key.frame===state.frame&&property===p} title={`${t[p]} · ${key.frame+1} · ${key.value}`} aria-label={`${t[p]} · ${t.frame} ${key.frame+1}`} style={{left:key.frame*zoom}} onClick={e=>{e.stopPropagation();if(movedKey.current){movedKey.current=false;return;}setProperty(p);seek(key.frame);}} onPointerDown={e=>{if(disabled)return;e.stopPropagation();drag.current={x:e.clientX,frame:key.frame,layer:item.id,property:p,key};e.currentTarget.setPointerCapture(e.pointerId);}} onPointerUp={e=>{const d=drag.current;drag.current=null;if(!d||!d.property||!d.key)return;const frame=Math.max(0,Math.min(data.duration-1,d.frame+Math.round((e.clientX-d.x)/zoom)));if(frame!==d.frame){movedKey.current=true;void command({action:'moveKey',layer:d.layer,from:d.frame,frame,property:d.property});}if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}}>{key.interpolation==='hold'?'■':'◆'}</button>)}
            </div></div>)}
          </div>)}
          <div className="timeline-playhead" style={{left:180+state.frame*zoom}} aria-hidden="true"/>
        </div>
      </div>
      <aside className="timeline-inspector">
        <div className="timeline-inspector-title"><strong>{layer?.name??t.hint}</strong><span>{layer?.pixel?t.pixel:t.vector}</span></div>
        {layer?.locked?<p>{t.locked}</p>:layer?.pixel?<>
          <p>{t.pixelHint}</p>
        </>:layer?<>
          <label>{t.value}<select value={property} onChange={e=>setProperty(e.target.value as Property)}>{properties.map(p=><option key={p} value={p}>{t[p]}</option>)}</select><input aria-label={t[property]} type="number" step="0.1" value={value} disabled={disabled} onChange={e=>setValue(e.target.value)}/></label>
          <label>{t.interpolation}<select value={interpolation} onChange={e=>setInterpolation(e.target.value as Interpolation)}>{(['linear','hold','ease'] as const).map(i=><option key={i} value={i}>{t[i]}</option>)}</select></label>
          <p>{t.vectorHint}</p>
        </>:null}
      </aside>
    </div>:<p className="timeline-empty">{t.empty}</p>}
    {error&&<div className="timeline-error" role="alert">{error}</div>}
  </div>;
}
