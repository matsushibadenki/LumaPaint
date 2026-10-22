import {useEffect,useLayoutEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import type {Locale} from '../i18n';
export const layerContextLabels={ja:{title:'レイヤー',rasterize:'ラスタライズ',hint:'ドキュメントの解像度でピクセルレイヤーに変換します。Undoで元に戻せます。'},en:{title:'Layer',rasterize:'Rasterize',hint:'Convert to a pixel layer at document resolution. Undo restores the vector layer.'},'zh-CN':{title:'图层',rasterize:'栅格化',hint:'以文档分辨率转换为像素图层。撤销可恢复矢量图层。'}};
export function LayerContextMenu({locale,x,y,enabled,onRasterize,onClose}:{locale:Locale;x:number;y:number;enabled:boolean;onRasterize:()=>Promise<void>;onClose:()=>void}){
 const t=layerContextLabels[locale],root=useRef<HTMLDivElement>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[position,setPosition]=useState({left:x,top:y});
 useLayoutEffect(()=>{const b=root.current?.getBoundingClientRect();if(b)setPosition({left:Math.max(8,Math.min(x,innerWidth-b.width-8)),top:Math.max(8,Math.min(y,innerHeight-b.height-8))})},[x,y,error]);
 useEffect(()=>{const outside=(e:PointerEvent)=>{if(!busy&&!root.current?.contains(e.target as Node))onClose()};const key=(e:KeyboardEvent)=>{if(e.key==='Escape'&&!busy){e.preventDefault();e.stopPropagation();onClose()}if(['ArrowDown','ArrowUp','Home','End'].includes(e.key)){e.preventDefault();root.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()}};document.addEventListener('pointerdown',outside);document.addEventListener('keydown',key);return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',key)}},[busy,onClose]);
 const run=async()=>{if(busy||!enabled)return;setBusy(true);setError('');try{await onRasterize();onClose()}catch(e){setError(String(e))}finally{setBusy(false)}};
 return createPortal(<div ref={root} className="layer-context-menu" data-floating-panel="menu" role="menu" aria-label={t.title} aria-busy={busy} style={position} onContextMenu={e=>e.preventDefault()}><button type="button" role="menuitem" autoFocus disabled={!enabled||busy} title={t.hint} onClick={()=>void run()}>{t.rasterize}</button>{error&&<p role="alert">{error}</p>}</div>,document.body)
}
