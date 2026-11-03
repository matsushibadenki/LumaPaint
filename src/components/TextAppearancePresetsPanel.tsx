import {useEffect, useRef, useState} from 'react';
import {textAppearancePresetApi, type TextAppearance, type TextAppearancePresetLibrary} from '../bridge';
import type {Locale} from '../i18n';
import catalog from '../../crates/lumapaint-core/src/text_appearance_presets.json';
import {PanelBlock} from './PanelBlock';
import {Icon} from './Icon';
import {TextAppearancePreview} from './TextAppearancePreview';

export const textPresetLabels = {
  ja:{import:'プリセットを読み込み',export:'すべてのプリセットを書き出し',transferHint:'同じ名前・設定は重複せず追加します。既存のプリセットは保持されます。',title:'マイプリセット',name:'プリセット名',save:'現在の飾り文字を保存',update:'選択したプリセットを現在の設定で更新',rename:'プリセット名を変更',remove:'プリセットを削除',retry:'プリセットを再読み込み',error:'プリセットを読み込み・保存できませんでした',empty:'保存したプリセットはここに表示されます',hint:'塗り・縁・影・強さをアプリ共通で保存。カードをクリックすると選択した文字へ適用します。文字内容・フォント・位置は保持します。更新や削除は適用済みの文字を変更しません。',limit:'64件まで保存できます',sample:'見出し'},
  en:{import:'Import presets',export:'Export all presets',transferHint:'Adds presets without replacing existing entries. Identical names and settings are skipped.',title:'My presets',name:'Preset name',save:'Save current text effects',update:'Update selected preset from current settings',rename:'Rename preset',remove:'Delete preset',retry:'Reload presets',error:'Could not load or save presets',empty:'Saved presets appear here',hint:'Save fill, outlines, shadow and strength across the app. Click a card to apply to selected text, preserving its content, font and position. Updating or deleting a preset leaves existing text unchanged.',limit:'Up to 64 presets',sample:'Title'},
  'zh-CN':{import:'导入预设',export:'导出所有预设',transferHint:'添加预设并保留现有预设；跳过名称和设置均相同的预设。',title:'我的预设',name:'预设名称',save:'保存当前艺术字',update:'使用当前设置更新所选预设',rename:'重命名预设',remove:'删除预设',retry:'重新加载预设',error:'无法加载或保存预设',empty:'保存的预设将显示在此处',hint:'在应用中保存填色、描边、阴影和强度。单击卡片应用到所选文字，保留文字内容、字体和位置。更新或删除预设不会改变已应用的文字。',limit:'最多保存64个预设',sample:'标题'},
};
export function TextAppearancePresetsPanel({locale,appearance,disabled,onApply}:{locale:Locale;appearance:TextAppearance|null|undefined;disabled:boolean;onApply:(style:TextAppearance)=>Promise<void>}) {
  const t=textPresetLabels[locale];
  const [library,setLibrary]=useState<TextAppearancePresetLibrary|null>(null),[selected,setSelected]=useState<number|null>(null),[name,setName]=useState('');
  const [ready,setReady]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[reload,setReload]=useState(0);
  const pending=useRef(false),mounted=useRef(false);
  const accept=(next:TextAppearancePresetLibrary)=>setLibrary(previous=>previous&&previous.revision>next.revision?previous:next);
  useEffect(()=>{
    let live=true,received=false,unlisten:(()=>void)|undefined;mounted.current=true;setReady(false);setError('');
    void(async()=>{try{
      const stop=await textAppearancePresetApi.subscribe(next=>{if(live){received=true;accept(next);setReady(true);setError('');}});
      if(!live){stop();return;}unlisten=stop;
      const next=await textAppearancePresetApi.change('get');if(live){accept(next);setReady(true);}
    }catch(cause){if(live&&!received)setError(String(cause));}})();
    return()=>{live=false;mounted.current=false;unlisten?.();};
  },[reload]);
  const target=library?.presets.find(p=>p.id===selected),blocked=busy||!ready;
  const validName=!!name.trim()&&Array.from(name.trim()).length<=80&&!/[\u0000-\u001f\u007f-\u009f]/.test(name);
  async function run(action:'add'|'update'|'rename'|'remove') {
    if(blocked||pending.current||(!target&&action!=='add')||((action==='add'||action==='update')&&(disabled||!appearance))||((action==='add'||action==='rename')&&!validName))return;
    pending.current=true;setBusy(true);setError('');
    try{
      const next=await textAppearancePresetApi.change(action,target?.id,name.trim(),action==='add'||action==='update'?structuredClone(appearance!):undefined);
      if(!mounted.current)return;accept(next);
      if(action==='add'){const added=next.presets.at(-1);if(added){setSelected(added.id);setName(added.name);}}
      if(action==='remove'){setSelected(null);setName('');}
    }catch(cause){if(mounted.current)setError(String(cause));}
    finally{pending.current=false;if(mounted.current)setBusy(false);}
  }
  async function transfer(exportFile:boolean){
    if(blocked||pending.current)return;
    pending.current=true;setBusy(true);setError('');
    try{const next=await textAppearancePresetApi.transfer(exportFile);if(mounted.current)accept(next);}
    catch(cause){if(mounted.current)setError(String(cause));}
    finally{pending.current=false;if(mounted.current)setBusy(false);}
  }
  async function apply(id:number){
    const entry=library?.presets.find(p=>p.id===id);if(!entry||disabled||blocked||pending.current)return;
    pending.current=true;setBusy(true);setError('');setSelected(entry.id);setName(entry.name);
    try{await onApply({...structuredClone(entry.settings),enabled:true});}catch(cause){if(mounted.current)setError(String(cause));}
    finally{pending.current=false;if(mounted.current)setBusy(false);}
  }
  return <PanelBlock id="headline.user-presets" title={<>{t.title}<span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span></>}>
    <div className="headline-user-presets" aria-busy={busy||(!ready&&!error)}>
      <div className="headline-library-actions"><input aria-label={t.name} placeholder={t.name} value={name} maxLength={160} disabled={blocked} onChange={e=>setName(e.target.value)} onKeyDown={e=>{e.stopPropagation();if(e.key==='Enter'&&!e.nativeEvent.isComposing){e.preventDefault();void run('add');}}}/><button aria-label={t.save} title={(library?.presets.length??0)>=64?t.limit:t.save} disabled={blocked||disabled||!appearance||!validName||(library?.presets.length??0)>=64} onClick={()=>void run('add')}><Icon name="plus"/></button></div>
      <div className="headline-library-manage"><select aria-label={t.title} value={target?.id??''} disabled={blocked} onChange={e=>{const entry=library?.presets.find(p=>p.id===Number(e.target.value));setSelected(entry?.id??null);setName(entry?.name??'');}}><option value="">—</option>{library?.presets.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select><button aria-label={t.update} title={t.update} disabled={blocked||disabled||!appearance||!target} onClick={()=>void run('update')}>↻</button><button aria-label={t.rename} title={t.rename} disabled={blocked||!target||!validName||name.trim()===target.name} onClick={()=>void run('rename')}>✎</button><button aria-label={t.remove} title={t.remove} disabled={blocked||!target} onClick={()=>void run('remove')}>⌫</button></div>
      <div className="headline-library-transfer"><button aria-label={t.import} title={`${t.import} — ${t.transferHint}`} disabled={blocked} onClick={()=>void transfer(false)}>↓</button><button aria-label={t.export} title={t.export} disabled={blocked||!library?.presets.length} onClick={()=>void transfer(true)}>↑</button></div>
      {library?.presets.length===0&&<p className="panel-empty">{t.empty}</p>}
      <div className="headline-presets">{library?.presets.map(p=>{const paint=p.settings.custom??catalog.find(c=>c.id===p.settings.preset)!.paint;return <button key={p.id} className="headline-preset headline-user-preset" aria-label={p.name} title={p.name} aria-pressed={selected===p.id} disabled={disabled||blocked} onClick={()=>void apply(p.id)}><TextAppearancePreview paint={paint} amount={p.settings.amount} locale={locale} sample={t.sample}/><span>{p.name}</span></button>;})}</div>
      {error&&<div className="effect-preset-error"><span role="alert" title={error}>{t.error}</span><button aria-label={t.retry} title={t.retry} disabled={busy} onClick={()=>setReload(v=>v+1)}>↻</button></div>}
    </div>
  </PanelBlock>;
}
