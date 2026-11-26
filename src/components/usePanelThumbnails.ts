import { useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';

type Thumbnails = { layers: [string, string][]; masks: [string, string][]; channels: string[] };
type Result = { key: string; data: Thumbnails; error: string };
const empty: Thumbnails = { layers: [], masks: [], channels: [] };

export function usePanelThumbnails(documentKey: string, revision: number, target: string, enabled: boolean, includeChannels: boolean) {
  const cacheKey = `${documentKey}:${target}:${includeChannels}`;
  const [result, setResult] = useState<Result>({ key: '', data: empty, error: '' });
  const inFlight = useRef(false);
  const pending = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!enabled || !documentKey || !isTauri()) return;
    let disposed = false;
    const run = async () => {
      inFlight.current = true;
      try {
        const data = await invoke<Thumbnails>('panel_thumbnails', { includeChannels });
        if (!disposed) setResult({ key: cacheKey, data, error: '' });
      } catch (cause) {
        if (!disposed) setResult(previous => ({
          key: cacheKey,
          data: previous.key === cacheKey ? previous.data : empty,
          error: String(cause),
        }));
      } finally {
        inFlight.current = false;
        const next = pending.current;
        pending.current = null;
        next?.();
      }
    };
    const start = () => { void run(); };
    const timer = window.setTimeout(() => {
      if (inFlight.current) pending.current = start;
      else start();
    }, 300);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      if (pending.current === start) pending.current = null;
    };
  }, [documentKey, revision, cacheKey, enabled, includeChannels]);

  return result.key === cacheKey
    ? { ...result.data, error: result.error }
    : { ...empty, error: '' };
}
