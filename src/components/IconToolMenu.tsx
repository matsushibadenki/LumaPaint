import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { Icon } from './Icon';
import { toolMenuImages } from './toolMenuImages';

export interface IconToolChoice { id: string; label: string; icon: keyof typeof import('./Icon').iconPaths; shortcut?: string; enabled?: boolean }
export function IconToolMenu({ label, choices, selected, active, enabled, selectionShortcuts = false, onSelect, onError }: {
  label: string; choices: readonly [IconToolChoice, IconToolChoice, ...IconToolChoice[]]; selected: string; active: boolean; enabled: boolean; selectionShortcuts?: boolean;
  onSelect: (tool: string) => void; onError: (message: string) => void;
}) {
  const selectedIndex = Math.max(0, choices.findIndex(choice => choice.id === selected));
  const choice = choices[selectedIndex];
  const [native, setNative] = useState(false);
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const busy = useRef(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const close = (focus = false) => { setOpen(false); if (focus) trigger.current?.focus(); };

  const show = async () => {
    if (!enabled || busy.current) return;
    if (open && !native) { setOpen(false); return; }
    if (!isTauri()) { setOpen(value => !value); return; }
    busy.current = true;
    setNative(true);
    setOpen(true);
    try {
      const rect = trigger.current!.getBoundingClientRect();
      // AppKit hosts the horizontal picker above Metal; other platforms use the DOM picker.
      const result = await invoke<{ supported: boolean; index: number | null }>('icon_tool_menu', {
        request: { x: rect.right + 4, y: rect.top + rect.height / 2, selected: selectedIndex, enabled: choices.map(item => item.enabled !== false), selectionShortcuts,
        labels: choices.map(item => item.shortcut ? `${item.label} (${item.shortcut})` : item.label),
        images: await toolMenuImages(trigger.current!, choices.map(item => item.icon), choices.map(item => item.enabled !== false)), },
      });
      if (result.supported) { setOpen(false); if (result.index !== null && choices[result.index]?.enabled !== false && choices[result.index]) onSelect(choices[result.index].id); }
      else setNative(false);
    } catch (cause) { setOpen(false); onError(String(cause)); }
    finally { busy.current = false; }
  };

  useLayoutEffect(() => {
    if (!open || native || !enabled) return;
    const rect = trigger.current!.getBoundingClientRect();
    const panel = popup.current!;
    setPosition({
      left: Math.max(8, Math.min(rect.right + 4, innerWidth - panel.offsetWidth - 8)),
      top: Math.max(8, Math.min(rect.top + (rect.height - panel.offsetHeight) / 2, innerHeight - panel.offsetHeight - 8)),
    });
    (panel.querySelector<HTMLButtonElement>('[aria-checked="true"]:not(:disabled)') ?? panel.querySelector<HTMLButtonElement>('button:not(:disabled)'))?.focus();
  }, [open, native, enabled, selected]);

  useEffect(() => { if (!enabled) setOpen(false); }, [enabled]);
  useEffect(() => {
    if (!open || native) return;
    const dismiss = (event: PointerEvent | FocusEvent) => {
      const target = event.target as Node;
      if (!trigger.current?.contains(target) && !popup.current?.contains(target)) setOpen(false);
    };
    const resize = () => setOpen(false);
    document.addEventListener('pointerdown', dismiss);
    document.addEventListener('focusin', dismiss);
    window.addEventListener('resize', resize);
    return () => {
      document.removeEventListener('pointerdown', dismiss);
      document.removeEventListener('focusin', dismiss);
      window.removeEventListener('resize', resize);
    };
  }, [open, native]);

  const menuKey = (event: KeyboardEvent) => {
    event.stopPropagation();
    if (event.key === 'Escape') { event.preventDefault(); close(true); }
    if (event.key === 'Tab') { close(true); }
    if (selectionShortcuts && event.key.toLowerCase() === 'm' && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault(); close(true); onSelect(choices[event.shiftKey ? 1 : 0].id);
    }
    if (['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const items = [...popup.current!.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled)')];
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
        : (current + (event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
    }
  };

  return <>
    <button ref={trigger} type="button" className={`tool-button selection-tool-trigger${active ? ' selected' : ''}`}
      aria-label={`${label} · ${choice.label}`} title={`${label} · ${choice.label}`}
      aria-pressed={active} aria-haspopup="menu" aria-expanded={open} aria-controls={!native && open ? id : undefined} disabled={!enabled}
      onClick={() => void show()} onContextMenu={event => { event.preventDefault(); if (!open) void show(); }}
      onKeyDown={event => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowRight') { event.preventDefault(); event.stopPropagation(); if (!open) void show(); }
        if (event.key === 'Escape' && open) { event.preventDefault(); event.stopPropagation(); close(true); }
      }}><Icon name={choice.icon} /><span className="tool-menu-corner" aria-hidden="true" /></button>
    {!native && open && enabled && createPortal(<div ref={popup} id={id} role="menu" aria-label={label}
      aria-orientation="horizontal" className="selection-tool-dropdown" style={position} onKeyDown={menuKey}>
      {choices.map(item => <button type="button" key={item.id} role="menuitemradio" aria-checked={selected === item.id} tabIndex={-1} disabled={item.enabled === false}
        className={`tool-button selection-tool-menu-item${selected === item.id ? ' selected' : ''}`}
        aria-label={item.label} title={item.shortcut ? `${item.label} (${item.shortcut})` : item.label} aria-keyshortcuts={item.shortcut}
        onClick={() => { close(true); onSelect(item.id); }}>
        <Icon name={item.icon} />
      </button>)}
    </div>, document.body)}
  </>;
}
