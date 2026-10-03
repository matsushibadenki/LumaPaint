import { useEffect, useRef, useState } from 'react';
import type { Locale } from '../i18n';
import { swatchLibrary, type Brush } from '../bridge';
import { swatchSeeds } from './swatchSeeds';
import './tone-studio.css';
export const toneStudioLabels={ja:{title:'カラートーンクリエイター',close:'閉じる',limit:'スウォッチが満杯です。5件分の空きを確保してください。'},en:{title:'Color Tone Creator',close:'Close',limit:'Swatches are full. Make room for 5 colors.'},'zh-CN':{title:'色调创建器',close:'关闭',limit:'色板已满，请预留5个颜色的位置。'}};
export function ToneStudio({locale,open,onClose}:{locale:Locale;open:boolean;onClose:()=>void}) {
  const frame=useRef<HTMLIFrameElement>(null);const pending=useRef(false);const [top,setTop]=useState(44);
  useEffect(()=>{const header=window.document.querySelector('.application-bar');if(!header)return;const update=()=>setTop(header.getBoundingClientRect().bottom);const observer=new ResizeObserver(update);observer.observe(header);update();window.addEventListener('resize',update);return()=>{observer.disconnect();window.removeEventListener('resize',update);};},[]);
  useEffect(()=>{
    const receive=async(event:MessageEvent)=>{
      if(event.source!==frame.current?.contentWindow||!open)return;
      const data=event.data;
      if(data?.type==='lumapaint-tone-close'){if(!pending.current)onClose();return;}
      if(data?.type!=='lumapaint-tone-export'||pending.current||!Number.isSafeInteger(data.requestId)||!Array.isArray(data.colors)||data.colors.length!==5||!data.colors.every((c:unknown)=>typeof c==='string'&&/^#[0-9a-f]{6}$/i.test(c)))return;
      const source=event.source as Window;const requestId=data.requestId;pending.current=true;
      try {
        const palette=data.colors as string[];
        const items=await swatchLibrary('get',undefined,undefined,swatchSeeds());
        if(items.length>507)throw new Error(toneStudioLabels[locale].limit);
        const tone=typeof data.tone==='string'?Array.from(data.tone).slice(0,40).join(''):'Tone';
        await swatchLibrary('addMany',undefined,undefined,undefined,palette.map((hex,i)=>({name:`${tone} ${i+1} · ${hex.toUpperCase()}`,paint:{kind:'color',color:hex.slice(1).match(/../g)!.map(c=>parseInt(c,16)) as Brush['color']}})));
        source.postMessage({type:'lumapaint-tone-result',requestId,ok:true},'*');
      }catch(error){source.postMessage({type:'lumapaint-tone-result',requestId,ok:false,error:String(error)},'*');}
      finally{pending.current=false;}
    };
    const key=(event:KeyboardEvent)=>{if(open&&event.key==='Escape'&&!pending.current){event.preventDefault();onClose();}};
    window.addEventListener('message',receive);window.addEventListener('keydown',key);return()=>{window.removeEventListener('message',receive);window.removeEventListener('keydown',key);};
  },[locale,open,onClose]);
  return <section id="tone-studio-overlay" className="tone-studio-overlay" hidden={!open} style={{top}} role="dialog" aria-label={toneStudioLabels[locale].title}>
    <iframe ref={frame} src={`/tone-studio/index.html?locale=${encodeURIComponent(locale)}`} title={toneStudioLabels[locale].title} sandbox="allow-scripts allow-same-origin" />
  </section>;
}
