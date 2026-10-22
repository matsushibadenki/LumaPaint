import {useEffect,useRef,useState} from 'react';
import {ColorPickerPopover} from './ColorPickerPopover';
import type {Locale} from '../i18n';
export type OnionSettings={scope?:'layer'|'scene';enabled:boolean;before:number;after:number;step:number;opacity:number;fade:number;previousColor:number[];nextColor:number[];spacing:'cels'|'frames';color:'original'|'half'|'tint';fixedFrames:number[]};
export const onionLabels={
 ja:{scope:'参照範囲',layer:'選択レイヤー',scene:'全体シーン',title:'オニオンスキン',before:'前のコマ',after:'後のコマ',step:'間隔',opacity:'不透明度',fade:'減衰',spacing:'参照単位',cels:'コマ',frames:'フレーム',color:'表示色',original:'元の色',half:'混合色',tint:'指定色',previous:'前の色',next:'後の色',pin:'現在のフレームを固定参照',fixed:'固定参照',remove:'参照を解除',info:'選択レイヤーまたは全体シーンの前後を参照表示します。全体シーンに連続モーションがある場合はフレーム単位です。作画・保存画像には混ざりません。再生中は非表示です。',thumbnails:'コマプレビュー',none:'なし'},
 en:{scope:'Reference scope',layer:'Selected layer',scene:'Entire scene',title:'Onion skin',before:'Previous cels',after:'Next cels',step:'Step',opacity:'Opacity',fade:'Fade',spacing:'Reference unit',cels:'Cels',frames:'Frames',color:'Color mode',original:'Original',half:'Half color',tint:'Tint',previous:'Previous color',next:'Next color',pin:'Pin current frame',fixed:'Fixed references',remove:'Unpin reference',info:'Show neighboring pictures of the selected layer or entire scene. Scenes with continuous motion use frame spacing. References do not alter drawings or exported images and are hidden during playback.',thumbnails:'Cel previews',none:'None'},
 'zh-CN':{scope:'参考范围',layer:'选中图层',scene:'整个场景',title:'洋葱皮',before:'前面的画格',after:'后面的画格',step:'间隔',opacity:'不透明度',fade:'衰减',spacing:'参考单位',cels:'画格',frames:'帧',color:'显示颜色',original:'原色',half:'混合色',tint:'指定颜色',previous:'前帧颜色',next:'后帧颜色',pin:'固定参考当前帧',fixed:'固定参考',remove:'取消固定',info:'显示选中图层或整个场景的前后画面。包含连续动画的场景按帧参考。参考图像不会修改绘画或导出图像，播放时隐藏。',thumbnails:'画格预览',none:'无'},
};
export function OnionControls({locale,settings,frame,disabled,onChange}:{locale:Locale;settings:OnionSettings;frame:number;disabled:boolean;onChange:(value:OnionSettings)=>void}) {
 const t=onionLabels[locale];
 const [draft,setDraft]=useState(settings);
 const editing=useRef(false);
 const colorDraft=useRef(draft); colorDraft.current=draft;
 useEffect(()=>{if(!editing.current)setDraft(settings);},[settings]);
 function commit(value:OnionSettings){setDraft(value);onChange(value);}
 function numeric(key:'before'|'after'|'step'|'opacity'|'fade',min:number,max:number){return <label key={key}>{t[key]}<input aria-label={`${t.title} · ${t[key]}`} type="number" min={min} max={max} step={1} value={Number.isFinite(draft[key])?draft[key]:''} disabled={disabled} onFocus={()=>{editing.current=true;}} onChange={e=>setDraft({...draft,[key]:e.target.value===''?NaN:Number(e.target.value)})} onBlur={()=>{editing.current=false;const value=draft[key];if(Number.isFinite(value)&&Number.isInteger(value)&&value>=min&&value<=max)commit(draft);else setDraft(settings);}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></label>;}
 return <fieldset className="timeline-onion-settings" disabled={disabled}>
  <legend>{t.title} <span className="timeline-info" title={t.info} aria-label={t.info}>ⓘ</span></legend>
  <div className="timeline-onion-grid">{numeric('before',0,4)}{numeric('after',0,4)}{numeric('opacity',0,100)}{numeric('fade',0,100)}</div>
  <label>{t.scope}<select aria-label={t.scope} value={draft.scope??'layer'} onChange={e=>commit({...draft,scope:e.target.value as OnionSettings['scope']})}><option value="layer">{t.layer}</option><option value="scene">{t.scene}</option></select></label>
  <label>{t.spacing}<select value={draft.spacing} onChange={e=>commit({...draft,spacing:e.target.value as OnionSettings['spacing']})}><option value="cels">{t.cels}</option><option value="frames">{t.frames}</option></select></label>
  {numeric('step',1,24)}
  <label>{t.color}<select value={draft.color} onChange={e=>commit({...draft,color:e.target.value as OnionSettings['color']})}>{(['original','half','tint'] as const).map(color=><option key={color} value={color}>{t[color]}</option>)}</select></label>
  <div className="timeline-onion-grid">{(['previousColor','nextColor'] as const).map((key,index)=><label key={key}>{index===0?t.previous:t.next}<ColorPickerPopover locale={locale} label={index===0?t.previous:t.next} disabled={disabled} color={[draft[key][0],draft[key][1],draft[key][2]]} onOpen={()=>{editing.current=true;}} onChange={color=>{const next={...colorDraft.current,[key]:color};colorDraft.current=next;setDraft(next);}} onClose={()=>{editing.current=false;commit(colorDraft.current);}}/></label>)}</div>
  <button disabled={disabled||!settings.fixedFrames.includes(frame)&&settings.fixedFrames.length>=4} aria-pressed={settings.fixedFrames.includes(frame)} onClick={()=>commit({...draft,fixedFrames:settings.fixedFrames.includes(frame)?settings.fixedFrames.filter(f=>f!==frame):[...settings.fixedFrames,frame].sort((a,b)=>a-b)})}>⌖ {t.pin}</button>
  {!!settings.fixedFrames.length&&<div className="timeline-fixed-references" aria-label={t.fixed}>{settings.fixedFrames.map(f=><button key={f} title={`${t.remove} · ${f+1}`} aria-label={`${t.remove} · ${f+1}`} onClick={()=>commit({...draft,fixedFrames:settings.fixedFrames.filter(frame=>frame!==f)})}>{f+1} ×</button>)}</div>}
 </fieldset>;
}
