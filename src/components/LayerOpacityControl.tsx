import { useEffect, useRef, useState } from 'react';
import { CompactSlider } from './CompactSlider';

// Keep dragging local; one committed gesture produces one renderer update and undo entry.
export function LayerOpacityControl({ opacity, disabled, label, onCommit }: {
  opacity: number; disabled: boolean; label: string; onCommit: (opacity: number) => void;
}) {
  const percent = Math.round(opacity * 100);
  const [draft, setDraft] = useState(percent);
  const committed = useRef(percent);
  useEffect(() => { if (!disabled) { setDraft(percent); committed.current = percent; } }, [percent, disabled]);
  const commit = (value: number) => {
    if (disabled || value === committed.current) return;
    committed.current = value;
    onCommit(value / 100);
  };
  return <><CompactSlider aria-label={label} disabled={disabled} min="0" max="100" value={draft}
    onChange={event => setDraft(Number(event.currentTarget.value))}
    onPointerDown={event => event.currentTarget.setPointerCapture(event.pointerId)}
    onPointerUp={event => commit(Number(event.currentTarget.value))}
    onPointerCancel={event => { event.currentTarget.value = String(committed.current); setDraft(committed.current); }}
    onKeyUp={event => { if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)) commit(Number(event.currentTarget.value)); }}
    onBlur={event => commit(Number(event.currentTarget.value))} /><output>{draft}%</output></>;
}
