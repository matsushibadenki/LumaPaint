import { useEffect, useRef, useState } from 'react';
import { layerHistogram, type LayerHistogram } from '../bridge';

/** One bounded analysis at a time; old documents/layers cannot install results. */
export function useLayerHistogram(source: string, id: string | undefined, revision: number) {
  const [result, setResult] = useState<{source: string; histogram: LayerHistogram} | null>(null);
  const generation = useRef(0), busy = useRef(false), refresh = useRef<() => void>(() => {});
  useEffect(() => {
    const request = ++generation.current;
    let disposed = false, started = false;
    const run = async () => {
      if (disposed || started || busy.current || !source || !id) return;
      started = true; busy.current = true;
      try {
        const histogram = await layerHistogram(id);
        if (!disposed && generation.current === request) setResult({source, histogram});
      } catch { if (!disposed && generation.current === request) setResult(null); }
      finally { busy.current = false; if (generation.current !== request) refresh.current(); }
    };
    refresh.current = () => { void run(); };
    const timer = window.setTimeout(refresh.current, 300);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [source, id, revision]);
  return result?.source === source ? result.histogram : undefined;
}
