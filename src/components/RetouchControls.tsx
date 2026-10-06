import {useEffect,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {CanvasTool} from '../bridge';
import type {Locale} from '../i18n';
export const retouchTools=['blur','sharpen','smudge'] as const;
export const isRetouch=(tool:CanvasTool)=>retouchTools.includes(tool as typeof retouchTools[number]);
export const defaultRetouchSettings={strength:.5,sampleAllLayers:false,protectDetail:true,fingerPainting:false,pressureSize:false,pressureStrength:true};
type Settings=typeof defaultRetouchSettings;
export const retouchLabels={ja:{strength:'強さ',all:'すべてのレイヤー',detail:'ディテールを保護',finger:'指先ペイント',size:'筆圧：サイズ',pressure:'筆圧：強さ',hint:'ドラッグで補正 · U：ぼかし · Shift+U：切替',fingerHint:'Option：描画色で指先ペイントを開始'},en:{strength:'Strength',all:'Sample all layers',detail:'Protect detail',finger:'Finger painting',size:'Pressure: size',pressure:'Pressure: strength',hint:'Drag to retouch · U: Blur · Shift+U: cycle',fingerHint:'Option with Smudge: start with foreground'},'zh-CN':{strength:'强度',all:'对所有图层取样',detail:'保护细节',finger:'手指绘画',size:'压感：大小',pressure:'压感：强度',hint:'拖动修饰 · U：模糊 · Shift+U：切换',fingerHint:'Option：使用前景色开始涂抹'}};
export function RetouchControls({tool,locale,details=false,onChange,onError}:{tool:CanvasTool;locale:Locale;details?:boolean;onChange?:(settings:Settings)=>void;onError?:(message:string)=>void}){
 const t=retouchLabels[locale], [settings,setSettings]=useState(defaultRetouchSettings);
 useEffect(()=>{let live=true;const refresh=()=>{if(isTauri())void invoke<Settings>('retouch_settings').then(v=>{if(live){setSettings(v);onChange?.(v);}}).catch(e=>onError?.(String(e)));};refresh();const sub=isTauri()?listen('retouch-settings-changed',refresh):null;return()=>{live=false;void sub?.then(f=>f());};},[]);
 const update=(patch:Partial<Settings>)=>{const next={...settings,...patch};setSettings(next);if(onChange)onChange(next);else if(isTauri())void invoke('set_retouch_settings',{settings:next}).catch(e=>onError?.(String(e)));};
 const check=(label:string,key:Exclude<keyof Settings,'strength'>)=><label><input type="checkbox" checked={settings[key]} onChange={e=>update({[key]:e.target.checked})}/>{label}</label>;
 return <div className={`clone-stamp-controls ${details?'clone-stamp-details':''}`} title={t.hint}>
 <label>{t.strength}<span className="clone-stamp-value"><input aria-label={t.strength} type="number" min="0" max="100" value={Math.round(settings.strength*100)} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value)&&value>=0&&value<=100)update({strength:value/100});}}/>%</span></label>
 {check(t.all,'sampleAllLayers')}{tool==='sharpen'&&check(t.detail,'protectDetail')}{tool==='smudge'&&check(t.finger,'fingerPainting')}
 {details&&<>{check(t.size,'pressureSize')}{check(t.pressure,'pressureStrength')}<p>{t.hint}</p>{tool==='smudge'&&<p>{t.fingerHint}</p>}</>}
 </div>;
}
