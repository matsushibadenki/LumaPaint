import { listen } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Locale } from '../i18n';
export const cloneStampLabels={en:{mode:'Mode',opacity:'Opacity',flow:'Flow',aligned:'Aligned',sample:'Sample',layers:['Current layer','Current & below','All layers'],modes:['Normal','Multiply','Screen','Overlay','Darken','Lighten'],angle:'Angle',roundness:'Roundness',pressureSize:'Pressure: size',pressureOpacity:'Pressure: opacity',hint:'Option / Alt-click: set source · S: Clone Stamp'},ja:{mode:'モード',opacity:'不透明度',flow:'流量',aligned:'調整あり',sample:'サンプル',layers:['現在のレイヤー','現在のレイヤー以下','すべてのレイヤー'],modes:['通常','乗算','スクリーン','オーバーレイ','比較（暗）','比較（明）'],angle:'角度',roundness:'真円率',pressureSize:'筆圧：サイズ',pressureOpacity:'筆圧：不透明度',hint:'Option / Altクリック：コピー元を指定 · S：コピースタンプ'},'zh-CN':{mode:'模式',opacity:'不透明度',flow:'流量',aligned:'对齐',sample:'取样',layers:['当前图层','当前及以下图层','所有图层'],modes:['正常','正片叠底','滤色','叠加','变暗','变亮'],angle:'角度',roundness:'圆度',pressureSize:'压感：大小',pressureOpacity:'压感：不透明度',hint:'Option / Alt单击：设置取样点 · S：仿制图章'}};
export const defaultCloneStampSettings={opacity:1,flow:1,aligned:true,sample:'currentLayer',mode:'normal',angle:0,roundness:1,pressureSize:false,pressureOpacity:false};
type CloneStampSettings=typeof defaultCloneStampSettings;
export function CloneStampControls({locale,details=false,onError,onChange}:{locale:Locale;details?:boolean;onError?:(message:string)=>void;onChange?:(settings:CloneStampSettings)=>void}){
 const t=cloneStampLabels[locale];const [settings,setSettings]=useState(defaultCloneStampSettings);
 useEffect(()=>{let live=true;const refresh=()=>{if(isTauri())void invoke<CloneStampSettings>('clone_stamp_settings').then(value=>{if(live){setSettings(value);onChange?.(value);}}).catch(error=>onError?.(String(error)));};refresh();const subscription=isTauri()?listen('clone-stamp-settings-changed',refresh):null;return()=>{live=false;void subscription?.then(unlisten=>unlisten());};},[]);
 const update=(patch:Partial<CloneStampSettings>)=>{const next={...settings,...patch};setSettings(next);if(onChange)onChange(next);else if(isTauri())void invoke('set_clone_stamp_settings',{settings:next}).catch(error=>onError?.(String(error)));};
 const number=(label:string,key:'opacity'|'flow'|'angle'|'roundness',min:number,max:number,scale=1)=><label><span>{label}</span><span className="clone-stamp-value"><input aria-label={label} type="number" min={min} max={max} value={Math.round(settings[key]*scale)} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value)&&value>=min&&value<=max)update({[key]:value/scale});}}/>{scale===100?'%':key==='angle'?'°':''}</span></label>;
 return <div title={t.hint} className={`clone-stamp-controls ${details?'clone-stamp-details':''}`}>
 <label>{t.mode}<select value={settings.mode} onChange={e=>update({mode:e.target.value})}>{['normal','multiply','screen','overlay','darken','lighten'].map((mode,i)=><option key={mode} value={mode}>{t.modes[i]}</option>)}</select></label>
 {number(t.opacity,'opacity',0,100,100)}{number(t.flow,'flow',0,100,100)}
 <label><input type="checkbox" checked={settings.aligned} onChange={e=>update({aligned:e.target.checked})}/>{t.aligned}</label>
 <label>{t.sample}<select value={settings.sample} onChange={e=>update({sample:e.target.value})}>{['currentLayer','currentBelow','allLayers'].map((sample,i)=><option key={sample} value={sample}>{t.layers[i]}</option>)}</select></label>
 {details&&<>{number(t.angle,'angle',-360,360)}{number(t.roundness,'roundness',1,100,100)}<label><input type="checkbox" checked={settings.pressureSize} onChange={e=>update({pressureSize:e.target.checked})}/>{t.pressureSize}</label><label><input type="checkbox" checked={settings.pressureOpacity} onChange={e=>update({pressureOpacity:e.target.checked})}/>{t.pressureOpacity}</label><p>{t.hint}</p></>}
 </div>;
}
