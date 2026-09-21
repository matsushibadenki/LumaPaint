import { useEffect, useRef, useState, type PointerEvent } from 'react';
import type { LayerSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { Icon } from './Icon';

type Gesture = { id: string; pointerId: number; startX: number; startY: number; x: number; y: number; dragging: boolean };

export function LayerList({ layers, selectedId, enabled, locale, onSelect, onToggle, onRename, onReorder }: {
  layers: LayerSnapshot[]; selectedId: string; enabled: boolean; locale: Locale;
  onSelect: (id: string) => void; onToggle: (id: string) => void;
  onRename: (layer: LayerSnapshot, name: string) => void; onReorder: (ids: string[]) => void;
}) {
  const t = workspaceMessages[locale];
  const list = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [gap, setGap] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [name, setName] = useState('');
  const displayed = [...layers].reverse();
  const orderKey = displayed.map(layer => layer.id).join('|');

  function cancel() {
    const current = gesture.current;
    gesture.current = null;
    if (current && list.current?.hasPointerCapture(current.pointerId)) list.current.releasePointerCapture(current.pointerId);
    setDraggedId(null);
    setGap(null);
  }

  // Changing documents, losing focus, or leaving the panel must never commit a drag.
  useEffect(() => {
    cancel();
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') cancel(); };
    window.addEventListener('keydown', escape);
    window.addEventListener('blur', cancel);
    return () => { cancel(); window.removeEventListener('keydown', escape); window.removeEventListener('blur', cancel); };
  }, [orderKey, enabled]);

  function hitGap(x: number, y: number): number | null {
    const element = list.current;
    if (!element) return null;
    const bounds = element.getBoundingClientRect();
    if (x < bounds.left || x > bounds.right || y < bounds.top || y > bounds.bottom) return null;
    const rows = [...element.querySelectorAll<HTMLElement>('[data-layer-id]')];
    const index = rows.findIndex(row => {
      const rect = row.getBoundingClientRect();
      return y < rect.top + rect.height / 2;
    });
    // The legacy base layer is fixed; the last valid gap is immediately above it.
    return Math.min(index < 0 ? rows.length : index, layers.filter(layer => layer.deletable).length);
  }

  useEffect(() => {
    if (!draggedId) return;
    let frame = 0;
    const scroll = () => {
      const current = gesture.current;
      const element = list.current;
      if (!current || !element) return;
      const rect = element.getBoundingClientRect();
      if (current.x >= rect.left && current.x <= rect.right && current.y >= rect.top && current.y <= rect.bottom) {
        const speed = current.y < rect.top + 28 ? -7 : current.y > rect.bottom - 28 ? 7 : 0;
        if (speed) { element.scrollTop += speed; setGap(hitGap(current.x, current.y)); }
      }
      frame = requestAnimationFrame(scroll);
    };
    frame = requestAnimationFrame(scroll);
    return () => cancelAnimationFrame(frame);
  }, [draggedId, orderKey]);

  function start(event: PointerEvent<HTMLDivElement>, layer: LayerSnapshot) {
    if (!enabled || event.button !== 0 || !event.isPrimary || gesture.current || (event.target as Element).closest('button, input')) return;
    onSelect(layer.id);
    event.currentTarget.focus({ preventScroll: true });
    if (!layer.deletable) return;
    gesture.current = { id: layer.id, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY, dragging: false };
  }

  function move(event: PointerEvent<HTMLDivElement>) {
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    current.x = event.clientX; current.y = event.clientY;
    if (!current.dragging && Math.hypot(current.x - current.startX, current.y - current.startY) < 5) return;
    event.preventDefault();
    if (!current.dragging) list.current?.setPointerCapture(event.pointerId);
    current.dragging = true;
    setDraggedId(current.id);
    setGap(hitGap(current.x, current.y));
  }

  function finish(event: PointerEvent<HTMLDivElement>) {
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const insertion = hitGap(event.clientX, event.clientY);
    if (enabled && current.dragging && insertion !== null) {
      const ids = displayed.filter(layer => layer.deletable).map(layer => layer.id);
      const from = ids.indexOf(current.id);
      if (from >= 0) {
        const next = [...ids];
        next.splice(from, 1);
        next.splice(insertion > from ? insertion - 1 : insertion, 0, current.id);
        if (next.some((id, index) => id !== ids[index])) onReorder(next);
      }
    }
    cancel();
  }

  function commitName(layer: LayerSnapshot) {
    const trimmed = name.trim();
    setEditingId(null);
    if (trimmed && trimmed !== layer.name) onRename(layer, trimmed);
  }

  return <div ref={list} className={`layer-list${draggedId ? ' is-dragging' : ''}`} role="list" aria-label={t.layers}
    onPointerMove={move} onPointerUp={finish} onPointerCancel={cancel} onLostPointerCapture={cancel}
    onPointerLeave={() => { if (!gesture.current?.dragging) cancel(); }}
    onDragStart={event => event.preventDefault()}>
    {displayed.map((layer, index) => <div key={layer.id} data-layer-id={layer.id} role="listitem" tabIndex={enabled ? 0 : -1}
      className={`layer-row${selectedId === layer.id ? ' selected' : ''}${draggedId === layer.id ? ' dragging' : ''}${gap === index ? ' insert-before' : ''}`}
      data-movable={enabled && layer.deletable} title={layer.deletable ? t.reorderLayer : t.fixedBaseLayer}
      onPointerDown={event => start(event, layer)}
      onKeyDown={event => {
        if (!enabled || (event.target as Element).closest('input, button')) return;
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onSelect(layer.id); }
        if (!event.altKey || !['ArrowUp', 'ArrowDown'].includes(event.key) || !layer.deletable) return;
        event.preventDefault();
        const ids = displayed.filter(item => item.deletable).map(item => item.id);
        const target = index + (event.key === 'ArrowUp' ? -1 : 1);
        if (target >= 0 && target < ids.length) { ids.splice(index, 1); ids.splice(target, 0, layer.id); onSelect(layer.id); onReorder(ids); }
      }}>
      <button className="icon-button" disabled={!enabled} onClick={() => onToggle(layer.id)} title={layer.visible ? t.hideLayer : t.showLayer}
        aria-label={`${layer.visible ? t.hideLayer : t.showLayer}: ${layer.name}`} aria-pressed={layer.visible}><Icon name={layer.visible ? 'eye' : 'eyeOff'} /></button>
      <span className={`layer-thumb ${layer.kind}`} aria-hidden="true">{layer.kind === 'svg' ? 'SVG' : ''}</span>
      {layer.maskEnabled && <span className={`mask-thumb${layer.maskInverted ? ' inverted' : ''}`} aria-hidden="true" />}
      {editingId === layer.id ? <input className="layer-name" autoFocus maxLength={120} aria-label={t.renameLayer} value={name}
        onFocus={event => event.currentTarget.select()} onChange={event => setName(event.target.value)}
        onBlur={() => commitName(layer)} onKeyDown={event => { if (event.key === 'Enter') commitName(layer); if (event.key === 'Escape') setEditingId(null); }} />
        : <span className="layer-name" onDoubleClick={() => { if (enabled) { setName(layer.name); setEditingId(layer.id); } }}>{layer.name}</span>}
      {layer.alphaLocked && <span className="layer-lock" title={t.lockAlpha}>α</span>}
      {layer.locked && <span className="layer-lock" title={t.lockLayer}>▣</span>}
    </div>)}
  </div>;
}
