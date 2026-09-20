import { useEffect, useState } from 'react';
import type { Brush } from '../bridge';

export function SizeInput({ value, onChange, label, disabled = false }: { value: number; onChange: (value: number) => void; label: string; disabled?: boolean }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const number = Number(draft);
    const next = Number.isFinite(number) ? Math.min(128, Math.max(1, Math.round(number))) : value;
    setDraft(String(next)); onChange(next);
  };
  return <span className="number-field"><input aria-label={label} type="number" min="1" max="128" value={draft} disabled={disabled} onChange={event => setDraft(event.target.value)} onBlur={commit} onKeyDown={event => { if (event.key === 'Enter') { commit(); event.currentTarget.blur(); } }} /><span>px</span></span>;
}

export function toHex(color: Brush['color']): string { return '#' + color.map(value => value.toString(16).padStart(2, '0')).join(''); }
export function fromHex(value: string): Brush['color'] { return [parseInt(value.slice(1, 3), 16), parseInt(value.slice(3, 5), 16), parseInt(value.slice(5, 7), 16)]; }

export function HexInput({ color, onChange, label, invalid }: { color: Brush['color']; onChange: (color: Brush['color']) => void; label: string; invalid: string }) {
  const hex = toHex(color);
  const [draft, setDraft] = useState(hex);
  const [error, setError] = useState(false);
  useEffect(() => { setDraft(hex); setError(false); }, [hex]);
  const commit = () => {
    const valid = /^#[\da-f]{6}$/i.test(draft);
    setError(!valid);
    if (valid) onChange(fromHex(draft));
  };
  return <div><label className="property-row">{label}<input className="hex-input" aria-invalid={error} aria-describedby={error ? 'color-error' : undefined} value={draft} spellCheck={false} maxLength={7} onChange={event => setDraft(event.target.value)} onBlur={commit} onKeyDown={event => { if (event.key === 'Enter') commit(); }} /></label>{error && <p id="color-error" className="field-error">{invalid}</p>}</div>;
}
