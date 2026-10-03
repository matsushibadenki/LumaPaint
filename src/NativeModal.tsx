import { useEffect, useRef, type ReactNode } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Locale, Theme } from './i18n';
export type ModalKind = 'settings' | 'newDocument' | 'colorSettings' | 'transform' | 'directControls' | 'importImage' | 'toolSettings';
export function NativeModal({ kind, locale, theme, action, onClose, onError, children }: {
  kind: ModalKind; locale: Locale; theme: Theme; action?: string; onClose: () => void; onError: (error: string) => void; children: ReactNode;
}) {
  const request = useRef({ requestId: crypto.randomUUID(), kind, locale, theme, action: action ?? null });
  const callbacks = useRef({ onClose, onError }); callbacks.current = { onClose, onError };
  const generation = useRef(0);
  useEffect(() => {
    if (!isTauri()) return;
    const serial = ++generation.current;
    let disposed = false;
    const listener = getCurrentWebviewWindow().listen<string>('modal-closed', event => {
      if (!disposed && event.payload === request.current.requestId) callbacks.current.onClose();
    });
    void listener.then(() => { if (!disposed) return invoke('open_modal_window', { request: request.current }); }).catch(error => {
      if (!disposed) { callbacks.current.onError(String(error)); callbacks.current.onClose(); }
    });
    return () => {
      disposed = true; void listener.then(unlisten => unlisten()).catch(() => {});
      queueMicrotask(() => { if (generation.current === serial) void invoke('close_modal_window', { requestId: request.current.requestId }).catch(() => {}); });
    };
  }, []);
  return isTauri() ? null : children;
}
