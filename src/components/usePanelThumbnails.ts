import { useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
type Thumbnails = { layers: [string,string][]; channels: string[] };
const empty: Thumbnails = { layers: [], channels: [] };
export function usePanelThumbnails(documentKey: string, revision: number, target = '') {
  const cacheKey = `${documentKey}:${target}`;
  const [result, setResult] = useState<{key:string; data:Thumbnails}>({key:'',data:empty});
  const [error,setError] = useState('');
  const generation = useRef(0);
  const inFlight = useRef(false);
  const refresh = useRef<() => void>(()=>{});
  useEffect(() => {
    const request = ++generation.current;
    let disposed = false;
    const run = async () => {
      if (disposed || inFlight.current || !documentKey || !isTauri()) return;
      inFlight.current = true;
      try {
        const data = await invoke<Thumbnails>('panel_thumbnails');
        if (!disposed && generation.current === request) { setResult({key:cacheKey,data}); setError(''); }
      } catch (cause) { if (!disposed) setError(String(cause)); }
      finally { inFlight.current = false; if (generation.current !== request) refresh.current(); }
    };
    refresh.current = () => { void run(); };
    const timer = window.setTimeout(refresh.current,300);
    return () => { disposed = true; window.clearTimeout(timer); };
  },[documentKey,revision,cacheKey]);
  return { ...(result.key === cacheKey ? result.data : empty), error };
}
