import type { Brush } from '../bridge';
import type { Locale } from '../i18n';
import { brushBlendGroups, brushBlendName, brushSettingsLabels, type BrushBlendMode } from '../brush-settings';
import { PercentInput } from './BrushControls';
import { CompactSlider } from './CompactSlider';

type Props = {
  brush: Brush;
  locale: Locale;
  onChange: (brush: Brush) => void;
  enabled?: boolean;
  compact?: boolean;
  eraser?: boolean;
  alphaLocked?: boolean;
};

export function BrushSettings({ brush, locale, onChange, enabled = true, compact = false, eraser = false, alphaLocked = false }: Props) {
  const t = brushSettingsLabels[locale];
  const percentages = (['opacity', 'flow', 'smoothing', 'alpha'] as const).map(key => {
    const value = brush[key] ?? (key === 'smoothing' ? 0 : 1);
    const change = (value: number) => onChange({ ...brush, [key]: value });
    return <div key={key} className={compact ? 'brush-option' : 'diameter-row'}>
      <span>{t[key]}</span>
      {!compact && <CompactSlider aria-label={t[key]} min="0" max="100" value={Math.round(value * 100)} disabled={!enabled} onChange={event => change(Number(event.target.value) / 100)} />}
      <PercentInput label={t[key]} value={value} disabled={!enabled} onChange={change} />
    </div>;
  });
  const mode = <label className={compact ? 'brush-option' : 'property-row'}>
    <span>{t.blendMode}</span>
    <select aria-label={t.blendMode} disabled={!enabled || eraser} value={eraser ? 'clear' : brush.blendMode ?? 'normal'} onChange={event => onChange({ ...brush, blendMode: event.target.value as BrushBlendMode })}>
      {brushBlendGroups.map((group, i) => <optgroup key={i} label={t.groups[i]}>
        {group.map(value => <option key={value} value={value} disabled={alphaLocked && (value === 'clear' || value === 'behind')}>{brushBlendName(value, locale)}</option>)}
      </optgroup>)}
    </select>
  </label>;
  return compact ? <>{mode}{percentages}</> : <div className="brush-settings">
    {mode}{percentages}
    <p className="muted small">{t.hint}</p>
  </div>;
}
