import { useEffect, useId, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { Brush } from '../bridge';
import type { Locale } from '../i18n';
import { toHex } from './BrushControls';
import { ColorPanel, colorPanelLabels, type ColorTarget } from './ColorPanel';

const labels = {
  ja: { close: 'カラージェネレーターを閉じる' },
  en: { close: 'Close color generator' },
  'zh-CN': { close: '关闭颜色生成器' },
};

export const colorPickerVisibilityEvent = 'lumapaint-color-picker-visibility';
export type ColorPickerOcclusion = { left: number; top: number; right: number; bottom: number };

export function ColorPickerPopover({ locale, color, disabled = false, label, target = 'foreground', onChange, onOpen, children }: {
  locale: Locale;
  color: Brush['color'];
  disabled?: boolean;
  label: string;
  target?: ColorTarget;
  onChange: (color: Brush['color']) => void;
  onOpen?: () => void;
  children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 16, top: 48 });
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointerId: number; offsetX: number; offsetY: number } | null>(null);
  const id = useId();

  useLayoutEffect(() => {
    if (!open || !trigger.current) return;
    const place = () => {
      const rect = trigger.current?.getBoundingClientRect();
      if (!rect) return;
      const width = Math.min(320, window.innerWidth - 32);
      const left = Math.max(16, Math.min(rect.left, window.innerWidth - width - 16));
      const below = rect.bottom + 8;
      const height = Math.min(560, window.innerHeight - 32);
      const top = below + height <= window.innerHeight - 16 ? below : Math.max(16, rect.top - height - 8);
      setPosition({ left, top });
    };
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => { window.removeEventListener('resize', place); window.removeEventListener('scroll', place, true); };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!popup.current?.contains(target) && !trigger.current?.contains(target)) setOpen(false);
    };
    const closeEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { setOpen(false); trigger.current?.focus(); }
    };
    document.addEventListener('pointerdown', closeOutside);
    document.addEventListener('keydown', closeEscape);
    return () => { document.removeEventListener('pointerdown', closeOutside); document.removeEventListener('keydown', closeEscape); };
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !popup.current) return;
    const rect = popup.current.getBoundingClientRect();
    const occlusion: ColorPickerOcclusion = { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
    window.dispatchEvent(new CustomEvent(colorPickerVisibilityEvent, { detail: { open: true, rect: occlusion } }));
  }, [open, position]);

  useEffect(() => {
    if (!open) return;
    return () => { window.dispatchEvent(new CustomEvent(colorPickerVisibilityEvent, { detail: { open: false } })); };
  }, [open]);

  const beginDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest('button')) return;
    const rect = popup.current?.getBoundingClientRect();
    if (!rect) return;
    drag.current = { pointerId: event.pointerId, offsetX: event.clientX - rect.left, offsetY: event.clientY - rect.top };
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
  };
  const moveDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (drag.current?.pointerId !== event.pointerId || !popup.current) return;
    const width = popup.current.offsetWidth, height = popup.current.offsetHeight;
    setPosition({
      left: Math.max(16, Math.min(event.clientX - drag.current.offsetX, window.innerWidth - width - 16)),
      top: Math.max(16, Math.min(event.clientY - drag.current.offsetY, window.innerHeight - height - 16)),
    });
  };
  const endDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    drag.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  };

  return <span className="color-picker-anchor">
    <button ref={trigger} type="button" className="color-picker-trigger" disabled={disabled}
      aria-label={label} title={`${label}: ${toHex(color).toUpperCase()}`} aria-expanded={open} aria-controls={id}
      style={{ backgroundColor: toHex(color) }}
      onClick={() => setOpen(value => {
        const next = !value;
        if (next) onOpen?.();
        return next;
      })}>
      <span className="color-picker-chip" style={{ backgroundColor: toHex(color) }} aria-hidden="true" />
      {children}
    </button>
    {open && createPortal(<div ref={popup} id={id} role="dialog" aria-label={`${label} · ${colorPanelLabels[locale].color}`}
      className="color-picker-popover" style={{ left: position.left, top: position.top }}>
      <header onPointerDown={beginDrag} onPointerMove={moveDrag} onPointerUp={endDrag} onPointerCancel={endDrag}><strong>{label}</strong><button type="button" aria-label={labels[locale].close} onClick={() => { setOpen(false); trigger.current?.focus(); }}>×</button></header>
      <ColorPanel locale={locale} color={color} backgroundColor={color} activeColor={target}
        onSelectColor={() => {}} onChange={onChange} onBackgroundChange={onChange} onSwap={() => {}} singleColor />
    </div>, document.body)}
  </span>;
}
