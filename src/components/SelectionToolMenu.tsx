import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Locale } from '../i18n';
import type { CanvasTool } from '../bridge';
import { workspaceMessages } from '../workspace-i18n';
import { Icon } from './Icon';
import { toolMenuImages } from './toolMenuImages';

export type SelectionTool = Extract<CanvasTool, 'rectangle' | 'ellipse'>;
const choices = [{ tool: 'rectangle', shortcut: 'M' }, { tool: 'ellipse', shortcut: 'Shift+M' }] as const;

export function SelectionToolMenu({ locale, selected, active, enabled, onSelect, onError }: {
  locale: Locale; selected: SelectionTool; active: boolean; enabled: boolean;
  onSelect: (tool: SelectionTool) => void; onError: (message: string) => void;
}) {
  const t = workspaceMessages[locale];
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
      const result = await invoke<{ supported: boolean; tool: SelectionTool | null }>('selection_tool_menu', {
        request: { x: rect.right + 4, y: rect.top + rect.height / 2, selected,
        labels: choices.map(({ tool, shortcut }) => `${t[tool]} (${shortcut})`),
        images: await toolMenuImages(trigger.current!), },
      });
      if (result.supported) { setOpen(false); if (result.tool) onSelect(result.tool); }
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
    panel.querySelector<HTMLButtonElement>('[aria-checked="true"]')?.focus();
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
    if (event.key.toLowerCase() === 'm' && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault(); close(true); onSelect(event.shiftKey ? 'ellipse' : 'rectangle');
    }
    if (['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const items = [...popup.current!.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')];
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
        : (current + (event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
    }
  };

  return <>
    <button ref={trigger} type="button" className={`tool-button selection-tool-trigger${active ? ' selected' : ''}`}
      aria-label={`${t.selectionTools} · ${t[selected]}`} title={`${t.selectionTools} · ${t[selected]}`}
      aria-pressed={active} aria-haspopup="menu" aria-expanded={open} aria-controls={!native && open ? id : undefined} disabled={!enabled}
      onClick={() => void show()} onContextMenu={event => { event.preventDefault(); if (!open) void show(); }}
      onKeyDown={event => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowRight') { event.preventDefault(); event.stopPropagation(); if (!open) void show(); }
        if (event.key === 'Escape' && open) { event.preventDefault(); event.stopPropagation(); close(true); }
      }}><Icon name={selected} /><span className="tool-menu-corner" aria-hidden="true" /></button>
    {!native && open && enabled && createPortal(<div ref={popup} id={id} role="menu" aria-label={t.selectionTools}
      aria-orientation="horizontal" className="selection-tool-dropdown" style={position} onKeyDown={menuKey}>
      {choices.map(({ tool, shortcut }) => <button type="button" key={tool} role="menuitemradio" aria-checked={selected === tool} tabIndex={-1}
        className={`tool-button selection-tool-menu-item${selected === tool ? ' selected' : ''}`}
        aria-label={t[tool]} title={`${t[tool]} (${shortcut})`} aria-keyshortcuts={shortcut}
        onClick={() => { close(true); onSelect(tool); }}>
        <Icon name={tool} />
      </button>)}
    </div>, document.body)}
  </>;
}
