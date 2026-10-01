import { useEffect, useRef, useState } from 'react';
import { applyGradient, type Gradient, type DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import './gradient-panel.css';
export const gradientLabels = {
  ja: { title: 'グラデーション', dither:'ディザ', presets:'プリセット', target:'適用先', fill:'塗り', stroke:'線', pixels:'ピクセル', type:'種類', linear:'線形', radial:'円形', angle:'角度', aspect:'縦横比', method:'方式', classic:'クラシック', perceptual:'知覚的', light:'リニア', opacity:'不透明度', position:'位置', midpoint:'中間点', color:'カラー', reverse:'反転', remove:'停止点を削除', apply:'適用', hint:'バーをクリックして色を追加。停止点・◇をドラッグして調整。', pixelHint:'適用すると選択範囲に描画します。選択範囲がない場合はレイヤー全体に描画します。', select:'表示中のロックされていないパス、またはピクセルレイヤーを選択してください。' },
  en: { title:'Gradient', dither:'Dither', presets:'Presets', target:'Target', fill:'Fill', stroke:'Stroke', pixels:'Pixels', type:'Type', linear:'Linear', radial:'Radial', angle:'Angle', aspect:'Aspect ratio', method:'Method', classic:'Classic', perceptual:'Perceptual', light:'Linear', opacity:'Opacity', position:'Location', midpoint:'Midpoint', color:'Color', reverse:'Reverse', remove:'Delete stop', apply:'Apply', hint:'Click the ramp to add colors. Drag stops and diamonds to adjust.', pixelHint:'Apply paints within the selection, or the entire layer when no selection exists.', select:'Select visible unlocked paths or a pixel layer.' },
  'zh-CN':{ title:'渐变', dither:'仿色', presets:'预设', target:'应用于', fill:'填色', stroke:'描边', pixels:'像素', type:'类型', linear:'线性', radial:'径向', angle:'角度', aspect:'长宽比', method:'方法', classic:'经典', perceptual:'感知', light:'线性', opacity:'不透明度', position:'位置', midpoint:'中点', color:'颜色', reverse:'反转', remove:'删除色标', apply:'应用', hint:'点击渐变条添加颜色。拖动色标和菱形进行调整。', pixelHint:'应用于选区；没有选区时绘制整个图层。', select:'请选择未锁定的可见路径或像素图层。' },
};
const presets: Gradient[] = [
  [[255,220,0,255],[242,66,24,255]], [[255,255,255,255],[0,0,0,255]], [[255,210,0,255],[255,54,38,255]], [[52,160,220,255],[255,255,255,0]], [[255,255,255,255],[0,0,0,255]],
].map((colors,i) => ({kind:i===4?'radial':'linear', angle:0, aspect:1, dither:false, method:'classic', stops:colors.map((color,j)=>({position:j,color:color as [number,number,number,number],midpoint:.5}))}));
function sample(g:Gradient,p:number):[number,number,number,number] {
  if(p<g.stops[0].position)return g.stops[0].color;
  const i=g.stops.findIndex((s,j)=>j>0&&p<s.position);if(i<0)return g.stops[g.stops.length-1].color;
  const a=g.stops[i-1],b=g.stops[i];const q=Math.max(0,Math.min(1,(p-a.position)/Math.max(.000001,b.position-a.position)));const t=q<a.midpoint?.5*q/a.midpoint:.5+.5*(q-a.midpoint)/(1-a.midpoint);
  if(g.method==='perceptual') {
    const mult=(v:number[],m:number[][])=>m.map(r=>r.reduce((sum,n,j)=>sum+n*v[j],0));
    const lab=(c:number[])=>mult(mult(c.slice(0,3).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}),[[.4122214708,.5363325363,.0514459929],[.2119034982,.6806995451,.1073969566],[.0883024619,.2817188376,.6299787005]]).map(Math.cbrt),[[.2104542553,.793617785,-.0040720468],[1.9779984951,-2.428592205,.4505937099],[.0259040371,.7827717662,-.808675766]]);
    const aa=lab(a.color),bb=lab(b.color);const lms=mult(aa.map((v,j)=>v*(1-t)+bb[j]*t),[[1,.3963377774,.2158037573],[1,-.1055613458,-.0638541728],[1,-.0894841775,-1.291485548]]).map(v=>v*v*v);
    const rgb=mult(lms,[[4.0767416621,-3.3077115913,.2309699292],[-1.2684380046,2.6097574011,-.3413193965],[-.0041960863,-.7034186147,1.707614701]]).map(v=>Math.max(0,Math.min(255,Math.round((v<=.0031308?v*12.92:1.055*v**(1/2.4)-.055)*255))));
    return [...rgb,Math.round(a.color[3]*(1-t)+b.color[3]*t)] as [number,number,number,number];
  }
  return a.color.map((c,j)=>{if(j===3||g.method==='classic')return Math.round(c*(1-t)+b.color[j]*t);const decode=(v:number)=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4;const v=decode(c/255)*(1-t)+decode(b.color[j]/255)*t;return Math.round((v<=.0031308?v*12.92:1.055*v**(1/2.4)-.055)*255);}) as [number,number,number,number];
}
const css = (g: Gradient) => {
  const stops=g.stops.flatMap((s,i)=>Array.from({length:i===g.stops.length-1?1:16},(_,j)=>{const end=g.stops[i+1]?.position??s.position;const mid=s.position+(end-s.position)*s.midpoint;const p=j<8?s.position+(mid-s.position)*j/8:mid+(end-mid)*(j-8)/8;const c=j===0?s.color:sample(g,p);return `rgba(${c[0]},${c[1]},${c[2]},${c[3]/255}) ${p*100}%`;})).join(',');
  return `${g.kind==='radial'?`radial-gradient(ellipse 50% ${50*g.aspect}% at center,`:`linear-gradient(${90-g.angle}deg,`} ${stops})`;
};
function NumberField({label, value, min, max, onDraft, onCommit}:{label:string;value:number;min:number;max:number;onDraft:(v:number)=>void;onCommit:()=>void}) {
  const [draft,setDraft]=useState(String(value)); const focused=useRef(false); const original=useRef(value); useEffect(()=>{if(!focused.current)setDraft(String(Number(value.toFixed(3))));},[value]);
  return <label className="gradient-field"><span>{label}</span><input type="number" value={draft} min={min} max={max} step="1" onFocus={()=>{focused.current=true;original.current=value;}} onChange={e=>{setDraft(e.target.value);const n=Number(e.target.value);if(e.target.value!==''&&Number.isFinite(n)&&n>=min&&n<=max)onDraft(n);}} onBlur={()=>{focused.current=false;setDraft(String(Number(value.toFixed(3))));onCommit();}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){onDraft(original.current);setDraft(String(original.current));e.currentTarget.blur();}}}/></label>;
}
export function GradientPanel({locale,document,enabled,onUpdate}:{locale:Locale;document:DocumentSnapshot;enabled:boolean;onUpdate:(d:DocumentSnapshot)=>void}) {
  const t=gradientLabels[locale];const [gradient,setGradient]=useState<Gradient>(()=>structuredClone(presets[0]));const current=useRef(gradient);current.current=gradient;
  const [stop,setStop]=useState(0);const [target,setTarget]=useState<'fill'|'stroke'>('fill');const [busy,setBusy]=useState(false);const [error,setError]=useState('');const ramp=useRef<HTMLDivElement>(null);
  const selected=document.layers.flatMap(l=>l.objects.filter(o=>document.selectedVectorObjects.includes(o.id)).map(o=>({l,o})));
  const layer=document.layers.find(l=>l.id===document.layerId);const pixels=selected.length===0&&layer?.kind==='paint';
  const editable=enabled&&!document.activeSavedPath&&!busy&&(pixels?!!layer?.visible&&!layer.locked&&!layer.alphaLocked:selected.length>0&&selected.every(({l,o})=>l.kind==='vector'&&l.visible&&!l.locked&&o.visible&&!o.locked&&o.kind!=='text'));
  const stored=target==='fill'?selected[0]?.o.fillGradient:selected[0]?.o.strokeGradient;
  const storedKey=JSON.stringify(stored);const selectionKey=document.selectedVectorObjects.join(',');
  useEffect(()=>{if(storedKey&&storedKey!=='null'){const g=JSON.parse(storedKey) as Gradient;setGradient(g);current.current=g;setStop(i=>Math.min(i,g.stops.length-1));}setError('');},[storedKey,selectionKey,target]);
  useEffect(()=>setStop(0),[selectionKey,target]);
  const update=(g:Gradient)=>{current.current=g;setGradient(g);};
  async function apply(g=current.current) {if(!editable)return;setBusy(true);setError('');try {onUpdate(await applyGradient(document.selectedVectorObjects,pixels?'pixels':target,target==='stroke'&&!pixels?{...g,dither:false}:g));}catch(e){setError(String(e));}finally{setBusy(false);}}
  const commit=()=>{if(!pixels)void apply();};
  const change=(patch:Partial<Gradient>, immediate=false)=>{const g={...current.current,...patch};update(g);if(immediate&&!pixels)void apply(g);};
  const changeStop=(patch:Partial<Gradient['stops'][number]>)=>{update({...current.current,stops:current.current.stops.map((s,i)=>i===stop?{...s,...patch}:s)});};
  const position=(clientX:number)=>{const rect=ramp.current!.getBoundingClientRect();return Math.max(0,Math.min(1,(clientX-rect.left)/rect.width));};
  const drag=(e:React.PointerEvent<HTMLButtonElement>,index:number,midpoint=false)=>{
    if(!editable)return;e.preventDefault();e.stopPropagation();e.currentTarget.setPointerCapture(e.pointerId);if(!midpoint)setStop(index);
    const el=e.currentTarget;
    const move=(event:PointerEvent)=>{const g=current.current;const p=position(event.clientX);update({...g,stops:g.stops.map((s,i)=>i!==index?s:midpoint?{...s,midpoint:Math.max(.01,Math.min(.99,(p-s.position)/Math.max(.00001,g.stops[i+1].position-s.position)))}:{...s,position:Math.max(g.stops[i-1]?.position??0,Math.min(g.stops[i+1]?.position??1,p))})});};
    const end=()=>{el.removeEventListener('pointermove',move);el.removeEventListener('pointerup',end);el.removeEventListener('pointercancel',end);commit();};el.addEventListener('pointermove',move);el.addEventListener('pointerup',end);el.addEventListener('pointercancel',end);
  };
  const s=gradient.stops[Math.min(stop,gradient.stops.length-1)];
  return <div className="gradient-panel" aria-busy={busy}><header><h2>{t.title}</h2><span aria-hidden="true">≡</span></header>
    <fieldset disabled={!enabled||busy}><div className="gradient-preset-heading">{t.presets}<select aria-label={t.presets} value="" onChange={e=>{const g=structuredClone(presets[Number(e.target.value)]);update(g);setStop(0);if(!pixels)void apply(g);}}><option value="">▾</option>{presets.map((_,i)=><option value={i} key={i}>{i+1}</option>)}</select></div><div className="gradient-presets">{presets.map((g,i)=><button key={i} aria-label={`${t.presets} ${i+1}`} style={{background:css(g)}} onClick={()=>{const next=structuredClone(g);update(next);setStop(0);if(!pixels)void apply(next);}}/>)}</div></fieldset>
    <fieldset disabled={!editable}><div className="gradient-type-row"><div className="gradient-swatch" style={{background:css(gradient)}}/><div><label className="gradient-field"><span>{t.target}</span>{pixels?<span>{t.pixels}</span>:<select value={target} onChange={e=>setTarget(e.target.value as 'fill'|'stroke')}><option value="fill">{t.fill}</option><option value="stroke">{t.stroke}</option></select>}</label><div className="gradient-types"><span>{t.type}</span>{(['linear','radial'] as const).map(kind=><button aria-label={t[kind]} title={t[kind]} aria-pressed={gradient.kind===kind} key={kind} onClick={()=>change({kind},true)} style={{background:kind==='linear'?'linear-gradient(90deg,#fff,#222)':'radial-gradient(#fff,#222)'}}/>)}</div></div></div>
    <NumberField label={`${t.angle} °`} value={gradient.angle} min={-360} max={360} onDraft={angle=>change({angle})} onCommit={commit}/>
    {gradient.kind==='radial'&&<NumberField label={`${t.aspect} %`} value={gradient.aspect*100} min={1} max={1000} onDraft={n=>change({aspect:n/100})} onCommit={commit}/>}
    <label className="gradient-field"><span>{t.method}</span><select value={gradient.method} onChange={e=>change({method:e.target.value as Gradient['method']},true)}><option value="classic">{t.classic}</option><option value="perceptual">{t.perceptual}</option><option value="linear">{t.light}</option></select></label>
    <label className="gradient-dither"><input type="checkbox" checked={!!gradient.dither && (pixels||target==='fill')} disabled={!pixels&&target==='stroke'} onChange={e=>change({dither:e.target.checked},true)}/><span>{t.dither}</span></label>
    <div className="gradient-ramp-wrap"><button title={t.reverse} aria-label={t.reverse} onClick={()=>{const stops=[...gradient.stops].reverse().map((s,i)=>({...s,position:1-s.position,midpoint:i<gradient.stops.length-1?1-gradient.stops[gradient.stops.length-2-i].midpoint:.5}));change({stops},true);setStop(gradient.stops.length-1-stop);}}>⇄</button><div className="gradient-ramp" ref={ramp} style={{background:css({...gradient,kind:'linear',angle:0})}} onClick={e=>{if(e.target!==e.currentTarget||gradient.stops.length>=64)return;const p=position(e.clientX);const next=[...gradient.stops,{position:p,color:sample(gradient,p),midpoint:.5}].sort((a,b)=>a.position-b.position);setStop(next.findIndex(x=>x.position===p));change({stops:next},true);}}>
    {gradient.stops.map((item,i)=><button key={i} className={`gradient-stop ${i===stop?'selected':''}`} title={`${t.position} ${Math.round(item.position*100)}%`} aria-label={`${t.color} ${i+1}`} style={{left:`${item.position*100}%`,background:`rgba(${item.color.slice(0,3).join(',')},${item.color[3]/255})`}} onClick={e=>{e.stopPropagation();setStop(i);}} onPointerDown={e=>drag(e,i)} onKeyDown={e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();setStop(i);update({...gradient,stops:gradient.stops.map((v,j)=>j===i?{...v,position:Math.max(gradient.stops[i-1]?.position??0,Math.min(gradient.stops[i+1]?.position??1,v.position+(e.key==='ArrowLeft'?-.01:.01)))}:v)});commit();}}}/>)}
    {gradient.stops.slice(0,-1).map((v,i)=><button key={i} className="gradient-midpoint" title={t.midpoint} aria-label={`${t.midpoint} ${i+1}`} style={{left:`${(v.position+(gradient.stops[i+1].position-v.position)*v.midpoint)*100}%`}} onPointerDown={e=>drag(e,i,true)}/>)}
    </div><button title={t.remove} aria-label={t.remove} disabled={gradient.stops.length<=2} onClick={()=>{change({stops:gradient.stops.filter((_,i)=>i!==stop)},true);setStop(Math.max(0,stop-1));}}>⌫</button></div>
    <label className="gradient-field"><span>{t.color}</span><input type="color" value={`#${s.color.slice(0,3).map(c=>c.toString(16).padStart(2,'0')).join('')}`} onChange={e=>{const hex=e.target.value;changeStop({color:[parseInt(hex.slice(1,3),16),parseInt(hex.slice(3,5),16),parseInt(hex.slice(5,7),16),s.color[3]]});}} onBlur={commit}/></label>
    <NumberField label={`${t.opacity} %`} value={s.color[3]/255*100} min={0} max={100} onDraft={n=>changeStop({color:[s.color[0],s.color[1],s.color[2],Math.round(n/100*255)]})} onCommit={commit}/>
    <NumberField label={`${t.position} %`} value={s.position*100} min={(gradient.stops[stop-1]?.position??0)*100} max={(gradient.stops[stop+1]?.position??1)*100} onDraft={n=>changeStop({position:n/100})} onCommit={commit}/>
    {stop<gradient.stops.length-1&&<NumberField label={`${t.midpoint} %`} value={s.midpoint*100} min={1} max={99} onDraft={n=>changeStop({midpoint:n/100})} onCommit={commit}/>}
    <button className="gradient-apply" onClick={()=>void apply()}>{t.apply}</button></fieldset>
    <p className="gradient-hint">{editable?(pixels?t.pixelHint:t.hint):t.select}</p>{error&&<p role="alert" className="gradient-error">{error}</p>}
  </div>;
}
