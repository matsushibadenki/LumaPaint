import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { editorWindowTargets, newEditorWindow } from '../bridge';
import type { Locale } from '../i18n';
const labels = {
  ja:{title:'ドキュメントを移動',fresh:'新しいウインドウへ移動',view:'新しいウインドウで同じ文書を表示',loading:'ウインドウを確認中…'},
  en:{title:'Move document',fresh:'Move to a new window',view:'New view of this document',loading:'Loading windows…'},
  'zh-CN':{title:'移动文档',fresh:'移动到新窗口',view:'在新窗口中查看此文档',loading:'正在加载窗口…'},
};
export function DocumentTabMenu({locale,x,y,onClose,onMove,onView}:{locale:Locale;x:number;y:number;onClose:()=>void;onMove:(target:string)=>Promise<void>;onView:(target:string)=>Promise<void>}) {
  const [targets,setTargets]=useState<[string,string][]|null>(null);
  const [busy,setBusy]=useState(false);const [error,setError]=useState('');
  const root=useRef<HTMLDivElement>(null);const t=labels[locale];
  useEffect(()=>{let active=true;editorWindowTargets().then(items=>{if(active)setTargets(items);}).catch(cause=>{if(active)setError(String(cause));});return()=>{active=false;};},[]);
  useEffect(()=>{
    const outside=(event:PointerEvent)=>{if(!root.current?.contains(event.target as Node)&&!busy)onClose();};
    const escape=(event:KeyboardEvent)=>{if(event.key==='Escape'&&!busy)onClose();};
    document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);
    return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);};
  },[onClose,busy]);
  async function move(target?:string,view=false) {
    if(busy)return;setBusy(true);setError('');
    try {await (view?onView:onMove)(target??await newEditorWindow());onClose();}catch(cause){setError(String(cause));}finally{setBusy(false);}
  }
  return createPortal(<div ref={root} className="document-tab-menu" role="menu" aria-label={t.title} aria-busy={busy} style={{left:Math.max(16,Math.min(x,window.innerWidth-316)),top:Math.max(16,Math.min(y,window.innerHeight-320))}}>
    <strong>{t.title}</strong><button role="menuitem" disabled={busy} autoFocus onClick={()=>void move()}>{t.fresh}</button>
    <button role="menuitem" disabled={busy} onClick={()=>void move(undefined,true)}>{t.view}</button>
    {targets===null&&!error&&<span>{t.loading}</span>}{targets?.map(([label,title])=><button role="menuitem" key={label} title={title} disabled={busy} onClick={()=>void move(label)}>{title}</button>)}
    {error&&<span role="alert">{error}</span>}
  </div>,document.body);
}
