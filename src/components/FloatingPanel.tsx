import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { Locale } from '../i18n';
export type PanelPlacement = {x:number;y:number;width:number;height:number};
export const dockingLabels = {
  ja:{float:'パネルを切り離す',dock:'右側にドッキング',close:'閉じてドックへ戻す',reset:'パネル配置をリセット',hint:'見出しをドラッグして移動・ドッキング'},
  en:{float:'Float panel',dock:'Dock on the right',close:'Close and return to dock',reset:'Reset panel layout',hint:'Drag the header to move or dock'},
  'zh-CN':{float:'浮动面板',dock:'停靠到右侧',close:'关闭并返回停靠栏',reset:'重置面板布局',hint:'拖动标题移动或停靠'},
};
export function fitPlacement(p:PanelPlacement):PanelPlacement {
  const width=Math.max(240,Math.min(p.width,window.innerWidth-24));
  const height=Math.max(140,Math.min(p.height,window.innerHeight-100));
  return {width,height,x:Math.max(8,Math.min(p.x,window.innerWidth-width-8)),y:Math.max(84,Math.min(p.y,window.innerHeight-height-8))};
}
export function FloatingPanel({title,locale,placement,onMove,onDock,children}:{title:string;locale:Locale;placement:PanelPlacement;onMove:(p:PanelPlacement)=>void;onDock:()=>void;children:ReactNode}) {
  const ref=useRef<HTMLDivElement>(null);const drag=useRef<{x:number;y:number;p:PanelPlacement}|null>(null);
  const latest=useRef({placement,onMove});latest.current={placement,onMove};
  const [target,setTarget]=useState(false);const t=dockingLabels[locale];
  const overlapsDock=(x:number,y:number)=>{const r=document.querySelector('.inspector')?.getBoundingClientRect();return !!r&&x>=r.left-24&&x<=r.right&&y>=r.top&&y<=r.bottom;};
  useEffect(()=>{const update=()=>onMove(fitPlacement(placement));window.addEventListener('resize',update);return()=>window.removeEventListener('resize',update);},[placement,onMove]);
  useEffect(()=>{const node=ref.current;if(!node)return;const observer=new ResizeObserver(()=>{window.dispatchEvent(new Event('panel-layout-change'));const r=node.getBoundingClientRect();const p=latest.current.placement;if(Math.abs(r.width-p.width)>.5||Math.abs(r.height-p.height)>.5)latest.current.onMove(fitPlacement({x:r.x,y:r.y,width:r.width,height:r.height}));});observer.observe(node);return()=>observer.disconnect();},[]);
  useEffect(()=>{window.dispatchEvent(new Event('panel-layout-change'));return()=>{requestAnimationFrame(()=>window.dispatchEvent(new Event('panel-layout-change')));};},[placement]);
  return createPortal(<div ref={ref} className={`floating-panel${target?' docking':''}`} role="region" aria-label={title} data-floating-panel style={{left:placement.x,top:placement.y,width:placement.width,height:placement.height}} onPointerDown={()=>{if(ref.current)ref.current.style.zIndex=String(++topPanel);}} onPointerUp={e=>{if(e.target!==e.currentTarget)return;if(!drag.current&&ref.current){const r=ref.current.getBoundingClientRect();onMove(fitPlacement({x:r.x,y:r.y,width:r.width,height:r.height}));}}}>
    <header title={t.hint} onDoubleClick={onDock} onPointerDown={e=>{if((e.target as HTMLElement).closest('button')||e.button!==0)return;e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);drag.current={x:e.clientX,y:e.clientY,p:placement};}} onPointerMove={e=>{const d=drag.current;if(!d)return;onMove(fitPlacement({...d.p,x:d.p.x+e.clientX-d.x,y:d.p.y+e.clientY-d.y}));setTarget(overlapsDock(e.clientX,e.clientY));}} onPointerUp={e=>{if(!drag.current)return;e.stopPropagation();drag.current=null;setTarget(false);if(overlapsDock(e.clientX,e.clientY))onDock();}} onPointerCancel={()=>{drag.current=null;setTarget(false);}}>
      <strong>{title}</strong><button title={t.dock} aria-label={`${title} · ${t.dock}`} onClick={onDock}>⇥</button><button title={t.close} aria-label={`${title} · ${t.close}`} onClick={onDock}>×</button>
    </header><div className="floating-panel-content">{children}</div>
  </div>,document.body);
}
let topPanel=90;
