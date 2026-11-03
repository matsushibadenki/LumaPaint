import {useRef, useState} from 'react';
import type {Locale} from '../i18n';
import type {TextSettings, TextAppearance, TextAppearancePaint} from '../bridge';
import catalog from '../../crates/lumapaint-core/src/text_appearance_presets.json';
import {PanelBlock} from './PanelBlock';
import {Icon} from './Icon';
import {TextAppearancePreview} from './TextAppearancePreview';
import {TextAppearancePresetsPanel} from './TextAppearancePresetsPanel';
import {ColorPickerPopover} from './ColorPickerPopover';
import {fromHex, toHex} from './BrushControls';

export const headlineLabels = {
  ja: {title:'飾り文字',appearance:'アピアランス',metal:'メタリック',color:'カラー',outline:'縁取り',toggle:'文字装飾を有効化',remove:'アピアランスを解除',amount:'装飾の強さ',empty:'テキストを選択してください',locked:'選択したテキストは編集できません',hint:'プリセットをクリックして適用。元の文字と書式は保持され、打ち替え後も装飾が追従します。目で一時非表示、×で解除。入力中の文字は確定して適用します。',sample:'見出し',fill:'塗り',gradient:'グラデーション',solid:'単色',angle:'角度',stop:'色',outer:'外側の縁',middle:'中央の縁',inner:'内側の縁',width:'幅',shadow:'影',blur:'ぼかし',opacity:'不透明度',offsetX:'横位置',offsetY:'縦位置',reset:'プリセットの設定に戻す',custom:'調整済み',units:'幅・ぼかし・位置は文字サイズに対する割合です。装飾の強さも反映します。縁の幅0でその縁を非表示、影の不透明度0で影を非表示にします。',presets:'プリセット'},
  en: {title:'Text effects',appearance:'Appearance',metal:'Metallic',color:'Color',outline:'Outline',toggle:'Enable text appearance',remove:'Remove appearance',amount:'Effect strength',empty:'Select a text object',locked:'Selected text cannot be edited',hint:'Click a preset to apply. Original text and formatting stay editable; effects follow replacement text. Eye hides temporarily; × removes. Active typing is committed when applying.',sample:'Title',fill:'Fill',gradient:'Gradient',solid:'Solid',angle:'Angle',stop:'Color',outer:'Outer outline',middle:'Middle outline',inner:'Inner outline',width:'Width',shadow:'Shadow',blur:'Blur',opacity:'Opacity',offsetX:'Offset X',offsetY:'Offset Y',reset:'Reset to preset',custom:'Customized',units:'Width, blur and offsets are percentages of font size, scaled by effect strength. A zero outline width hides that outline; zero shadow opacity hides the shadow.',presets:'Presets'},
  'zh-CN': {title:'艺术字',appearance:'外观',metal:'金属',color:'色彩',outline:'描边',toggle:'启用文字装饰',remove:'移除外观',amount:'装饰强度',empty:'请选择文本对象',locked:'所选文本无法编辑',hint:'单击预设应用。保留原始文字和格式，替换文字后装饰自动更新。眼睛临时隐藏，×移除。输入中的文本将确认后应用。',sample:'标题',fill:'填色',gradient:'渐变',solid:'纯色',angle:'角度',stop:'颜色',outer:'外层描边',middle:'中层描边',inner:'内层描边',width:'宽度',shadow:'阴影',blur:'模糊',opacity:'不透明度',offsetX:'水平偏移',offsetY:'垂直偏移',reset:'恢复预设设置',custom:'已调整',units:'宽度、模糊和偏移均为字号的百分比，并随装饰强度缩放。描边宽度为0时隐藏该层，阴影不透明度为0时隐藏阴影。',presets:'预设'},
};

function NumberField({label,value,min,max,step=.1,disabled,onChange,unit='%'}:{label:string;value:number;min:number;max:number;step?:number;disabled:boolean;onChange:(v:number)=>void;unit?:string}) {
  return <label className="headline-number"><span>{label}</span><span className="headline-input-unit"><input aria-label={label} type="number" min={min} max={max} step={step} key={value} defaultValue={value} disabled={disabled} onBlur={event=>{
    const raw=event.currentTarget.value, next=Number(raw);
    if(!raw.trim()||!Number.isFinite(next)||next<min||next>max){event.currentTarget.value=String(value);return;}
    event.currentTarget.value=String(value);
    if(next!==value)onChange(next);
  }} onKeyDown={event=>{
    if(event.key==='Enter'||event.key==='Escape'){
      event.preventDefault();event.stopPropagation();
      if(event.key==='Escape')event.currentTarget.value=String(value);
      event.currentTarget.blur();
    }
  }}/><span aria-hidden="true">{unit}</span></span></label>;
}
function ColorField({locale,label,value,disabled,onChange}:{locale:Locale;label:string;value:string;disabled:boolean;onChange:(v:string)=>void}) {
  function commit(raw:string){const next=raw.trim().toLowerCase();if(/^#[\da-f]{6}$/.test(next)&&next!==value.toLowerCase())onChange(next);}
  return <div className="headline-color-field"><span>{label}</span><div className="headline-color-inputs"><ColorPickerPopover locale={locale} label={label} color={fromHex(value)} disabled={disabled} onChange={color=>commit(toHex(color))}/><input className="headline-hex" aria-label={`${label} HEX`} key={value} defaultValue={value} disabled={disabled} spellCheck={false} maxLength={7} onBlur={e=>{
    const next=e.currentTarget.value.trim();if(!/^#[\da-f]{6}$/i.test(next)){e.currentTarget.value=value;return;}e.currentTarget.value=value;commit(next);
  }} onKeyDown={e=>{if(e.key==='Enter'||e.key==='Escape'){e.preventDefault();e.stopPropagation();if(e.key==='Escape')e.currentTarget.value=value;e.currentTarget.blur();}}}/></div></div>;
}

export function TextAppearancePanel({locale,settings,enabled,onChange}:{locale:Locale;settings:TextSettings|null;enabled:boolean;onChange:(next:TextSettings)=>Promise<void>}) {
  const t=headlineLabels[locale], [pending,setPending]=useState(false), [error,setError]=useState('');
  const busy=useRef(false), style=settings?.text.appearance, blocked=!settings||!enabled||pending;
  const preset=catalog.find(p=>p.id===style?.preset), paint=style?.custom??preset?.paint;
  async function apply(appearance:TextAppearance|null){
    if(blocked||busy.current||!settings)return;
    busy.current=true;setPending(true);setError('');
    try{await onChange({...settings,text:{...settings.text,appearance}});}
    catch(e){setError(String(e));}
    finally{busy.current=false;setPending(false);}
  }
  function customize(next:TextAppearancePaint){if(style)void apply({...style,custom:next});}
  const disabled=blocked||!style;
  const outlineLabels=[t.outer,t.middle,t.inner];
  return <div className="headline-panel" key={settings?.id??'empty'}>
    <div className="headline-status"><strong>{preset?.names[locale]??t.appearance}{style?.custom&&<span className="headline-custom" title={t.custom} aria-label={t.custom}> •</span>}</strong><span className="timeline-info" title={t.hint} aria-label={t.hint}>ⓘ</span><button aria-label={t.reset} title={t.reset} disabled={disabled||!style?.custom} onClick={()=>void apply({...style!,custom:null})}>↺</button><button aria-label={t.toggle} title={t.toggle} aria-pressed={!!style?.enabled} disabled={disabled} onClick={()=>void apply({...style!,enabled:!style!.enabled})}><Icon name={style?.enabled?'eye':'eyeOff'}/></button><button aria-label={t.remove} title={t.remove} disabled={disabled} onClick={()=>void apply(null)}>×</button></div>
    {!settings&&<p className="panel-empty">{t.empty}</p>}{settings&&!enabled&&<p className="panel-empty">{t.locked}</p>}
    <NumberField label={t.amount} value={style?.amount??1} min={.25} max={2} disabled={disabled} unit="×" onChange={amount=>void apply({...style!,amount})}/>
    <TextAppearancePresetsPanel locale={locale} appearance={style} disabled={blocked} onApply={apply}/>
    {paint&&<div className="headline-editor">
      <PanelBlock id="headline.fill" title={t.fill}>
        <div className="headline-fill-mode"><select aria-label={t.fill} value={paint.fill.gradient?'gradient':'solid'} disabled={disabled} onChange={e=>customize({...paint,fill:{...paint.fill,gradient:e.target.value==='gradient'}})}><option value="gradient">{t.gradient}</option><option value="solid">{t.solid}</option></select><span className="timeline-info" title={t.units} aria-label={t.units}>ⓘ</span></div>
        <div className="headline-gradient-strip" aria-hidden="true" style={{background:paint.fill.gradient?`linear-gradient(${180-paint.fill.angle}deg,${paint.fill.colors.join(',')})`:paint.fill.colors[0]}}/>
        <div className="headline-fill-colors">{paint.fill.colors.slice(0,paint.fill.gradient?5:1).map((color,index)=><ColorField locale={locale} key={index} label={`${t.stop} ${index+1}`} value={color} disabled={disabled} onChange={value=>customize({...paint,fill:{...paint.fill,colors:paint.fill.colors.map((c,i)=>i===index?value:c)}})}/>)}</div>
        {paint.fill.gradient&&<NumberField label={t.angle} value={paint.fill.angle} min={0} max={360} step={1} unit="°" disabled={disabled} onChange={angle=>customize({...paint,fill:{...paint.fill,angle}})}/>}
      </PanelBlock>
      <PanelBlock id="headline.outlines" title={t.outline}>
        {paint.outlines.map((outline,index)=><div className="headline-outline-row" key={index}><ColorField locale={locale} label={outlineLabels[index]} value={outline.color} disabled={disabled} onChange={color=>customize({...paint,outlines:paint.outlines.map((o,i)=>i===index?{...o,color}:o)})}/><NumberField label={`${outlineLabels[index]} ${t.width}`} value={outline.width} min={0} max={50} disabled={disabled} onChange={width=>customize({...paint,outlines:paint.outlines.map((o,i)=>i===index?{...o,width}:o)})}/></div>)}
      </PanelBlock>
      <PanelBlock id="headline.shadow" title={t.shadow}>
        <ColorField locale={locale} label={`${t.shadow} ${t.stop}`} value={paint.shadow.color} disabled={disabled} onChange={color=>customize({...paint,shadow:{...paint.shadow,color}})}/>
        <div className="headline-shadow-grid">{(['width','blur','offsetX','offsetY','opacity'] as const).map(field=><NumberField key={field} label={t[field]} value={paint.shadow[field]} min={field.startsWith('offset')?-100:0} max={field==='blur'?50:100} disabled={disabled} onChange={value=>customize({...paint,shadow:{...paint.shadow,[field]:value}})}/>)}</div>
      </PanelBlock>
    </div>}
    <PanelBlock id="headline.presets" title={t.presets}>
      {(['metal','color','outline'] as const).map(group=><PanelBlock key={group} id={`headline.gallery.${group}`} title={t[group]}><div className="headline-presets">{catalog.filter(p=>p.group===group).map(p=><button key={p.id} className="headline-preset" aria-label={p.names[locale]} title={p.names[locale]} aria-pressed={style?.preset===p.id} disabled={blocked} onClick={()=>void apply({preset:p.id,enabled:true,amount:style?.amount??1})}><TextAppearancePreview paint={p.paint} locale={locale} sample={t.sample}/><span>{p.names[locale]}</span></button>)}</div></PanelBlock>)}
    </PanelBlock>
    {error&&<p role="alert">{error}</p>}
  </div>;
}
