import { NumberDraftInput } from './NumberDraftInput';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import { useEffect, useState } from 'react';
import type { Brush } from '../bridge';

export const MAX_BRUSH_SIZE = 512;

export function PercentInput({ value, onChange, label, disabled = false }: { value: number; onChange: (value: number) => void; label: string; disabled?: boolean }) {
  return <span className="number-field percent-field"><NumberDraftInput label={label} value={value * 100} min={0} max={100} disabled={disabled} onCommit={percent => onChange(percent / 100)} /><span>%</span></span>;
}

export function SizeInput({ value, onChange, label, disabled = false, resolution=72 }: { value: number; onChange: (value: number) => void; label: string; disabled?: boolean; resolution?: number }) {
  const unit=useMeasurementUnit(), factor=pixelsPerMeasurement(unit,resolution);
  return <span className="number-field"><NumberDraftInput key={`${unit}:${factor}`} label={label} value={value / factor} min={1 / factor} max={MAX_BRUSH_SIZE / factor} step={1 / factor} disabled={disabled} onCommit={number => onChange(Math.max(1, Math.min(MAX_BRUSH_SIZE, number * factor)))} /><span>{unitSymbols[unit]}</span></span>;
}

export function toHex(color: Brush['color']): string { return '#' + color.map(value => value.toString(16).padStart(2, '0')).join(''); }
export function fromHex(value: string): Brush['color'] { return [parseInt(value.slice(1, 3), 16), parseInt(value.slice(3, 5), 16), parseInt(value.slice(5, 7), 16)]; }

export function HexInput({ color, onChange, label, invalid, noColor = false, onNone, noneLabel }: { color: Brush['color']; onChange: (color: Brush['color']) => void; label: string; invalid: string; noColor?: boolean; onNone?: () => void; noneLabel?: string }) {
  const hex = toHex(color);
  const [draft, setDraft] = useState(hex);
  const [error, setError] = useState(false);
  useEffect(() => { setDraft(hex); setError(false); }, [hex]);
  const commit = () => {
    const valid = /^#[\da-f]{6}$/i.test(draft);
    setError(!valid);
    if (valid && draft.toLowerCase() !== hex.toLowerCase()) onChange(fromHex(draft));
  };
  return <div>{onNone && <button type="button" aria-pressed={noColor} onClick={onNone}><span className="paint-none" style={{display:'inline-block',width:16,height:16,marginRight:6}} aria-hidden="true"/>{noneLabel}</button>}<label className="property-row">{label}<input className="hex-input" aria-invalid={error} aria-describedby={error ? 'color-error' : undefined} value={draft} spellCheck={false} maxLength={7} onChange={event => setDraft(event.target.value)} onBlur={commit} onKeyDown={event => { if (event.key === 'Enter') commit(); }} /></label>{error && <p id="color-error" className="field-error">{invalid}</p>}</div>;
}
