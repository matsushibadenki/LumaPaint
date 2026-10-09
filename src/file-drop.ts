import { useEffect, useRef } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export type FileDrop = { paths: string[]; targetId: number | null };

/** Physical Tauri coordinates must be converted before hitting CSS rectangles. */
export function fileDropTarget(x: number, y: number, activeId: number | null): number | null | undefined {
  if (document.querySelector('[role="dialog"]')) return undefined;
  const element = document.elementFromPoint(x, y);
  if (!element) return undefined;
  if (element.closest('.document-tabs') && !element.closest('.document-tab-shell')) return null;
  const canvas = element.closest('.native-slot');
  if (canvas?.getAttribute('data-file-drop-enabled') === 'true') return activeId;
  if (element.closest('.canvas-stage')?.getAttribute('data-file-drop-enabled') === 'true') return null;
  return undefined;
}

export function useFileDrop(activeId: number | null, onDrop: (drop: FileDrop) => void, onError: (error: string) => void) {
  const current = useRef({ activeId, onDrop, onError });
  useEffect(() => { current.current = { activeId, onDrop, onError }; }, [activeId, onDrop, onError]);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => { if (live) stops.push(stop); else stop(); };
    const view = getCurrentWebviewWindow();
    // The NSView canvas receives its own AppKit drop, above the WebView.
    void view.listen<FileDrop>('native-file-drop', ({ payload }) => {
      if (live && !document.querySelector('[role="dialog"]')) current.current.onDrop(payload);
    }).then(keep).catch(error => { if (live) current.current.onError(String(error)); });
    void view.onDragDropEvent(event => {
      if (!live || event.payload.type !== 'drop') return;
      const { paths, position } = event.payload;
      const targetId = fileDropTarget(position.x / window.devicePixelRatio, position.y / window.devicePixelRatio, current.current.activeId);
      if (targetId !== undefined) current.current.onDrop({ paths, targetId });
    }).then(keep).catch(error => { if (live) current.current.onError(String(error)); });
    return () => { live = false; stops.forEach(stop => stop()); };
  }, []);
}
