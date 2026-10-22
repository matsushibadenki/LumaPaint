import {useEffect, useRef, useState} from 'react';
import {pixelSelectionPreview, type PixelSelectionRequest} from '../bridge';

/** Debounce edits and serialize workers so a slider cannot flood the native queue. */
export function useSelectionPreview(active: boolean, request: PixelSelectionRequest | null, staleMessage: string) {
  const generation = useRef(0);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!active) return;
    const serial = ++generation.current;
    setError('');
    if (!request) {
      setPending(false);
      queue.current = queue.current.catch(() => {}).then(() => {
        if (generation.current === serial) return pixelSelectionPreview(null);
      }).catch(cause => { if (generation.current === serial) setError(String(cause)); });
      return;
    }
    setPending(true);
    const timer = window.setTimeout(() => {
      queue.current = queue.current.catch(() => {}).then(async () => {
        if (generation.current !== serial) return;
        try {
          const accepted = await pixelSelectionPreview(request);
          if (generation.current === serial && !accepted) setError(staleMessage);
        } catch (cause) {
          if (generation.current === serial) setError(String(cause));
        } finally {
          if (generation.current === serial) setPending(false);
        }
      });
    }, 150);
    return () => { window.clearTimeout(timer); ++generation.current; };
  }, [active, request, staleMessage]);
  useEffect(() => () => {
    ++generation.current;
    // Native modal teardown also clears its owner's preview and invalidates workers.
    if (active) void pixelSelectionPreview(null).catch(() => {});
  }, [active]);
  async function clear() {
    ++generation.current;
    await queue.current.catch(() => {});
    await pixelSelectionPreview(null);
  }
  return {pending, error, clear};
}
