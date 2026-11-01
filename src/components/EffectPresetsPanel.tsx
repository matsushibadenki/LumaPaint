import { useEffect, useRef, useState } from 'react';
import { effectPresetLibrary, onEffectPresetsChanged, type EffectPresetLibrary, type EffectPresetSettings, type LayerEffects } from '../bridge';
import type { Locale } from '../i18n';
import { PanelBlock } from './PanelBlock';
import { Icon } from './Icon';

export const effectPresetLabels = {
  ja: { title:'エフェクトプリセット', hint:'補正値・カーブ・ミキサー・グレーディングと適用状態をアプリ共通で保存します。適用先のマスク・スクリーントーンは維持します。', select:'エフェクトプリセットを選択', name:'プリセット名', base:'プリセット', save:'現在のエフェクトを保存', apply:'プリセットを適用', update:'現在のエフェクトで更新', rename:'プリセット名を変更', remove:'プリセットを削除', retry:'プリセットを再読み込み', error:'プリセットを読み込み・保存できませんでした', limit:'プリセットは64件まで保存できます' },
  en: { title:'Effect presets', hint:'Share adjustments, curves, mixer, grading and enabled state across the app. Applying preserves the destination mask and screentone.', select:'Choose an effect preset', name:'Preset name', base:'Preset', save:'Save current effects', apply:'Apply preset', update:'Update from current effects', rename:'Rename preset', remove:'Delete preset', retry:'Reload presets', error:'Could not load or save presets', limit:'Up to 64 presets can be saved' },
  'zh-CN': { title:'效果预设', hint:'在应用中共享调整值、曲线、混合器、颜色分级和启用状态。应用时保留目标图层的蒙版和网点。', select:'选择效果预设', name:'预设名称', base:'预设', save:'保存当前效果', apply:'应用预设', update:'使用当前效果更新', rename:'重命名预设', remove:'删除预设', retry:'重新加载预设', error:'无法加载或保存预设', limit:'最多可保存64个预设' },
};
/** Explicit whitelist: a library entry must never contain layer-local mask data. */
export function captureEffectSettings(effects: LayerEffects): EffectPresetSettings {
  return structuredClone({ enabled:effects.enabled, values:effects.values, curves:effects.curves, curveSmooth:effects.curveSmooth,
    mixer:effects.mixer, grading:effects.grading, gradingBlend:effects.gradingBlend, gradingBalance:effects.gradingBalance });
}
export function EffectPresetsPanel({locale, effects, disabled, onApply}: {locale:Locale;effects:LayerEffects;disabled:boolean;onApply:(settings:EffectPresetSettings)=>void}) {
  const t=effectPresetLabels[locale];
  const [library,setLibrary]=useState<EffectPresetLibrary|null>(null),[selected,setSelected]=useState(''),[name,setName]=useState('');
  const [ready,setReady]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[reload,setReload]=useState(0);
  const pending=useRef(false),mounted=useRef(false),initialized=useRef(false);
  const accept=(next:EffectPresetLibrary)=>{
    setLibrary(previous=>previous&&previous.revision>next.revision?previous:next);
    // A fresh panel offers the first stored look without applying it.
    if(!initialized.current){initialized.current=true;const first=next.presets[0];if(first){setSelected(String(first.id));setName(first.name);}}
  };
  useEffect(()=>{
    let live=true,received=false;let unlisten:(()=>void)|undefined;mounted.current=true;setReady(false);setError('');
    void (async()=>{
      try {
        const stop=await onEffectPresetsChanged(next=>{if(live){received=true;accept(next);setReady(true);setError('');}});
        if(!live){stop();return;}unlisten=stop;
        const next=await effectPresetLibrary('get');
        if(live){accept(next);setReady(true);}
      } catch(cause){if(live&&!received)setError(String(cause));}
    })();
    return ()=>{live=false;mounted.current=false;unlisten?.();};
  },[reload]);
  const target=library?.presets.find(preset=>String(preset.id)===selected);
  const blocked=disabled||busy||!ready;
  async function run(action:'add'|'update'|'rename'|'remove') {
    if(blocked||pending.current||(!target&&action!=='add'))return;
    pending.current=true;setBusy(true);setError('');
    try {
      const next=await effectPresetLibrary(action,target?.id,name.trim()||`${t.base} ${(library?.presets.length??0)+1}`,
        action==='add'||action==='update'?captureEffectSettings(effects):undefined);
      if(!mounted.current)return;
      accept(next);
      if(action==='add'){const created=next.presets.at(-1);if(created){setSelected(String(created.id));setName(created.name);}}
      if(action==='remove'){setSelected('');setName('');}
    }catch(cause){if(mounted.current)setError(String(cause));}
    finally{pending.current=false;if(mounted.current)setBusy(false);}
  }
  return <PanelBlock id="effects.presets" title={<>{t.title}<span className="panel-info" title={t.hint} aria-label={t.hint}>ⓘ</span></>}>
    <div className="effect-presets" aria-busy={busy||(!ready&&!error)}>
      <div className="effect-preset-row">
        <select aria-label={t.select} value={target?selected:''} disabled={blocked} onChange={event=>{
          const preset=library?.presets.find(preset=>String(preset.id)===event.target.value);
          setSelected(event.target.value);setName(preset?.name??'');
        }}><option value="">—</option>{library?.presets.map(preset=><option key={preset.id} value={preset.id}>{preset.name}</option>)}</select>
        <button type="button" title={t.apply} aria-label={t.apply} disabled={blocked||!target} onClick={()=>target&&onApply(target.settings)}><Icon name="effects"/></button>
      </div>
      <div className="effect-preset-row">
        <input aria-label={t.name} placeholder={t.name} maxLength={80} value={name} disabled={blocked} onChange={event=>setName(event.target.value)} onKeyDown={event=>{
          if(event.nativeEvent.isComposing)return;
          event.stopPropagation();
          if(event.key==='Enter'&&target&&name.trim()&&name.trim()!==target.name){event.preventDefault();void run('rename');}
        }}/>
        <button type="button" title={(library?.presets.length??0)>=64?t.limit:t.save} aria-label={t.save} disabled={blocked||(library?.presets.length??0)>=64} onClick={()=>void run('add')}><Icon name="plus"/></button>
      </div>
      <div className="effect-preset-actions">
        <button type="button" title={t.update} aria-label={t.update} disabled={blocked||!target} onClick={()=>void run('update')}>↻</button>
        <button type="button" title={t.rename} aria-label={t.rename} disabled={blocked||!target||!name.trim()||name.trim()===target.name} onClick={()=>void run('rename')}>✎</button>
        <button type="button" title={t.remove} aria-label={t.remove} disabled={blocked||!target} onClick={()=>void run('remove')}>⌫</button>
      </div>
      {error&&<div className="effect-preset-error"><span role="alert" title={error}>{t.error}</span><button type="button" title={t.retry} aria-label={t.retry} disabled={busy} onClick={()=>setReload(value=>value+1)}>↻</button></div>}
    </div>
  </PanelBlock>;
}
