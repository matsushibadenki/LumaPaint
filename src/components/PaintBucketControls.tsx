import {listen} from '@tauri-apps/api/event';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {useEffect,useState} from 'react';
import type {Locale} from '../i18n';
import {cloneStampLabels} from './CloneStampControls';
export const paintBucketLabels={ja:{source:'塗りつぶし',foreground:'描画色',pattern:'パターン',patterns:['市松模様','斜線','ドット'],patternSize:'パターン幅',tolerance:'許容値',antiAlias:'アンチエイリアス',contiguous:'隣接',allLayers:'すべてのレイヤー',hint:'領域をクリックして塗りつぶし · Shift+G：塗りつぶし'},en:{source:'Fill',foreground:'Foreground',pattern:'Pattern',patterns:['Checker','Stripes','Dots'],patternSize:'Pattern size',tolerance:'Tolerance',antiAlias:'Anti-alias',contiguous:'Contiguous',allLayers:'All layers',hint:'Click an area to fill · Shift+G: Paint Bucket'},'zh-CN':{source:'填充',foreground:'前景色',pattern:'图案',patterns:['棋盘','斜线','圆点'],patternSize:'图案大小',tolerance:'容差',antiAlias:'消除锯齿',contiguous:'连续',allLayers:'所有图层',hint:'单击区域填充 · Shift+G：油漆桶'}};
export const defaultPaintBucketSettings={source:'foreground',pattern:'checker',patternSize:8,mode:'normal',opacity:1,tolerance:32,antiAlias:true,contiguous:true,allLayers:false};
type Settings=typeof defaultPaintBucketSettings;
export function PaintBucketControls({locale,details=false,onError,onChange}:{locale:Locale;details?:boolean;onError?:(error:string)=>void;onChange?:(settings:Settings)=>void}){
 const t=paintBucketLabels[locale],common=cloneStampLabels[locale];const [settings,setSettings]=useState(defaultPaintBucketSettings);
 useEffect(()=>{let live=true;const refresh=()=>{if(isTauri())void invoke<Settings>('paint_bucket_settings').then(value=>{if(live){setSettings(value);onChange?.(value);}}).catch(error=>onError?.(String(error)));};refresh();const listener=isTauri()?listen('paint-bucket-settings-changed',refresh):null;return()=>{live=false;void listener?.then(fn=>fn());};},[]);
 const update=(patch:Partial<Settings>)=>{const next={...settings,...patch};setSettings(next);if(onChange)onChange(next);else if(isTauri())void invoke('set_paint_bucket_settings',{settings:next}).catch(error=>onError?.(String(error)));};
 const number=(label:string,key:'opacity'|'tolerance'|'patternSize',min:number,max:number,scale=1)=><label><span>{label}</span><span className="clone-stamp-value"><input type="number" aria-label={label} min={min} max={max} value={Math.round(settings[key]*scale)} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isInteger(value)&&value>=min&&value<=max)update({[key]:value/scale});}}/>{scale===100?'%':''}</span></label>;
 return <div title={t.hint} className={`paint-bucket-controls clone-stamp-controls ${details?'clone-stamp-details':''}`}>
 <label>{t.source}<select value={settings.source} onChange={e=>update({source:e.target.value})}><option value="foreground">{t.foreground}</option><option value="pattern">{t.pattern}</option></select></label>
 {settings.source==='pattern'&&<><label>{t.pattern}<select value={settings.pattern} onChange={e=>update({pattern:e.target.value})}>{['checker','stripes','dots'].map((v,i)=><option key={v} value={v}>{t.patterns[i]}</option>)}</select></label>{number(t.patternSize,'patternSize',2,512)}</>}
 <label>{common.mode}<select value={settings.mode} onChange={e=>update({mode:e.target.value})}>{['normal','multiply','screen','overlay','darken','lighten'].map((v,i)=><option key={v} value={v}>{common.modes[i]}</option>)}</select></label>
 {number(common.opacity,'opacity',0,100,100)}{number(t.tolerance,'tolerance',0,255)}
 {(['antiAlias','contiguous','allLayers'] as const).map(key=><label key={key}><input type="checkbox" checked={settings[key]} onChange={e=>update({[key]:e.target.checked})}/>{t[key]}</label>)}
 {details&&<p>{t.hint}</p>}
 </div>;
}
