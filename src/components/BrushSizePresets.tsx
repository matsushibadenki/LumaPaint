import { useId, useMemo } from 'react';
import type { Locale } from '../i18n';
import { pixelsPerMeasurement, unitSymbols, useMeasurementUnit } from '../measurement-units';
import { MAX_BRUSH_SIZE } from './BrushControls';
import { PanelBlock } from './PanelBlock';

export const brushSizePresets = [1, 2, 4, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, MAX_BRUSH_SIZE] as const;
export const brushSizePresetLabels = {
  ja: {title:'サイズプリセット', diameter:'ブラシ径', hint:'選ぶと直径だけを変更します。数値はpx、丸は相対サイズです。'},
  en: {title:'Size presets', diameter:'Brush diameter', hint:'Changes diameter only. Values are in px; circles show relative sizes.'},
  'zh-CN': {title:'尺寸预设', diameter:'画笔直径', hint:'仅更改直径。数值单位为px，圆点表示相对大小。'},
};
export function BrushSizePresets({locale, value, resolution, disabled=false, onChange}: {
  locale:Locale; value:number; resolution:number; disabled?:boolean; onChange:(size:number)=>void;
}) {
  const t=brushSizePresetLabels[locale], name=useId(), unit=useMeasurementUnit();
  const format=useMemo(()=>new Intl.NumberFormat(locale,{maximumFractionDigits:3}),[locale]);
  const factor=pixelsPerMeasurement(unit,resolution);
  const choose=(size:number)=>{if(!disabled&&size!==value)onChange(size);};
  return <PanelBlock id="brush.sizes" title={<>{t.title}<span className="brush-size-unit">px</span><span className="panel-info" title={t.hint} aria-label={t.hint}>ⓘ</span></>}>
    <div className="brush-size-presets" role="radiogroup" aria-label={t.title}>
      {brushSizePresets.map(size=>{
        const selected=size===value, diameter=2+16*Math.sqrt(size/MAX_BRUSH_SIZE);
        const label=`${t.diameter}: ${size} px`;
        const title=unit==='pixels'?label:`${label} · ${format.format(size/factor)} ${unitSymbols[unit]}`;
        return <label key={size} className={`brush-size-choice${selected?' selected':''}${disabled?' disabled':''}`} title={title}>
          <input type="radio" name={name} value={size} checked={selected} disabled={disabled} aria-label={label} onChange={()=>choose(size)} onKeyDown={event=>{
            if(event.nativeEvent.isComposing)return;
            if(event.key==='Enter'){event.preventDefault();event.stopPropagation();choose(size);}
          }}/>
          <span className="brush-size-dot" style={{width:diameter,height:diameter}} aria-hidden="true"/>
          <span className="brush-size-value" aria-hidden="true">{size}</span>
        </label>;
      })}
    </div>
  </PanelBlock>;
}
