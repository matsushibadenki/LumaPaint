import { useEffect, useId, useRef, useState, type ReactNode, type KeyboardEvent, type PointerEvent } from 'react';
import { TimelinePanel } from './TimelinePanel';
import { AiGenerationPanel } from './AiGenerationPanel';
import type { Locale } from '../i18n';

const labels = {
  ja: { tabs: 'ドキュメント下部パネル', timeline: 'タイムライン', generation: 'AI生成', resize: '上下エリアの高さを調整', close: '下部パネルを閉じる' },
  en: { tabs: 'Document bottom panels', timeline: 'Timeline', generation: 'AI Generation', resize: 'Resize upper and lower areas', close: 'Close bottom panel' },
  'zh-CN': { tabs: '文档底部面板', timeline: '时间轴', generation: 'AI生成', resize: '调整上下区域高度', close: '关闭底部面板' },
};
type Tab = 'timeline' | 'generation';
const tabs: Tab[] = ['timeline', 'generation'];

/** Only view layout lives here; document and rendering state remain in Rust. */
export function DocumentDock({ locale, children }: { locale: Locale; children: ReactNode }) {
  const t = labels[locale];
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const buttons = useRef<Partial<Record<Tab, HTMLButtonElement | null>>>({});
  const [active, setActive] = useState<Tab | null>(null);
  const [height, setHeight] = useState(200);
  const panelOpened = useRef(false);
  function prepareTab(tab: Tab) {
    if ((tab === 'generation' || tab === 'timeline') && !panelOpened.current) { panelOpened.current = true; setHeight(value => Math.max(value, 420)); }
  }
  const [available, setAvailable] = useState(0);
  const [availableWidth, setAvailableWidth] = useState(0);
  const drag = useRef<{ pointer: number; y: number; height: number } | null>(null);
  const pending = useRef<number | null>(null);
  const frame = useRef(0);
  // Reserve a useful canvas area, the tab strip and the draggable divider.
  const max = Math.max(0, available - 120 - 34 - 8);
  const min = Math.min(active === 'generation' ? (availableWidth <= 650 ? 400 : 300) : active === 'timeline' ? (availableWidth <= 700 ? 280 : 200) : 80, max);
  const clamp = (value: number) => Math.max(min, Math.min(max, value));
  const panelHeight = clamp(height);

  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const observer = new ResizeObserver(() => {
      const bounds = element.getBoundingClientRect();
      setAvailable(bounds.height); setAvailableWidth(bounds.width);
    });
    observer.observe(element);
    return () => { observer.disconnect(); cancelAnimationFrame(frame.current); };
  }, []);

  function resize(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointer !== event.pointerId) return;
    pending.current = clamp(current.height + current.y - event.clientY);
    if (!frame.current) frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      if (pending.current !== null) setHeight(pending.current);
    });
  }
  function endResize(event: PointerEvent<HTMLDivElement>, cancel = false) {
    if (drag.current?.pointer !== event.pointerId) return;
    cancelAnimationFrame(frame.current); frame.current = 0;
    if (cancel) setHeight(drag.current.height);
    else if (pending.current !== null) setHeight(pending.current);
    drag.current = null; pending.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }
  function tabKey(event: KeyboardEvent<HTMLButtonElement>, tab: Tab) {
    const index = tabs.indexOf(tab);
    const next = event.key === 'ArrowRight' ? tabs[(index + 1) % tabs.length]
      : event.key === 'ArrowLeft' ? tabs[(index + tabs.length - 1) % tabs.length]
      : event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs[tabs.length - 1] : null;
    if (next) { event.preventDefault(); event.stopPropagation(); prepareTab(next); setActive(next); buttons.current[next]?.focus(); }
    if (event.key === 'Escape' && active) { event.preventDefault(); event.stopPropagation(); setActive(null); }
  }
  return <div ref={root} className="document-dock" data-open={active !== null}>
    <div className="document-dock-canvas">{children}</div>
    {active && <>
      <div className="document-dock-divider" role="separator" tabIndex={0} aria-label={t.resize} aria-orientation="horizontal"
        aria-controls={`${id}-panel`} aria-valuemin={Math.round(min)} aria-valuemax={Math.round(max)} aria-valuenow={Math.round(panelHeight)}
        onPointerDown={event => {
          if (event.button !== 0 || drag.current) return;
          event.preventDefault(); event.stopPropagation(); event.currentTarget.focus();
          drag.current = { pointer: event.pointerId, y: event.clientY, height: panelHeight };
          pending.current = null; event.currentTarget.setPointerCapture(event.pointerId);
        }} onPointerMove={resize} onPointerUp={event => endResize(event)} onPointerCancel={event => endResize(event, true)}
        onLostPointerCapture={event => endResize(event)} onKeyDown={event => {
          const step = event.shiftKey ? 64 : 16;
          const next = event.key === 'ArrowUp' ? panelHeight + step : event.key === 'ArrowDown' ? panelHeight - step
            : event.key === 'Home' ? min : event.key === 'End' ? max : null;
          if (next !== null) { event.preventDefault(); event.stopPropagation(); setHeight(clamp(next)); }
          if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); buttons.current[active]?.focus(); setActive(null); }
        }}><span aria-hidden="true" /></div>
    </>}
    <section id={`${id}-panel`} className="document-dock-panel" data-panel={active} role="tabpanel" hidden={!active} aria-labelledby={`${id}-${active ?? 'generation'}`} tabIndex={0} style={{ height: panelHeight }}>
      <div className="ai-dock-content" hidden={active !== 'generation'}><AiGenerationPanel locale={locale} /></div>
      {active === 'timeline' && <TimelinePanel locale={locale} />}
    </section>
    <div className="document-dock-bar">
      <div role="tablist" aria-label={t.tabs} className="document-dock-tabs">
        {tabs.map(tab => <button key={tab} ref={element => { buttons.current[tab] = element; }} id={`${id}-${tab}`} type="button" role="tab"
          aria-selected={active === tab} aria-expanded={active === tab} aria-controls={active === tab ? `${id}-panel` : undefined}
          tabIndex={active === null || active === tab ? 0 : -1} onKeyDown={event => tabKey(event, tab)}
          onClick={() => { prepareTab(tab); setActive(previous => previous === tab ? null : tab); }}>{t[tab]}</button>)}
      </div>
      {active && <button className="document-dock-close" type="button" aria-label={t.close} title={t.close}
        onClick={() => { buttons.current[active]?.focus(); setActive(null); }}>×</button>}
    </div>
  </div>;
}
