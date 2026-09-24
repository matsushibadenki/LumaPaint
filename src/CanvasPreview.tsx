import { useEffect, useRef, useState, type ReactNode } from 'react';
import { onNativeScaleChange, finishCanvasPath, resetCanvasPan, syncCanvas, type Brush, type CanvasTool, type CanvasInfo, type DocumentSnapshot } from './bridge';
import { messages, type Locale, type Theme } from './i18n';
import { workspaceMessages } from './workspace-i18n';
import { textPanelMessages } from './text-panel-i18n';
import { Icon } from './components/Icon';

type Status = 'loading' | 'ready' | 'browser' | 'unsupported' | 'failed' | 'hidden';

export function CanvasPreview({ locale, theme, brush, tool, zoom, visible = true, hasDocument = true, footerAccessory, onZoom, onDocument, onReady }: {
  locale: Locale; theme: Theme; brush: Brush; tool: CanvasTool; zoom: number; onZoom: (zoom: number) => void;
  visible?: boolean; hasDocument?: boolean; footerAccessory?: ReactNode;
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
  const settings = useRef({ zoom, dark, brush, tool, visible });
  const schedule = useRef<() => void>(() => {});
  const retry = () => { setError(''); setStatus('loading'); setAttempt(value => value + 1); };

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(media.matches);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  useEffect(() => {
    settings.current = { zoom, dark, brush, tool, visible };
    schedule.current();
  }, [zoom, dark, brush, tool, visible]);

  useEffect(() => {
    const finishOutside = (event: PointerEvent) => {
      if (event.button !== 0 || settings.current.tool !== 'vectorPen' || slot.current?.contains(event.target as Node)) return;
      // Capture runs before toolbar/panel actions; the bridge serializes with canvas sync.
      void finishCanvasPath().catch(cause => setError(String(cause)));
    };
    document.addEventListener('pointerdown', finishOutside, true);
    return () => document.removeEventListener('pointerdown', finishOutside, true);
  }, []);

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
      const detail = cause instanceof Error ? cause.message : String(cause);
      setError(detail);
      console.error('LumaPaint canvas synchronization failed:', cause);
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
          ...settings.current, visible: settings.current.visible && !document.hidden,
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
  }, [attempt, onDocument]);

  return <section className="canvas-workspace" aria-label={t.canvas}>
    <div ref={slot} className="native-slot" data-document={hasDocument ? 'open' : 'empty'} data-tool={tool} role={hasDocument ? 'img' : undefined} aria-label={hasDocument ? tool === 'text' ? textPanelMessages[locale].hint : tool === 'brush' ? t.canvasNote : tool === 'hand' ? workspaceMessages[locale].handHint : tool === 'zoomIn' || tool === 'zoomOut' ? workspaceMessages[locale].zoomClickHint : tool.startsWith('vector') ? workspaceMessages[locale].vectorHint : `${workspaceMessages[locale][tool]} · ${workspaceMessages[locale].selectionHint}` : undefined}>
      {hasDocument && <div className="paper-preview" aria-hidden="true" />}
      {status === 'failed' ? <div className="canvas-notice canvas-failure" role="alert">
        <p>{t.canvasStatus.failed}</p>
        <pre aria-label={t.errorDetails}>{error}</pre>
        <button type="button" onClick={retry}>{t.retry}</button>
      </div> : hasDocument && status !== 'ready' && <p className="canvas-notice">{t.canvasStatus[status]}</p>}
    </div>
    <div className="canvas-footer">
      <div className="zoom-controls">
        <button type="button" className="icon-button" title={t.zoomOut} aria-label={t.zoomOut} disabled={status !== 'ready' || zoom <= 0.25} onClick={() => onZoom(Math.max(0.25, zoom / 1.25))}><Icon name="minus" /></button>
        <output aria-label={t.zoom}>{Math.round(zoom * 100)}%</output>
        <button type="button" className="icon-button" title={t.zoomIn} aria-label={t.zoomIn} disabled={status !== 'ready' || zoom >= 4} onClick={() => onZoom(Math.min(4, zoom * 1.25))}><Icon name="plus" /></button>
        <button type="button" disabled={status !== 'ready'} onClick={() => { void resetCanvasPan().then(() => onZoom(1)); }}>{t.fit}</button>
      </div>
      {footerAccessory}
      <span className="canvas-state" role="status" title={info ? `${info.backend} · ${info.adapterName} · ${info.physicalWidth} × ${info.physicalHeight} px · ${info.scaleFactor}×` : undefined}>{t.canvasStatus[status]}</span>
      {status === 'failed' && <button type="button" onClick={retry}>{t.retry}</button>}
    </div>
  </section>;
}
