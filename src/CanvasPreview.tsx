import { useEffect, useRef, useState } from 'react';
import { onNativeScaleChange, syncCanvas, type Brush, type CanvasInfo, type DocumentSnapshot } from './bridge';
import { messages, type Locale, type Theme } from './i18n';
import { Icon } from './components/Icon';

type Status = 'loading' | 'ready' | 'browser' | 'unsupported' | 'failed' | 'hidden';

export function CanvasPreview({ locale, theme, brush, zoom, visible = true, onZoom, onDocument, onReady }: {
  locale: Locale; theme: Theme; brush: Brush; zoom: number; onZoom: (zoom: number) => void;
  visible?: boolean;
  onDocument: (value: DocumentSnapshot) => void; onReady: (ready: boolean) => void;
}) {
  const t = messages[locale];
  const slot = useRef<HTMLDivElement>(null);
  const [systemDark, setSystemDark] = useState(() => matchMedia('(prefers-color-scheme: dark)').matches);
  const [status, setStatus] = useState<Status>('loading');
  const [info, setInfo] = useState<CanvasInfo | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const dark = theme === 'dark' || (theme === 'system' && systemDark);
  const settings = useRef({ zoom, dark, brush, visible });
  const schedule = useRef<() => void>(() => {});

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(media.matches);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  useEffect(() => {
    settings.current = { zoom, dark, brush, visible };
    schedule.current();
  }, [zoom, dark, brush, visible]);

  useEffect(() => onReady(status === 'ready' || status === 'hidden'), [status, onReady]);

  useEffect(() => {
    const element = slot.current;
    if (!element) return;
    let active = true;
    let busy = false;
    let dirty = false;
    let failed = false;
    let frame = 0;
    let unlisten = () => {};

    const fail = (cause: unknown) => {
      if (!active) return;
      failed = true;
      setInfo(null);
      setStatus('failed');
      setError(String(cause));
      void syncCanvas({ x: 0, y: 0, width: 0, height: 0, ...settings.current, visible: false }).catch(() => {});
    };

    const render = async () => {
      frame = 0;
      if (!active || failed) return;
      if (busy) { dirty = true; return; }
      busy = true;
      dirty = false;
      const rect = element.getBoundingClientRect();
      try {
        const result = await syncCanvas({
          x: rect.x, y: rect.y, width: rect.width, height: rect.height,
          ...settings.current, visible: visible && !document.hidden,
        });
        if (!active || failed) return;
        setInfo(result);
        if (result?.document) onDocument(result.document);
        setStatus(result?.status ?? 'browser');
      } catch (cause) {
        fail(cause);
      } finally {
        busy = false;
        if (active && dirty && !failed) requestRender();
      }
    };
    const requestRender = () => {
      if (active && !frame) frame = requestAnimationFrame(() => { void render(); });
    };
    schedule.current = requestRender;
    const observer = new ResizeObserver(requestRender);
    observer.observe(element);
    window.addEventListener('resize', requestRender);
    document.addEventListener('visibilitychange', requestRender);
    onNativeScaleChange(requestRender).then(stop => {
      if (active) unlisten = stop; else stop();
    }).catch(cause => {
      // Treat missing scale-event permissions as an explicit integration failure.
      fail(cause);
    });
    requestRender();
    return () => {
      active = false;
      schedule.current = () => {};
      cancelAnimationFrame(frame);
      observer.disconnect();
      unlisten();
      window.removeEventListener('resize', requestRender);
      document.removeEventListener('visibilitychange', requestRender);
      void syncCanvas({ x: 0, y: 0, width: 0, height: 0, ...settings.current, visible: false }).catch(() => {});
    };
  }, [attempt, onDocument, visible]);

  return <section className="canvas-workspace" aria-label={t.canvas}>
    <div ref={slot} className="native-slot" role="img" aria-label={t.canvasNote}>
      <div className="paper-preview" aria-hidden="true" />
      {status !== 'ready' && <p className="canvas-notice">{t.canvasStatus[status]}</p>}
    </div>
    <div className="canvas-footer">
      <div className="zoom-controls">
        <button type="button" className="icon-button" title={t.zoomOut} aria-label={t.zoomOut} disabled={status !== 'ready' || zoom <= 0.25} onClick={() => onZoom(Math.max(0.25, zoom / 1.25))}><Icon name="minus" /></button>
        <output aria-label={t.zoom}>{Math.round(zoom * 100)}%</output>
        <button type="button" className="icon-button" title={t.zoomIn} aria-label={t.zoomIn} disabled={status !== 'ready' || zoom >= 4} onClick={() => onZoom(Math.min(4, zoom * 1.25))}><Icon name="plus" /></button>
        <button type="button" disabled={status !== 'ready'} onClick={() => onZoom(1)}>{t.fit}</button>
      </div>
      <span className="canvas-state" role="status" title={info ? `${info.backend} · ${info.adapterName} · ${info.physicalWidth} × ${info.physicalHeight} px · ${info.scaleFactor}×` : undefined}>{t.canvasStatus[status]}</span>
      {status === 'failed' && <button onClick={() => { setError(''); setStatus('loading'); setAttempt(value => value + 1); }}>{t.retry}</button>}
    </div>
    {error && <details className="canvas-error"><summary>{t.errorDetails}</summary><pre>{error}</pre></details>}
  </section>;
}
