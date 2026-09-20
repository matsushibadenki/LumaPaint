import { useEffect, useState, type CSSProperties, type DragEvent, type KeyboardEvent } from 'react';
import type { Brush, DocumentSnapshot } from '../bridge';
import { readPreference, savePreference, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { HexInput, MAX_BRUSH_SIZE, PercentInput, SizeInput, fromHex, toHex } from './BrushControls';
import { Icon } from './Icon';
import { ColorPanel, colorPanelLabels } from './ColorPanel';

const swatches = ['#202020', '#808080', '#ffffff', '#e5796b', '#d6a13e', '#6b9c76', '#538fd2', '#a875ce'];
const panelIds = ['brush', 'color', 'document', 'layers'] as const;
type PanelId = (typeof panelIds)[number];

function isPanelId(value: unknown): value is PanelId {
  return typeof value === 'string' && panelIds.includes(value as PanelId);
}

function initialPanelOrder(): PanelId[] {
  const stored = readPreference('inspectorOrder');
  if (!stored) return [...panelIds];
  try {
    const parsed: unknown = JSON.parse(stored);
    if (Array.isArray(parsed) && parsed.every(isPanelId)) {
      const stored = [...new Set<PanelId>(parsed)];
      return [...stored, ...panelIds.filter(panel => !stored.includes(panel))];
    }
  } catch { /* Use the default order when an old preference cannot be read. */ }
  return [...panelIds];
}

function initialPanel(): PanelId {
  const stored = readPreference('inspectorPanel');
  return isPanelId(stored) ? stored : 'brush';
}

function Grip() {
  return <svg className="tab-grip" viewBox="0 0 8 12" aria-hidden="true"><circle cx="2" cy="2" r="1" /><circle cx="6" cy="2" r="1" /><circle cx="2" cy="6" r="1" /><circle cx="6" cy="6" r="1" /><circle cx="2" cy="10" r="1" /><circle cx="6" cy="10" r="1" /></svg>;
}

export function Inspector({ locale, brush, onBrush, document, onToggleLayer, enabled }: {
  locale: Locale; brush: Brush; onBrush: (brush: Brush) => void; document: DocumentSnapshot; onToggleLayer: (id: string) => void; enabled: boolean;
}) {
  const t = workspaceMessages[locale];
  const [order, setOrder] = useState<PanelId[]>(initialPanelOrder);
  const [activePanel, setActivePanel] = useState<PanelId>(initialPanel);
  const [draggedPanel, setDraggedPanel] = useState<PanelId | null>(null);
  const [dropTarget, setDropTarget] = useState<PanelId | null>(null);
  const labels: Record<PanelId, string> = { brush: t.brush, color: colorPanelLabels[locale].color, document: t.document, layers: t.layers };

  useEffect(() => savePreference('inspectorOrder', JSON.stringify(order)), [order]);
  useEffect(() => savePreference('inspectorPanel', activePanel), [activePanel]);

  function movePanel(source: PanelId, target: PanelId) {
    if (source === target) return;
    setOrder(current => {
      const targetIndex = current.indexOf(target);
      const next = current.filter(panel => panel !== source);
      next.splice(targetIndex, 0, source);
      return next;
    });
    setActivePanel(source);
  }

  function handleDrop(event: DragEvent<HTMLButtonElement>, target: PanelId) {
    event.preventDefault();
    const transferred = event.dataTransfer.getData('text/plain');
    const source = draggedPanel ?? (isPanelId(transferred) ? transferred : null);
    if (source) movePanel(source, target);
    setDraggedPanel(null);
    setDropTarget(null);
  }

  function handleTabKeyDown(event: KeyboardEvent<HTMLButtonElement>, panel: PanelId) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const index = order.indexOf(panel);
    const nextIndex = (index + (event.key === 'ArrowRight' ? 1 : -1) + order.length) % order.length;
    const nextPanel = order[nextIndex];
    if (event.altKey) movePanel(panel, nextPanel);
    else {
      setActivePanel(nextPanel);
      window.document.getElementById(`inspector-tab-${nextPanel}`)?.focus();
    }
  }

  return <aside className="inspector" aria-label={t.properties}>
    <div className="inspector-tabs" role="tablist" aria-label={t.properties}>
      {order.map(panel => <button
        key={panel}
        id={`inspector-tab-${panel}`}
        className={`inspector-tab${draggedPanel === panel ? ' dragging' : ''}${dropTarget === panel && draggedPanel !== panel ? ' drop-target' : ''}`}
        role="tab"
        aria-selected={activePanel === panel}
        aria-controls={`inspector-panel-${panel}`}
        tabIndex={activePanel === panel ? 0 : -1}
        draggable
        title={t.reorderPanel}
        onClick={() => setActivePanel(panel)}
        onKeyDown={event => handleTabKeyDown(event, panel)}
        onDragStart={event => { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', panel); setDraggedPanel(panel); }}
        onDragEnter={() => setDropTarget(panel)}
        onDragOver={event => { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; }}
        onDrop={event => handleDrop(event, panel)}
        onDragEnd={() => { setDraggedPanel(null); setDropTarget(null); }}
      ><Grip /><span>{labels[panel]}</span></button>)}
    </div>

    {activePanel === 'color' && <section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-color" aria-labelledby="inspector-tab-color">
      <ColorPanel locale={locale} color={brush.color} onChange={color => onBrush({ ...brush, color })} />
    </section>}
    {activePanel === 'brush' && <section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-brush" aria-labelledby="inspector-tab-brush">
      <p className="muted small">{t.roundBrush}</p>
      <div className="diameter-row"><label htmlFor="brush-size">{t.diameter}</label><input id="brush-size" type="range" min="1" max={MAX_BRUSH_SIZE} value={brush.size} onChange={event => onBrush({ ...brush, size: Number(event.target.value) })} /><SizeInput label={t.diameter} value={brush.size} onChange={size => onBrush({ ...brush, size })} /></div>
      <div className="diameter-row"><label htmlFor="brush-hardness">{t.hardness}</label><input id="brush-hardness" type="range" min="0" max="100" value={Math.round(brush.hardness * 100)} onChange={event => onBrush({ ...brush, hardness: Number(event.target.value) / 100 })} /><PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => onBrush({ ...brush, hardness })} /></div>
      <div className="swatches" aria-label={t.foreground}>{swatches.map((hex, index) => <button key={hex} className="swatch" title={t.colors[index]} aria-label={t.colors[index]} aria-pressed={toHex(brush.color) === hex} style={{ '--swatch': hex } as CSSProperties} onClick={() => onBrush({ ...brush, color: fromHex(hex) })} />)}</div>
      <HexInput label={t.hex} invalid={t.invalidColor} color={brush.color} onChange={color => onBrush({ ...brush, color })} />
    </section>}

    {activePanel === 'document' && <section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-document" aria-labelledby="inspector-tab-document">
      <dl className="document-properties"><div><dt>{t.width}</dt><dd>{document.width} px</dd></div><div><dt>{t.height}</dt><dd>{document.height} px</dd></div></dl>
    </section>}

    {activePanel === 'layers' && <section className="inspector-panel layer-panel" role="tabpanel" id="inspector-panel-layers" aria-labelledby="inspector-tab-layers">
      <div className="layer-list">{[...document.layers].reverse().map(layer => <div className="layer-row" key={layer.id}><button className="icon-button" disabled={!enabled} onClick={() => onToggleLayer(layer.id)} title={layer.visible ? t.hideLayer : t.showLayer} aria-label={`${layer.visible ? t.hideLayer : t.showLayer}: ${layer.name}`} aria-pressed={layer.visible}><Icon name={layer.visible ? 'eye' : 'eyeOff'} /></button><span className={`layer-thumb ${layer.kind}`} aria-hidden="true">{layer.kind === 'svg' ? 'SVG' : ''}</span><span className="layer-name">{layer.kind === 'paint' ? t.layer : layer.name}</span></div>)}</div><p className="layer-count">{document.layers.length} {t.layerUnit}<span>{document.strokeCount} {t.strokes}</span></p>
    </section>}
  </aside>;
}
