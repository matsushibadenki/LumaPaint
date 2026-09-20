import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { isTauri } from '@tauri-apps/api/core';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import { Menu } from '@tauri-apps/api/menu';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Locale } from '../i18n';
import { settingsMessages } from '../settings-i18n';

function AppMark() {
  return <svg viewBox="0 0 512 512" aria-hidden="true"><rect width="512" height="512" rx="112" fill="#233529" /><path d="M160 128h56v200h140v56H160z" fill="#e5efd8" /></svg>;
}

export function AppMenu({ locale, onSettings, onError }: { locale: Locale; onSettings: () => void; onError: (error: string) => void }) {
  const t = settingsMessages[locale];
  const native = isTauri();
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const busy = useRef(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });

  const show = async () => {
    if (busy.current) return;
    if (!native) { setOpen(value => !value); return; }
    busy.current = true; setOpen(true);
    let menu: Menu | undefined;
    try {
      menu = await Menu.new({ items: [
        { text: 'LumaPaint 0.1.0', enabled: false },
        { item: 'Separator' as const },
        { text: t.settings, accelerator: 'CmdOrCtrl+,', action: onSettings },
      ] });
      const rect = trigger.current!.getBoundingClientRect();
      const window = getCurrentWindow();
      const [size, scale] = await Promise.all([window.innerSize(), window.scaleFactor()]);
      const topInset = Math.max(0, size.height / scale - globalThis.innerHeight);
      await menu.popup(new LogicalPosition(rect.left, rect.bottom + topInset), window);
    } catch (cause) { onError(String(cause)); }
    finally {
      setOpen(false); busy.current = false;
      if (menu) await menu.close().catch(cause => onError(String(cause)));
    }
  };

  useLayoutEffect(() => {
    if (!open || native) return;
    const rect = trigger.current!.getBoundingClientRect();
    const panel = popup.current!;
    setPosition({ left: Math.max(8, Math.min(rect.left, innerWidth - panel.offsetWidth - 8)), top: rect.bottom + 2 });
    panel.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
  }, [open, native]);

  useEffect(() => {
    if (!open || native) return;
    const dismiss = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!trigger.current?.contains(target) && !popup.current?.contains(target)) setOpen(false);
    };
    document.addEventListener('pointerdown', dismiss);
    return () => document.removeEventListener('pointerdown', dismiss);
  }, [open, native]);

  return <>
    <button ref={trigger} className="app-menu-trigger" type="button" aria-label={t.appMenu} title={t.appMenu} aria-haspopup="menu" aria-expanded={open} onClick={() => void show()} onKeyDown={event => {
      if (event.key === 'ArrowDown') { event.preventDefault(); void show(); }
      if (event.key === 'Escape') setOpen(false);
    }}><AppMark /></button>
    {!native && open && createPortal(<div ref={popup} className="app-dropdown" role="menu" aria-label={t.appMenu} style={position} onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); setOpen(false); trigger.current?.focus(); }
    }}>
      <p>{t.about}<small>0.1.0</small></p>
      <div role="separator" />
      <button role="menuitem" onClick={() => { setOpen(false); onSettings(); }}>{t.settings}<kbd>⌘,</kbd></button>
    </div>, document.body)}
  </>;
}
