import { useRef, useState, type PointerEvent } from 'react';
import type { Locale } from '../i18n';
type Point = [number, number];
const labels = {
  ja: { graph:'カーブ編集', point:'ポイント', input:'入力', output:'出力', smooth:'滑らかな曲線', remove:'ポイントを削除', reset:'チャンネルをリセット', hint:'クリックで追加、ドラッグで移動。Deleteで削除。', limit:'ポイントは最大16個です。' },
  en: { graph:'Edit curve', point:'Point', input:'Input', output:'Output', smooth:'Smooth curve', remove:'Delete point', reset:'Reset channel', hint:'Click to add, drag to move. Delete removes a point.', limit:'Maximum 16 points.' },
  'zh-CN': { graph:'编辑曲线', point:'控制点', input:'输入', output:'输出', smooth:'平滑曲线', remove:'删除控制点', reset:'重置通道', hint:'点击添加，拖动移动。Delete删除控制点。', limit:'最多16个控制点。' },
};
function tangent(points: Point[], i: number) {
  const slope=(j:number)=>(points[j+1][1]-points[j][1])/(points[j+1][0]-points[j][0]);
  if(i===0)return slope(0);if(i===points.length-1)return slope(i-1);
  const a=slope(i-1),b=slope(i);if(a*b<=0)return 0;
  const h0=points[i][0]-points[i-1][0],h1=points[i+1][0]-points[i][0];
  const w1=2*h1+h0,w2=h1+2*h0;return (w1+w2)/(w1/a+w2/b);
}
function path(points: Point[], smooth: boolean) {
  let d=`M0 ${255-points[0][1]*255} L${points[0][0]*255} ${255-points[0][1]*255}`;
  for(let i=0;i<points.length-1;i++){
    const a=points[i],b=points[i+1],h=(b[0]-a[0])/3;
    d+=smooth?` C${(a[0]+h)*255} ${255-(a[1]+h*tangent(points,i))*255} ${(b[0]-h)*255} ${255-(b[1]-h*tangent(points,i+1))*255} ${b[0]*255} ${255-b[1]*255}`:` L${b[0]*255} ${255-b[1]*255}`;
  }
  return d+` L255 ${255-points.at(-1)![1]*255}`;
}
function Coordinate({value,label,disabled,onCommit}:{value:number;label:string;disabled:boolean;onCommit:(value:number)=>void}) {
  const [text,setText]=useState(String(Math.round(value*255)));
  const valid=text.trim()!==''&&Number.isFinite(Number(text))&&Number(text)>=0&&Number(text)<=255;
  return <label><span>{label}</span><input className="effect-number" aria-label={label} disabled={disabled} inputMode="decimal" value={text} aria-invalid={!valid} onChange={e=>setText(e.currentTarget.value)} onBlur={()=>{if(valid)onCommit(Number(text)/255);else setText(String(Math.round(value*255)));}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){setText(String(Math.round(value*255)));e.preventDefault();}}}/></label>;
}
export function CurveEditor({locale,channel,points,smooth,disabled,onChange,onCommit}:{locale:Locale;channel:string;points:Point[];smooth:boolean;disabled:boolean;onChange:(points:Point[],smooth:boolean)=>void;onCommit:(points:Point[],smooth:boolean)=>void}) {
  const t=labels[locale];const [selected,setSelected]=useState(0);
  const index=Math.min(selected,points.length-1);
  const latest=useRef(points);latest.current=points;
  const drag=useRef<{id:number;index:number;before:Point[]}|null>(null);
  const update=(next:Point[])=>{latest.current=next;onChange(next,smooth);};
  const commit=(next:Point[])=>{latest.current=next;onCommit(next,smooth);};
  const coords=(e:PointerEvent<SVGSVGElement>):Point=>{const r=e.currentTarget.getBoundingClientRect();return [(e.clientX-r.left)/r.width*279-12,255-((e.clientY-r.top)/r.height*279-12)];};
  const moved=(i:number,x:number,y:number)=>{const current=latest.current;const lo=i===0?0:current[i-1][0]+1/65535;const hi=i===current.length-1?1:current[i+1][0]-1/65535;return current.map((p,j)=>j===i?[Math.max(lo,Math.min(hi,x)),Math.max(0,Math.min(1,y))] as Point:p);};
  const remove=()=>{if(disabled||index===0||index===points.length-1)return;commit(points.filter((_,i)=>i!==index));setSelected(index-1);};
  return <div className="effect-curve-editor"><p className="effect-hint">{t.hint}</p><svg className="effect-curve interactive" viewBox="-12 -12 279 279" role="group" tabIndex={disabled?-1:0} aria-label={`${channel} ${t.graph}`} aria-disabled={disabled}
    onPointerDown={e=>{if(disabled||e.button!==0)return;e.preventDefault();e.currentTarget.focus();const [x,y]=coords(e);const radius=10*279/e.currentTarget.getBoundingClientRect().width;let found=points.findIndex(p=>Math.hypot(p[0]*255-x,p[1]*255-y)<radius);const before=points.map(p=>[...p] as Point);if(found<0){if(points.length>=16||x<=points[0][0]*255||x>=points.at(-1)![0]*255)return;const p:Point=[Math.round(x)/255,Math.max(0,Math.min(255,Math.round(y)))/255];if(points.some(q=>Math.abs(q[0]-p[0])<1/65535))return;const next=[...points,p].sort((a,b)=>a[0]-b[0]);found=next.indexOf(p);update(next);}setSelected(found);drag.current={id:e.pointerId,index:found,before};e.currentTarget.setPointerCapture(e.pointerId);}}
    onPointerMove={e=>{const g=drag.current;if(!g||g.id!==e.pointerId)return;const [x,y]=coords(e);update(moved(g.index,Math.round(x)/255,Math.round(y)/255));}}
    onPointerUp={e=>{const g=drag.current;if(!g||g.id!==e.pointerId)return;drag.current=null;const [x,y]=coords(e);if(g.index>0&&g.index<latest.current.length-1&&(x< -12||x>267||y< -12||y>267)){const next=latest.current.filter((_,i)=>i!==g.index);setSelected(g.index-1);commit(next);}else commit(latest.current);}}
    onPointerCancel={()=>{const g=drag.current;drag.current=null;if(g)update(g.before);}}
    onLostPointerCapture={()=>{const g=drag.current;drag.current=null;if(g)update(g.before);}}
    onKeyDown={e=>{if(disabled)return;if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();e.stopPropagation();remove();return;}if(e.key==='Escape'){e.preventDefault();e.stopPropagation();const g=drag.current;drag.current=null;if(g)update(g.before);return;}if(e.key.startsWith('Arrow')){e.preventDefault();e.stopPropagation();const step=(e.shiftKey?10:1)/255;const p=latest.current[index];update(moved(index,p[0]+(e.key==='ArrowRight'?step:e.key==='ArrowLeft'?-step:0),p[1]+(e.key==='ArrowUp'?step:e.key==='ArrowDown'?-step:0)));}}}
    onKeyUp={e=>{if(!disabled&&e.key.startsWith('Arrow')){e.stopPropagation();commit(latest.current);}}}>
    <rect x="0" y="0" width="255" height="255" fill="var(--color-field)"/><path d="M0 63.75H255 M0 127.5H255 M0 191.25H255 M63.75 0V255 M127.5 0V255 M191.25 0V255" stroke="var(--color-rule)" fill="none"/><path d="M0 255L255 0" stroke="var(--color-muted)" opacity=".4" strokeDasharray="3 3"/>
    <path d={path(points,smooth)} fill="none" stroke="var(--color-ink)" strokeWidth="1.8"/>{points.map(([x,y],i)=><circle key={i} cx={x*255} cy={255-y*255} r={i===index?4.5:3.5} fill={i===index?'var(--color-accent)':'var(--color-panel)'} stroke="var(--color-ink)"/>)}</svg>
    <div className="effect-curve-toolbar"><label><input type="checkbox" disabled={disabled} checked={smooth} onChange={e=>onCommit(points,e.currentTarget.checked)}/>{t.smooth}</label><select aria-label={`${channel} ${t.point}`} disabled={disabled} value={index} onChange={e=>setSelected(Number(e.currentTarget.value))}>{points.map((_,i)=><option key={i} value={i}>{t.point} {i+1}</option>)}</select></div>
    <div className="effect-curve-coordinates"><Coordinate key={`x-${index}-${points[index][0]}`} value={points[index][0]} label={t.input} disabled={disabled} onCommit={v=>commit(moved(index,v,points[index][1]))}/><Coordinate key={`y-${index}-${points[index][1]}`} value={points[index][1]} label={t.output} disabled={disabled} onCommit={v=>commit(moved(index,points[index][0],v))}/></div>
    <div className="effect-curve-actions"><button disabled={disabled||index===0||index===points.length-1} onClick={remove}>{t.remove}</button><button disabled={disabled} onClick={()=>{setSelected(0);onCommit([[0,0],[1,1]],true);}}>{t.reset}</button></div>{points.length===16&&<p className="effect-hint">{t.limit}</p>}
  </div>;
}
