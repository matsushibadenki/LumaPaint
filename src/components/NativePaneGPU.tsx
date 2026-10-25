import {useEffect,useRef,useState,type ReactNode} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {getCurrentWebviewWindow} from '@tauri-apps/api/webviewWindow';
import type {Locale,Theme} from '../i18n';
let token=0;
export function NativePaneGPU({id,epoch,locale,theme,visible,paused,onActivate,fallback}:{id:number;epoch:number;locale:Locale;theme:Theme;visible:boolean;paused:boolean;onActivate:()=>void;fallback:ReactNode}){
 const slot=useRef<HTMLDivElement>(null),[failed,setFailed]=useState(false),[ready,setReady]=useState(false);
 useEffect(()=>{
  if(!isTauri()){setFailed(true);return;}
  const element=slot.current;if(!element)return;
  let live=true,stopped=false,busy=false,dirty=false,frame=0,retry=0,failures=0;const nextGeneration=()=>token=Math.max(token+1,Date.now()*1024);
  setFailed(false);setReady(false);
  const hidden={x:0,y:0,width:0,height:0,zoom:1,dark:false,visible:false};
  const media=matchMedia('(prefers-color-scheme: dark)');
  const hide=()=>invoke('sync_document_pane_gpu',{id,generation:nextGeneration(),request:hidden}).catch(()=>{});
  const run=async()=>{
   frame=0;if(!live||stopped)return;if(busy){dirty=true;return;}busy=true;dirty=false;
   const r=element.getBoundingClientRect();
   try{
    const presented=await invoke<boolean>('sync_document_pane_gpu',{id,generation:nextGeneration(),request:{x:r.x,y:r.y,width:r.width,height:r.height,zoom:1,dark:theme==='dark'||theme==='system'&&media.matches,visible:visible&&!paused&&!document.hidden&&r.width>0&&r.height>0,overlays:Array.from(document.querySelectorAll('[data-floating-panel]')).slice(0,10).map(node=>{const b=node.getBoundingClientRect();return [b.left-r.left,b.top-r.top,b.right-r.left,b.bottom-r.top];})}});
    if(live){setReady(presented);if(!presented){if(++failures>8)throw Error('GPU surface unavailable');retry=window.setTimeout(schedule,Math.min(1000,16*2**failures));}else failures=0;}
   }catch{if(live){stopped=true;setFailed(true);void hide();}}
   finally{busy=false;if(live&&dirty)schedule();}
  };
  function schedule(){if(live&&!stopped&&!frame)frame=requestAnimationFrame(()=>{void run();});}
  const resize=new ResizeObserver(schedule);resize.observe(element);
  const scroll=(event:Event)=>{if(event.target instanceof Element&&event.target.contains(element))schedule();};
  window.addEventListener('resize',schedule);window.addEventListener('scroll',scroll,true);window.addEventListener('panel-layout-change',schedule);document.addEventListener('visibilitychange',schedule);media.addEventListener('change',schedule);
  let stop=()=>{};void getCurrentWebviewWindow().listen('documents-changed',schedule).then(fn=>{if(live)stop=fn;else fn();});schedule();
  return()=>{live=false;resize.disconnect();stop();cancelAnimationFrame(frame);clearTimeout(retry);window.removeEventListener('resize',schedule);window.removeEventListener('scroll',scroll,true);window.removeEventListener('panel-layout-change',schedule);document.removeEventListener('visibilitychange',schedule);media.removeEventListener('change',schedule);void hide();};
 },[id,epoch,theme,visible,paused]);
 const label={ja:'このドキュメントを編集（GPU表示）',en:'Edit this document (GPU view)','zh-CN':'编辑此文档（GPU视图）'}[locale];
 return <div className="document-native-pane"><button type="button" className="document-native-activate" onClick={onActivate} aria-label={label}><div ref={slot} className="document-native-slot" data-gpu-ready={ready&&!failed}/></button>{(failed||paused)&&visible?<div className="document-native-fallback">{fallback}</div>:!ready&&visible?<span className="document-pane-placeholder">GPU…</span>:null}{failed&&<span className="document-gpu-fallback-indicator" title={{ja:'GPU描画を利用できないためプレビューを表示しています',en:'Showing a preview because GPU rendering is unavailable','zh-CN':'GPU渲染不可用，正在显示预览'}[locale]}>!</span>}</div>;
}
