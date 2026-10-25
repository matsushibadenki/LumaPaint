import {useEffect,useRef,useState,type ReactNode,type PointerEvent as ReactPointerEvent} from 'react';
import {invoke,isTauri,convertFileSrc} from '@tauri-apps/api/core';
import type {DocumentTabSnapshot,DocumentWorkspaceSnapshot} from '../bridge';
import {useWorkspaceSession} from '../workspace-session';
import type {Locale,Theme} from '../i18n';
import {NativePaneGPU} from './NativePaneGPU';
import {arrangeLayout,dropDocument,emptyLayout,reconcileLayout,resizeLayout,type DocumentLayout,type DropEdge} from '../document-layout';
import './document-panes.css';
const words={
 ja:{layout:'ドキュメントの配置',single:'すべてをタブに統合',columns:'左右に',rows:'上下に',parts:'分割',grid:'格子状に配置',tabs:'ドキュメント',empty:'タブをここにドロップ',activate:'このドキュメントを編集',loading:'プレビューを準備中',retry:'プレビューを再読み込み',close:'閉じる',dirty:'未保存',resizeColumns:'左右の区画サイズを調整',resizeRows:'上下の区画サイズを調整'},
 en:{layout:'Arrange documents',single:'Consolidate all tabs',columns:'Columns:',rows:'Rows:',parts:'panes',grid:'Tile grid',tabs:'Documents',empty:'Drop a tab here',activate:'Edit this document',loading:'Preparing preview',retry:'Reload preview',close:'Close',dirty:'Unsaved',resizeColumns:'Resize adjacent columns',resizeRows:'Resize adjacent rows'},
 'zh-CN':{layout:'排列文档',single:'合并所有标签',columns:'左右',rows:'上下',parts:'分栏',grid:'网格排列',tabs:'文档',empty:'将标签拖到此处',activate:'编辑此文档',loading:'正在准备预览',retry:'重新加载预览',close:'关闭',dirty:'未保存',resizeColumns:'调整左右窗格大小',resizeRows:'调整上下窗格大小'},
};
function PanePreview({id,epoch,locale,onActivate}:{id:number;epoch:number;locale:Locale;onActivate:()=>void}){
 const [view,setView]=useState<{zoom:number;panX:number;panY:number;width:number;height:number}|null>(null);
 const [url,setUrl]=useState(''),[failed,setFailed]=useState(false),[retry,setRetry]=useState(0);
 useEffect(()=>{let live=true;setFailed(false);if(isTauri())void invoke<{url:string;view:typeof view}>('document_pane_preview',{id}).then(value=>{if(live){setView(value.view);const token=value.url.split('/').at(-1)!;setUrl(convertFileSrc(token,'document-preview'));}}).catch(()=>{if(live)setFailed(true);});return()=>{live=false;};},[id,epoch,retry]);
 const t=words[locale];
 return <div className="document-pane-preview"><button type="button" className="document-pane-activate" onClick={onActivate} aria-label={t.activate}>
 {url?<span className={view?"document-pane-view-frame":"document-pane-fit-frame"}><img src={url} alt="" draggable={false} style={view?{position:"absolute",width:view.width*view.zoom,height:view.height*view.zoom,left:`calc(50% + ${view.panX-view.width*view.zoom/2}px)`,top:`calc(50% + ${view.panY-view.height*view.zoom/2}px)`}:undefined} onError={()=>setFailed(true)}/></span>:<span className="document-pane-placeholder">{failed?'!':t.loading}</span>}
 </button>{failed&&<button type="button" className="document-pane-retry" onClick={()=>setRetry(n=>n+1)}>{t.retry}</button>}</div>;
}
export function DocumentPanes({documents,activeId,locale,disabled,onSelect,onClose,onContextMenu,onDragging,onWorkspace,theme,visible,children,dimensions}: {
 documents:DocumentTabSnapshot[];activeId:number|null;locale:Locale;theme:Theme;visible:boolean;disabled:boolean;onSelect:(id:number)=>void;onClose:(id:number)=>void;
 onContextMenu:(id:number,x:number,y:number)=>void;onDragging:(value:boolean)=>void;onWorkspace:(workspace:DocumentWorkspaceSnapshot)=>void;children:ReactNode;dimensions:ReactNode;
}){
 const [resizing,setResizing]=useState(false);
 const [layout,setLayout]=useState(emptyLayout),[menu,setMenu]=useState(false),[dragId,setDragId]=useState<number|null>(null),[hover,setHover]=useState<{index:number;edge:DropEdge}|null>(null);
 const root=useRef<HTMLDivElement>(null),drag=useRef<{id:number;x:number;y:number;pointer:number;moved:boolean;button:HTMLButtonElement}|null>(null),suppressClick=useRef(false);
 const grid=useRef<HTMLDivElement>(null),resize=useRef<{axis:'columns'|'rows';index:number;start:number;length:number;layout:DocumentLayout;button:HTMLElement;pointer:number;moved:boolean}|null>(null),resizeFrame=useRef(0),resizePending=useRef<DocumentLayout|null>(null);
 const t=words[locale],idKey=documents.map(d=>d.id).join(',');
 const current=reconcileLayout(layout,documents.map(d=>d.id),activeId);
 const sessionWarning=useWorkspaceSession(current,setLayout,onWorkspace,dragId!==null||resizing);
 useEffect(()=>{setLayout(p=>reconcileLayout(p,idKey? idKey.split(',').map(Number):[],activeId));},[idKey,activeId]);
 useEffect(()=>{const stop=()=>{setResizing(false);const sizing=resize.current;resize.current=null;if(sizing){cancelAnimationFrame(resizeFrame.current);resizeFrame.current=0;setLayout(sizing.layout);if(sizing.button.hasPointerCapture(sizing.pointer))sizing.button.releasePointerCapture(sizing.pointer);}const state=drag.current;drag.current=null;if(state?.button.hasPointerCapture(state.pointer))state.button.releasePointerCapture(state.pointer);setDragId(null);setHover(null);onDragging(false);};const cancel=(event:KeyboardEvent)=>{if(event.key==='Escape')stop();};window.addEventListener('keydown',cancel);window.addEventListener('dragend',stop);window.addEventListener('blur',stop);return()=>{window.removeEventListener('keydown',cancel);window.removeEventListener('dragend',stop);window.removeEventListener('blur',stop);};},[onDragging]);
 useEffect(()=>{if(!menu)return;const stop=(event:PointerEvent)=>{if(!(event.target as Element).closest('.document-arrange'))setMenu(false);};const key=(event:KeyboardEvent)=>{if(event.key==='Escape')setMenu(false);};document.addEventListener('pointerdown',stop);document.addEventListener('keydown',key);return()=>{document.removeEventListener('pointerdown',stop);document.removeEventListener('keydown',key);};},[menu]);
 useEffect(()=>{window.dispatchEvent(new Event('panel-layout-change'));},[menu]);
 const arrange=(rows:number,columns:number)=>{setLayout(arrangeLayout(current,rows,columns));setMenu(false);};
 function targetAt(x:number,y:number){
  const node=document.elementFromPoint(x,y)?.closest<HTMLElement>('[data-pane-index]');
  if(!node||!root.current?.contains(node))return null;
  const index=Number(node.dataset.paneIndex),r=node.getBoundingClientRect();
  const distances=[(x-r.left)/r.width,(r.right-x)/r.width,(y-r.top)/r.height,(r.bottom-y)/r.height],nearest=Math.min(...distances);
  let edge:DropEdge=nearest<.23?(['left','right','top','bottom'] as const)[distances.indexOf(nearest)]:'center';
  if(((edge==='left'||edge==='right')&&current.columns===3)||((edge==='top'||edge==='bottom')&&current.rows===3))edge='center';
  return {index,edge};
 }
 function moveTab(event:ReactPointerEvent<HTMLButtonElement>){
  const state=drag.current;if(!state||state.pointer!==event.pointerId)return;
  if(!state.moved&&Math.hypot(event.clientX-state.x,event.clientY-state.y)>6){state.moved=true;suppressClick.current=true;setMenu(false);setDragId(state.id);onDragging(true);}
  if(state.moved){event.preventDefault();setHover(targetAt(event.clientX,event.clientY));}
 }
 function endTab(event:ReactPointerEvent<HTMLButtonElement>,commit:boolean){
  const state=drag.current;if(!state||state.pointer!==event.pointerId)return;
  drag.current=null;
  if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);
  const target=state.moved&&commit?targetAt(event.clientX,event.clientY):null;
  if(target&&!disabled&&current.panes.some(p=>p.tabs.includes(state.id))){setLayout(dropDocument(current,state.id,target.index,target.edge));onSelect(state.id);}
  setDragId(null);setHover(null);onDragging(false);
 }
 function resizeAt(event:ReactPointerEvent<HTMLElement>){
  const sizing=resize.current;if(!sizing||sizing.pointer!==event.pointerId)return null;
  const coordinate=sizing.axis==='columns'?event.clientX:event.clientY;
  return resizeLayout(sizing.layout,sizing.axis,sizing.index,(coordinate-sizing.start)/sizing.length,(sizing.axis==='columns'?120:110)/sizing.length);
 }
 function finishResize(event:ReactPointerEvent<HTMLElement>,commit:boolean){
  const sizing=resize.current;if(!sizing||sizing.pointer!==event.pointerId)return;
  const next=commit?resizeAt(event):sizing.layout;resize.current=null;
  cancelAnimationFrame(resizeFrame.current);resizeFrame.current=0;resizePending.current=null;
  if(next)setLayout(next);setResizing(false);if(sizing.button.hasPointerCapture(sizing.pointer))sizing.button.releasePointerCapture(sizing.pointer);onDragging(false);
 }
 function separator(axis:'columns'|'rows',index:number){
  const sizes=axis==='columns'?current.columnSizes:current.rowSizes;
  const position=sizes.slice(0,index+1).reduce((a,b)=>a+b,0);

  const dimensions=()=>{const node=grid.current;return Math.max(1,(axis==='columns'?node?.scrollWidth:node?.scrollHeight)??1)-3*(sizes.length-1);};
  return <div key={`${axis}-${index}`} className="document-pane-divider" data-axis={axis} data-floating-panel role="separator" tabIndex={disabled?-1:0} aria-label={axis==='columns'?t.resizeColumns:t.resizeRows} aria-orientation={axis==='columns'?'vertical':'horizontal'} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(position*100)} style={axis==='columns'?{gridColumn:index*2+2,gridRow:'1 / -1'}:{gridRow:index*2+2,gridColumn:'1 / -1'}}
   onPointerDown={event=>{if(disabled||event.button!==0)return;event.preventDefault();event.currentTarget.focus();resize.current={axis,index,start:axis==='columns'?event.clientX:event.clientY,length:Math.max(1,dimensions()),layout:current,button:event.currentTarget,pointer:event.pointerId,moved:false};event.currentTarget.setPointerCapture(event.pointerId);}}
   onPointerMove={event=>{const next=resizeAt(event);if(!next||!resize.current)return;if(!resize.current.moved){const coordinate=resize.current.axis==='columns'?event.clientX:event.clientY;if(Math.abs(coordinate-resize.current.start)<1)return;resize.current.moved=true;setResizing(true);onDragging(true);}resizePending.current=next;if(!resizeFrame.current)resizeFrame.current=requestAnimationFrame(()=>{resizeFrame.current=0;if(resizePending.current)setLayout(resizePending.current);});}}
   onPointerUp={event=>finishResize(event,true)} onPointerCancel={event=>finishResize(event,false)} onLostPointerCapture={event=>{if(resize.current)finishResize(event,false);}}
   onDoubleClick={()=>{if(!disabled)setLayout(resizeLayout(current,axis,index,(sizes[index+1]-sizes[index])/2,0));}}
   onKeyDown={event=>{if(disabled)return;const delta=event.key===(axis==='columns'?'ArrowLeft':'ArrowUp')?-.02:event.key===(axis==='columns'?'ArrowRight':'ArrowDown')?.02:event.key==='Home'?-1:event.key==='End'?1:null;if(delta===null)return;event.preventDefault();setLayout(resizeLayout(current,axis,index,delta,(axis==='columns'?120:110)/Math.max(1,dimensions())));}}/>;
 }
 return <div ref={root} className="document-panes" data-dragging={dragId!==null}>
 <div className="document-arrange-bar"><div className="document-arrange"><button type="button" aria-label={t.layout} title={t.layout} aria-expanded={menu} onClick={()=>setMenu(v=>!v)} disabled={disabled||!documents.length}><svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="1.5" width="13" height="13" rx="1"/><path d="M8 2v12M8 8h6"/></svg></button>
 {menu&&<div className="document-arrange-menu" data-floating-panel role="menu">{([[1,1],[1,2],[1,3],[2,1],[3,1],[2,2],[3,3]] as const).map(([rows,columns])=><button type="button" role="menuitem" key={`${rows}:${columns}`} onClick={()=>arrange(rows,columns)}><span className="document-layout-symbol" style={{gridTemplateColumns:`repeat(${columns},1fr)`,gridTemplateRows:`repeat(${rows},1fr)`}}>{Array.from({length:rows*columns},(_,i)=><i key={i}/>)}</span>{rows===1&&columns===1?t.single:rows===1?`${t.columns} ${columns} ${t.parts}`:columns===1?`${t.rows} ${rows} ${t.parts}`:`${t.grid} ${rows}×${columns}`}</button>)}</div>}</div>{sessionWarning&&<span className="document-session-warning" title={{ja:"一部の文書または配置設定を復元・保存できませんでした。未保存文書は復旧メニューで確認してください。",en:"Some documents or workspace settings could not be restored or saved. Use recovery for unsaved documents.","zh-CN":"部分文档或窗格配置无法恢复或保存。未保存的文档请使用恢复功能。"}[locale]}>!</span>}{dimensions}</div>
 <div ref={grid} className="document-pane-grid" style={{gridTemplateColumns:current.columnSizes.map(s=>`minmax(120px,${s}fr)`).join(' 3px '),gridTemplateRows:current.rowSizes.map(s=>`minmax(110px,${s}fr)`).join(' 3px ')}}>
 {current.panes.map((pane,index)=>{const active=activeId!==null&&pane.tabs.includes(activeId);const selected=active?activeId:pane.selected;
 return <section key={index} className="document-pane" data-active={active} data-pane-document={selected??''} data-pane-index={index} style={{gridColumn:(index%current.columns)*2+1,gridRow:Math.floor(index/current.columns)*2+1}}>
 <div className="document-tab-list" role="tablist" aria-label={`${t.tabs} ${index+1}`}>
 {pane.tabs.map(id=>{const doc=documents.find(d=>d.id===id);if(!doc)return null;return <div key={id} className="document-tab-shell" aria-current={id===selected} data-document-id={id}>
 <button type="button" role="tab" aria-selected={id===selected} className="document-tab" disabled={disabled} draggable={false} onDragStart={event=>event.preventDefault()} onPointerDown={event=>{if(event.button!==0||disabled)return;suppressClick.current=false;drag.current={id,x:event.clientX,y:event.clientY,pointer:event.pointerId,moved:false,button:event.currentTarget};event.currentTarget.setPointerCapture(event.pointerId);}} onPointerMove={moveTab} onPointerUp={event=>endTab(event,true)} onPointerCancel={event=>endTab(event,false)} onLostPointerCapture={event=>{if(drag.current)endTab(event,false);}} onClick={event=>{if(suppressClick.current){event.preventDefault();return;}onSelect(id);}} onContextMenu={event=>{event.preventDefault();onContextMenu(id,event.clientX,event.clientY);}}><span>{doc.fileName??`${t.tabs} ${documents.indexOf(doc)+1}`}</span>{doc.format==='tiled'&&<span className="document-format">T</span>}{doc.dirty&&<span className="unsaved-dot" aria-label={t.dirty}/>}</button>
 <button type="button" className="document-close" disabled={disabled} aria-label={`${doc.fileName??t.tabs} · ${t.close}`} onClick={()=>onClose(id)}>×</button></div>;})}
 </div><div className="document-pane-body">{active?<>{children}{resizing&&activeId!==null&&<div className="document-pane-resize-preview" inert><PanePreview id={activeId} epoch={activeId} locale={locale} onActivate={()=>{}}/></div>}</>:selected!==null?<NativePaneGPU key={selected} id={selected} epoch={activeId??0} locale={locale} theme={theme} visible={visible} paused={dragId!==null||resizing} onActivate={()=>onSelect(selected)} fallback={<PanePreview id={selected} epoch={activeId??0} locale={locale} onActivate={()=>onSelect(selected)}/>}/>:<span className="document-pane-empty">{t.empty}</span>}</div>
 {hover?.index===index&&dragId!==null&&<div className="document-drop-indicator" data-edge={hover.edge}/>}
 </section>;})}
 {current.columnSizes.slice(0,-1).map((_,index)=>separator('columns',index))}{current.rowSizes.slice(0,-1).map((_,index)=>separator('rows',index))}</div></div>;
}
