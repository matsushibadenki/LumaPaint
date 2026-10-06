import {isRetouch,retouchLabels} from './components/RetouchControls';
import {isPathSelection,selectionPathLabels} from './components/SelectionPathControls';
import { paintBucketLabels } from './components/PaintBucketControls';
import { cloneStampLabels } from './components/CloneStampControls';
import { Rulers } from './components/Rulers';
import { MIN_ZOOM, MAX_ZOOM, ZOOM_PERCENTAGES, stepZoom, zoomLabel } from './zoom';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { onNativeScaleChange, finishCanvasPath, resetCanvasPan, syncCanvas, type DisplayChannel, type Brush, type CanvasTool, type CanvasInfo, type DocumentSnapshot } from './bridge';
import { messages, type Locale, type Theme } from './i18n';
import { workspaceMessages } from './workspace-i18n';
import { textPanelMessages } from './text-panel-i18n';
import { Icon } from './components/Icon';
import type { ColorPickerOcclusion } from './components/ColorPickerPopover';

import { usePasteboardColor } from './pasteboard-preference';

type Status = 'loading' | 'ready' | 'browser' | 'unsupported' | 'failed' | 'hidden';

export function CanvasPreview({ locale, theme, brush, tool, zoom, zoomCommand, channel = 0, visible = true, occlusion = null, hasDocument = true, resolution=72, verticalResolution=resolution, footerAccessory, onZoom, onDisplayZoom, onDocument, onReady }: {
  locale: Locale; theme: Theme; brush: Brush; tool: CanvasTool; zoom: number; onZoom: (zoom: number) => void;
  zoomCommand: { zoom: number; revision: number };
  onDisplayZoom: (zoom: number) => void;
  channel?: DisplayChannel; visible?: boolean; occlusion?: ColorPickerOcclusion | null; hasDocument?: boolean; resolution?:number; verticalResolution?:number; footerAccessory?: ReactNode;
  onDocument: (value: DocumentSnapshot) => void; onReady: (ready: boolean) => void;
}) {
  const [pasteboardColor] = usePasteboardColor();
  const t = messages[locale];
  const selectedZoom = ZOOM_PERCENTAGES.find(percent => Math.abs(percent / 100 - zoom) < 0.000001 * Math.max(1, zoom));
  const slot = useRef<HTMLDivElement>(null);
  const [systemDark, setSystemDark] = useState(() => matchMedia('(prefers-color-scheme: dark)').matches);
  const [status, setStatus] = useState<Status>('loading');
  const [info, setInfo] = useState<CanvasInfo | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const dark = theme === 'dark' || (theme === 'system' && systemDark);
  const settings = useRef({ zoomCommand, dark, brush, tool, visible, channel, occlusion, pasteboardColor });
  const schedule = useRef<() => void>(() => {});
  const retry = () => { setError(''); setStatus('loading'); setAttempt(value => value + 1); };

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(media.matches);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  useEffect(() => {
    settings.current = { zoomCommand, dark, brush, tool, visible, channel, occlusion, pasteboardColor };
    schedule.current();
  }, [zoomCommand, dark, brush, tool, visible, channel, occlusion, pasteboardColor]);

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
    const requestSettings = () => ({
      zoom: settings.current.zoomCommand.zoom, absoluteZoom: true, zoomRevision: settings.current.zoomCommand.revision,
      pasteboardColor: settings.current.pasteboardColor, dark: settings.current.dark, brush: settings.current.brush,
      tool: settings.current.tool, visible: settings.current.visible, channel: settings.current.channel,
    });

    const fail = (cause: unknown) => {
      if (!active) return;
      failed = true;
      setInfo(null);
      setStatus('failed');
      const detail = cause instanceof Error ? cause.message : String(cause);
      setError(detail);
      console.error('LumaPaint canvas synchronization failed:', cause);
      void syncCanvas({ x: 0, y: 0, width: 0, height: 0, ...requestSettings(), visible: false }).catch(() => {});
    };

    const render = async () => {
      frame = 0;
      if (!active || failed) return;
      if (busy) { dirty = true; return; }
      busy = true;
      dirty = false;
      const rect = element.getBoundingClientRect();
      const overlay = settings.current.occlusion;
      try {
        const result = await syncCanvas({
          x: rect.x, y: rect.y, width: rect.width, height: rect.height,
          ...requestSettings(), visible: settings.current.visible && !document.hidden,
          overlays: Array.from(document.querySelectorAll('[data-floating-panel]')).slice(0,10).map(node=>{const r=node.getBoundingClientRect();return [r.left-rect.x,r.top-rect.y,r.right-rect.x,r.bottom-rect.y] as [number,number,number,number];}),
          overlay: overlay ? [overlay.left - rect.x, overlay.top - rect.y, overlay.right - rect.x, overlay.bottom - rect.y] : null,
        });
        if (!active || failed) return;
        setInfo(result);
        if (result?.zoom != null) onDisplayZoom(result.zoom);
        if (result?.document) onDocument(result.document);
        setStatus(result?.status ?? 'browser');
      } catch (cause) {
        fail(cause);
      } finally {
        busy = false;
        // A setting may have changed while the failed request was in flight.
        // Do not discard the queued valid brush (for example, envelope disabled).
        if (active && dirty) {
          failed = false;
          requestRender();
        }
      }
    };
    const requestRender = () => {
      if (active && !frame) frame = requestAnimationFrame(() => { void render(); });
    };
    schedule.current = () => {
      // A validation/render error must not permanently disconnect later settings.
      // Retry on an explicit settings change, rather than continuously retrying.
      failed = false;
      requestRender();
    };
    const observer = new ResizeObserver(requestRender);
    observer.observe(element);
    window.addEventListener('resize', requestRender);
    window.addEventListener('panel-layout-change',requestRender);
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
      window.removeEventListener('panel-layout-change',requestRender);
      document.removeEventListener('visibilitychange', requestRender);
      void syncCanvas({ x: 0, y: 0, width: 0, height: 0, ...requestSettings(), visible: false }).catch(() => {});
    };
  }, [attempt, onDocument, onDisplayZoom]);

  return <section className="canvas-workspace" aria-label={t.canvas}>
    <div className="canvas-stage" data-document={hasDocument ? 'open' : 'empty'}>
    {hasDocument && <Rulers locale={locale} resolution={resolution} verticalResolution={verticalResolution} viewport={info?.rulerViewport ?? null} />}
    <div ref={slot} className="native-slot" data-document={hasDocument ? 'open' : 'empty'} data-tool={tool} role={hasDocument ? 'img' : undefined} aria-label={hasDocument ? (tool === 'text' || tool === 'textVertical') ? textPanelMessages[locale].hint : tool.startsWith('imageFrame') ? workspaceMessages[locale].frameHint : tool === 'crop' ? workspaceMessages[locale].cropHint : tool === 'gradient' ? workspaceMessages[locale].gradientHint : tool === 'eyedropper' ? workspaceMessages[locale].eyedropperHint : isRetouch(tool)?retouchLabels[locale].hint:isPathSelection(tool)?selectionPathLabels[locale].hint:tool === 'paintBucket' ? paintBucketLabels[locale].hint : tool === 'cloneStamp' ? cloneStampLabels[locale].hint : tool === 'brush' ? t.canvasNote : tool === 'hand' ? workspaceMessages[locale].handHint : tool === 'zoomIn' || tool === 'zoomOut' ? workspaceMessages[locale].zoomClickHint : tool.startsWith('vector') ? workspaceMessages[locale].vectorHint : `${workspaceMessages[locale][tool]} · ${workspaceMessages[locale].selectionHint}` : undefined}>
      {hasDocument && (status === 'browser' || status === 'unsupported') && <div className="paper-preview" aria-hidden="true" />}
      {status === 'failed' ? <div className="canvas-notice canvas-failure" role="alert">
        <p>{t.canvasStatus.failed}</p>
        <pre aria-label={t.errorDetails}>{error}</pre>
        <button type="button" onClick={retry}>{t.retry}</button>
      </div> : hasDocument && status !== 'ready' && <p className="canvas-notice">{t.canvasStatus[status]}</p>}
    </div>
    </div>
    <div className="canvas-footer">
      <div className="zoom-controls">
        <button type="button" className="icon-button" title={t.zoomOut} aria-label={t.zoomOut} disabled={status !== 'ready' || zoom <= MIN_ZOOM} onClick={() => onZoom(stepZoom(zoom, -1))}><Icon name="minus" /></button>
        <select aria-label={t.zoom} disabled={status !== 'ready'} value={String(selectedZoom != null ? selectedZoom / 100 : zoom)} onChange={event => onZoom(Number(event.target.value))}>
          {selectedZoom == null && <option value={String(zoom)}>{zoomLabel(zoom)}</option>}
          {[...ZOOM_PERCENTAGES].reverse().map(percent => <option key={percent} value={String(percent / 100)}>{percent}%</option>)}
          <option value="0">{t.fit}</option>
        </select>
        <button type="button" className="icon-button" title={t.zoomIn} aria-label={t.zoomIn} disabled={status !== 'ready' || zoom >= MAX_ZOOM - 0.0001} onClick={() => onZoom(stepZoom(zoom, 1))}><Icon name="plus" /></button>
        <button type="button" disabled={status !== 'ready'} onClick={() => { void resetCanvasPan().then(() => onZoom(0)); }}>{t.fit}</button>
      </div>
      {footerAccessory}
      <span className="canvas-state" role="status" title={info ? `${info.backend} · ${info.adapterName} · ${info.physicalWidth} × ${info.physicalHeight} px · ${info.scaleFactor}×` : undefined}>{t.canvasStatus[status]}</span>
      {status === 'failed' && <button type="button" onClick={retry}>{t.retry}</button>}
    </div>
  </section>;
}
