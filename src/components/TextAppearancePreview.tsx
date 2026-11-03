import {useId} from 'react';
import type {Locale} from '../i18n';
import type {TextAppearancePaint} from '../bridge';

/** Small UI sample only; document rendering remains on the Rust side. */
export function TextAppearancePreview({paint,locale,sample,amount=1}:{paint:TextAppearancePaint;locale:Locale;sample:string;amount?:number}) {
  const id=useId().replace(/:/g,''),size=locale==='en'?32:30,unit=size*amount/100;
  const angle=paint.fill.angle*Math.PI/180,dx=Math.sin(angle),dy=Math.cos(angle),extent=Math.abs(dx)+Math.abs(dy);
  const shadow=paint.shadow;
  return <svg aria-hidden="true" viewBox="0 0 128 52"><defs>
    <linearGradient id={id} x1={.5-dx*extent/2} x2={.5+dx*extent/2} y1={.5-dy*extent/2} y2={.5+dy*extent/2}>{paint.fill.colors.map((color,i)=><stop key={i} offset={i/4} stopColor={color}/>)}</linearGradient>
    <filter id={`${id}-shadow`} x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation={unit*shadow.blur}/></filter>
    <text id={`${id}-text`} x="64" y="35" textAnchor="middle" fontFamily="sans-serif" fontWeight="900" fontSize={size}>{sample}</text>
  </defs><g strokeLinejoin="round">
    {shadow.opacity>0&&<use href={`#${id}-text`} fill={shadow.color} stroke={shadow.color} strokeWidth={unit*shadow.width} opacity={shadow.opacity/100} transform={`translate(${unit*shadow.offsetX} ${unit*shadow.offsetY})`} filter={`url(#${id}-shadow)`}/>}
    {paint.outlines.map((outline,i)=>outline.width>0&&<use key={i} href={`#${id}-text`} fill={outline.color} stroke={outline.color} strokeWidth={paint.outlines.slice(i).reduce((sum,o)=>sum+o.width,0)*unit*2}/>)}
    <use href={`#${id}-text`} fill={paint.fill.gradient?`url(#${id})`:paint.fill.colors[0]}/>
  </g></svg>;
}
