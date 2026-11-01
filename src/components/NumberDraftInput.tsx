import { useRef, useState } from 'react';
import { SignedNumberInput, parseSignedNumber } from './SignedNumberInput';

/** Numeric settings commit once; unfinished text never changes the model. */
export function NumberDraftInput({ className, label, value, min, max, step = 1, disabled = false, onCommit }: {
  className?: string; label: string; value: number; min: number; max: number; step?: number; disabled?: boolean;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const dirty = useRef(false);
  const rounded = Number(value.toFixed(4));
  const text = draft ?? String(rounded < min || rounded > max ? value : rounded);
  return <SignedNumberInput className={className} label={label} value={text} min={min} max={max} step={step} stepBase={value} stepOrigin={dirty.current ? undefined : value} disabled={disabled}
    onFocus={() => { dirty.current = false; setDraft(text); }}
    onChange={next => { dirty.current = true; setDraft(next); }}
    onBlur={() => {
      const number = parseSignedNumber(text);
      const changed = dirty.current;
      dirty.current = false; setDraft(null);
      if (!disabled && changed && number !== null && number >= min && number <= max && number !== value) onCommit(number);
    }}
    onKeyDown={event => {
      if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); event.currentTarget.blur(); }
      else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); dirty.current = false; setDraft(null); event.currentTarget.blur(); }
    }} />;
}
