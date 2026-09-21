import { useEffect, useRef, useState, type CSSProperties, type DragEvent, type FormEvent, type KeyboardEvent } from 'react';
import type { BitDepth, Brush, ColorMode, ColorProfile, DocumentSettings, DocumentSnapshot, LayerSettings, TextSettings } from '../bridge';
import { readPreference, savePreference, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { HexInput, MAX_BRUSH_SIZE, PercentInput, SizeInput, fromHex, toHex } from './BrushControls';
import { Icon } from './Icon';
import { LayerList } from './LayerList';
import { TextPanel } from './TextPanel';
import { textPanelMessages } from '../text-panel-i18n';
import { ColorPanel, colorPanelLabels, type ColorTarget } from './ColorPanel';

const swatches = ['#202020', '#808080', '#ffffff', '#e5796b', '#d6a13e', '#6b9c76', '#538fd2', '#a875ce'];
const panelIds = ['brush', 'color', 'document', 'layers', 'text'] as const;
type PanelId = (typeof panelIds)[number];

function pixelsPerUnit(unit: DocumentSettings['unit'], resolution: number) {
  const dpi = Math.max(1, resolution || 72);
  if (unit === 'inches') return dpi;
  if (unit === 'centimeters') return dpi / 2.54;
  if (unit === 'millimeters') return dpi / 25.4;
  return 1;
}

function displaySize(pixels: number, unit: DocumentSettings['unit'], resolution: number) {
  const value = pixels / pixelsPerUnit(unit, resolution);
  return unit === 'pixels' ? Math.round(value) : Number(value.toFixed(3));
}

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

export function Inspector({ textPanelRequest, textSettings, textEditing, textEnabled, onTextChange, onTextBegin, onTextFinish, locale, brush, backgroundColor, activeColor, onSelectColor, colorPanelRequest, onBrush, onBackgroundChange, onSwapColors, document, onDocumentSettings, onColorMode, onBitDepth, onColorProfile, onToggleLayer, onLayerSettings, onDeleteLayer, onAddLayer, onReorderLayer, enabled }: {
  textPanelRequest: number; textSettings: TextSettings | null; textEditing: boolean; textEnabled: boolean;
  onTextChange: (settings: TextSettings) => Promise<void>; onTextBegin: () => void; onTextFinish: (commit: boolean) => void;
  locale: Locale; brush: Brush; onBrush: (brush: Brush) => void; document: DocumentSnapshot; onToggleLayer: (id: string) => void; enabled: boolean;
  backgroundColor: Brush['color']; activeColor: ColorTarget; onSelectColor: (target: ColorTarget) => void; colorPanelRequest: number; onBackgroundChange: (color: Brush['color']) => void; onSwapColors: () => void;
  onDocumentSettings: (settings: DocumentSettings) => void; onColorMode: (mode: ColorMode) => void; onBitDepth: (depth: BitDepth) => void; onColorProfile: (profile: ColorProfile) => void; onLayerSettings: (settings: LayerSettings) => void; onDeleteLayer: (id: string) => void; onAddLayer: () => void; onReorderLayer: (ids: string[]) => void;
}) {
  const t = workspaceMessages[locale];
  const [order, setOrder] = useState<PanelId[]>(initialPanelOrder);
  const [activePanel, setActivePanel] = useState<PanelId>(() => textPanelRequest > 0 ? 'text' : colorPanelRequest > 0 ? 'color' : initialPanel());
  useEffect(() => { if (textPanelRequest > 0) setActivePanel('text'); }, [textPanelRequest]);
  useEffect(() => { if (colorPanelRequest > 0) setActivePanel('color'); }, [colorPanelRequest]);
  const [draggedPanel, setDraggedPanel] = useState<PanelId | null>(null);
  const [dropTarget, setDropTarget] = useState<PanelId | null>(null);
  const [settings, setSettings] = useState<DocumentSettings>(() => ({ name: document.name, width: document.width, height: document.height, unit: document.unit, resolution: document.resolution, artboards: document.artboards, canvasColor: document.canvasColor, pixelAspectRatio: document.pixelAspectRatio }));
  const [displayDimensions, setDisplayDimensions] = useState(() => ({ width: displaySize(document.width, document.unit, document.resolution), height: displaySize(document.height, document.unit, document.resolution) }));
  const [selectedLayerId, setSelectedLayerId] = useState(document.layers.at(-1)?.id ?? 'layer-1');
  const previousLayerCount = useRef(document.layers.length);
  const [layerPanelMode, setLayerPanelMode] = useState<'layers' | 'channels'>('layers');
  const labels: Record<PanelId, string> = { brush: t.brush, color: colorPanelLabels[locale].color, document: t.document, layers: t.layers, text: textPanelMessages[locale].title };

  useEffect(() => savePreference('inspectorOrder', JSON.stringify(order)), [order]);
  useEffect(() => savePreference('inspectorPanel', activePanel), [activePanel]);
  useEffect(() => {
    setSettings({ name: document.name, width: document.width, height: document.height, unit: document.unit, resolution: document.resolution, artboards: document.artboards, canvasColor: document.canvasColor, pixelAspectRatio: document.pixelAspectRatio });
    setDisplayDimensions({ width: displaySize(document.width, document.unit, document.resolution), height: displaySize(document.height, document.unit, document.resolution) });
  }, [document.name, document.width, document.height, document.unit, document.resolution, document.artboards, document.canvasColor, document.pixelAspectRatio]);
  useEffect(() => {
    if (!document.layers.some(layer => layer.id === selectedLayerId)) setSelectedLayerId(document.layers.at(-1)?.id ?? 'layer-1');
    else if (document.layers.length > previousLayerCount.current) setSelectedLayerId(document.layers.at(-1)?.id ?? 'layer-1');
    previousLayerCount.current = document.layers.length;
  }, [document.layers, selectedLayerId]);
  const selectedLayer = document.layers.find(layer => layer.id === selectedLayerId) ?? document.layers.at(-1);
  const layerSettings = (layer: NonNullable<typeof selectedLayer>, changes: Partial<LayerSettings> = {}): LayerSettings => ({ id: layer.id, name: layer.name, opacity: layer.opacity, locked: layer.locked, alphaLocked: layer.alphaLocked, maskEnabled: layer.maskEnabled, maskInverted: layer.maskInverted, maskDensity: layer.maskDensity, ...changes });
  function submitDocument(event: FormEvent) {
    event.preventDefault();
    const factor = pixelsPerUnit(settings.unit, settings.resolution);
    onDocumentSettings({ ...settings, width: Math.max(1, Math.round(displayDimensions.width * factor)), height: Math.max(1, Math.round(displayDimensions.height * factor)) });
  }

  function changeUnit(unit: DocumentSettings['unit']) {
    const pixels = {
      width: displayDimensions.width * pixelsPerUnit(settings.unit, settings.resolution),
      height: displayDimensions.height * pixelsPerUnit(settings.unit, settings.resolution),
    };
    setSettings(value => ({ ...value, unit }));
    setDisplayDimensions({ width: displaySize(pixels.width, unit, settings.resolution), height: displaySize(pixels.height, unit, settings.resolution) });
  }

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

    {activePanel === 'text' && <section className="inspector-panel" role="tabpanel" id="inspector-panel-text" aria-labelledby="inspector-tab-text">
      <TextPanel locale={locale} settings={textSettings} resolution={document.resolution} enabled={textEnabled} editing={textEditing} onChange={onTextChange} onBegin={onTextBegin} onFinish={onTextFinish} />
    </section>}

    {activePanel === 'color' && <section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-color" aria-labelledby="inspector-tab-color">
      <ColorPanel locale={locale} color={brush.color} backgroundColor={backgroundColor} activeColor={activeColor} onSelectColor={onSelectColor} onChange={color => onBrush({ ...brush, color })} onBackgroundChange={onBackgroundChange} onSwap={onSwapColors} />
    </section>}
    {activePanel === 'brush' && <section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-brush" aria-labelledby="inspector-tab-brush">
      <p className="muted small">{t.roundBrush}</p>
      <div className="diameter-row"><label htmlFor="brush-size">{t.diameter}</label><input id="brush-size" type="range" min="1" max={MAX_BRUSH_SIZE} value={brush.size} onChange={event => onBrush({ ...brush, size: Number(event.target.value) })} /><SizeInput label={t.diameter} value={brush.size} onChange={size => onBrush({ ...brush, size })} /></div>
      <div className="diameter-row"><label htmlFor="brush-hardness">{t.hardness}</label><input id="brush-hardness" type="range" min="0" max="100" value={Math.round(brush.hardness * 100)} onChange={event => onBrush({ ...brush, hardness: Number(event.target.value) / 100 })} /><PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => onBrush({ ...brush, hardness })} /></div>
      <div className="swatches" aria-label={t.foreground}>{swatches.map((hex, index) => <button key={hex} className="swatch" title={t.colors[index]} aria-label={t.colors[index]} aria-pressed={toHex(brush.color) === hex} style={{ '--swatch': hex } as CSSProperties} onClick={() => onBrush({ ...brush, color: fromHex(hex) })} />)}</div>
      <HexInput label={t.hex} invalid={t.invalidColor} color={brush.color} onChange={color => onBrush({ ...brush, color })} />
    </section>}

    {activePanel === 'document' && <section className="inspector-panel document-settings-panel" role="tabpanel" id="inspector-panel-document" aria-labelledby="inspector-tab-document">
      <form className="document-settings-form" onSubmit={submitDocument}>
        <label className="document-name-field"><span>{t.documentName}</span><input disabled={!enabled} value={settings.name} maxLength={120} onChange={event => setSettings(value => ({ ...value, name: event.target.value }))} /></label>
        <div className="document-size-grid">
          <label><span>{t.width}</span><input disabled={!enabled} type="number" min={settings.unit === 'pixels' ? 1 : 0.001} step={settings.unit === 'pixels' ? 1 : 0.001} value={displayDimensions.width} onChange={event => setDisplayDimensions(value => ({ ...value, width: Number(event.target.value) }))} /></label>
          <label><span>{t.height}</span><input disabled={!enabled} type="number" min={settings.unit === 'pixels' ? 1 : 0.001} step={settings.unit === 'pixels' ? 1 : 0.001} value={displayDimensions.height} onChange={event => setDisplayDimensions(value => ({ ...value, height: Number(event.target.value) }))} /></label>
          <label className="document-unit"><span>{t.unit}</span><select disabled={!enabled} value={settings.unit} onChange={event => changeUnit(event.target.value as DocumentSettings['unit'])}><option value="pixels">{t.pixels}</option><option value="inches">{t.inches}</option><option value="centimeters">{t.centimeters}</option><option value="millimeters">{t.millimeters}</option></select></label>
        </div>
        <div className="document-direction"><span>{t.orientation}</span><button type="button" disabled={!enabled} aria-pressed={displayDimensions.height >= displayDimensions.width} onClick={() => setDisplayDimensions(value => value.height >= value.width ? value : ({ width: value.height, height: value.width }))}>{t.portrait}</button><button type="button" disabled={!enabled} aria-pressed={displayDimensions.width > displayDimensions.height} onClick={() => setDisplayDimensions(value => value.width > value.height ? value : ({ width: value.height, height: value.width }))}>{t.landscape}</button></div>
        <label className="document-check"><input disabled={!enabled} type="checkbox" checked={settings.artboards} onChange={event => setSettings(value => ({ ...value, artboards: event.target.checked }))} />{t.artboards}</label>
        <label><span>{t.resolution}</span><div className="field-with-unit"><input disabled={!enabled} type="number" min="1" max="1200" value={settings.resolution} onChange={event => setSettings(value => ({ ...value, resolution: Number(event.target.value) }))} /><span>{t.pixelsPerInch}</span></div></label>
        <div className="document-size-grid"><label><span>{t.colorMode}</span><select disabled={!enabled} value={document.colorMode} onChange={event => onColorMode(event.target.value as ColorMode)}><option value="rgb">RGB {t.color}</option><option value="cmyk">CMYK {t.color}</option></select></label><label><span>{t.bitDepth}</span><select disabled={!enabled} value={document.bitDepth} onChange={event => onBitDepth(Number(event.target.value) as BitDepth)}><option value="8">8 bit</option><option value="16">16 bit</option><option value="32">32 bit</option></select></label></div>
        <label><span>{t.canvasColor}</span><select disabled={!enabled} value={settings.canvasColor} onChange={event => setSettings(value => ({ ...value, canvasColor: event.target.value as DocumentSettings['canvasColor'] }))}><option value="white">{t.white}</option><option value="transparent">{t.transparent}</option></select></label>
        <details open><summary>{t.advancedOptions}</summary><label><span>{t.colorProfile}</span><select disabled={!enabled} value={document.colorProfile} onChange={event => onColorProfile(event.target.value as ColorProfile)}>{document.colorMode === 'rgb' ? <><option value="srgb">sRGB</option><option value="displayP3">Display P3</option><option value="adobeRgb1998">Adobe RGB (1998)</option></> : <option value="japanColor2001Coated">Japan Color 2001 Coated</option>}</select></label><label><span>{t.pixelAspectRatio}</span><select disabled={!enabled} value={settings.pixelAspectRatio} onChange={event => setSettings(value => ({ ...value, pixelAspectRatio: Number(event.target.value) }))}><option value="1">{t.squarePixels}</option><option value="1.2">1.2</option><option value="0.9">0.9</option></select></label></details>
        <button className="document-apply" disabled={!enabled} type="submit">{t.applyDocumentSettings}</button>
      </form>
    </section>}

    {activePanel === 'layers' && <section className="inspector-panel layer-panel" role="tabpanel" id="inspector-panel-layers" aria-labelledby="inspector-tab-layers">
      <div className="layer-subtabs"><button className={layerPanelMode === 'layers' ? 'active' : ''} onClick={() => setLayerPanelMode('layers')}>{t.layers}</button><button className={layerPanelMode === 'channels' ? 'active' : ''} onClick={() => setLayerPanelMode('channels')}>{t.channels}</button><button disabled>{t.paths}</button></div>
      {layerPanelMode === 'channels' ? <div className="channel-list">
        {[t.compositeChannel, 'Red', 'Green', 'Blue', t.alphaChannel].map((channel, index) => <div className={`channel-row${index === 4 ? ' alpha' : ''}`} key={channel}><Icon name="eye" /><span className="channel-thumb">{index === 4 ? 'α' : index === 0 ? 'RGB' : channel[0]}</span><span>{channel}</span></div>)}
      </div> : <>
        <div className="layer-compositing"><label><span>{t.layerBlendMode}</span><select disabled><option>{t.normalBlend}</option></select></label><label><span>{t.layerOpacity}</span><div><input disabled={!enabled || !selectedLayer} type="range" min="0" max="100" value={Math.round((selectedLayer?.opacity ?? 1) * 100)} onChange={event => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { opacity: Number(event.target.value) / 100 }))} /><output>{Math.round((selectedLayer?.opacity ?? 1) * 100)}%</output></div></label></div>
        <div className="layer-lock-row"><span>{t.lockLayer}</span><button type="button" disabled={!enabled || !selectedLayer} className={selectedLayer?.locked ? 'active' : ''} aria-pressed={selectedLayer?.locked ?? false} title={selectedLayer?.locked ? t.unlockLayer : t.lockLayer} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { locked: !selectedLayer.locked }))}>▣</button><button type="button" disabled={!enabled || !selectedLayer || selectedLayer.kind !== 'paint'} className={selectedLayer?.alphaLocked ? 'active' : ''} aria-pressed={selectedLayer?.alphaLocked ?? false} title={t.lockAlpha} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { alphaLocked: !selectedLayer.alphaLocked }))}>α</button><span className="layer-fill">{t.layerFill}: 100%</span></div>
        {selectedLayer?.maskEnabled && <div className="mask-controls"><label><span>{t.maskDensity}</span><input type="range" min="0" max="100" value={Math.round(selectedLayer.maskDensity * 100)} onChange={event => onLayerSettings(layerSettings(selectedLayer, { maskDensity: Number(event.target.value) / 100 }))} /><output>{Math.round(selectedLayer.maskDensity * 100)}%</output></label><button className={selectedLayer.maskInverted ? 'active' : ''} onClick={() => onLayerSettings(layerSettings(selectedLayer, { maskInverted: !selectedLayer.maskInverted }))}>{t.invertMask}</button></div>}
        <LayerList layers={document.layers} selectedId={selectedLayerId} enabled={enabled} locale={locale}
          onSelect={setSelectedLayerId} onToggle={onToggleLayer} onReorder={onReorderLayer}
          onRename={(layer, name) => onLayerSettings(layerSettings(layer, { name }))} />
        <div className="layer-actions"><button disabled={!enabled} title={t.addLayer} aria-label={t.addLayer} onClick={onAddLayer}>＋</button><button disabled title={t.layerOptions} aria-label={t.layerOptions}>fx</button><button disabled={!enabled || !selectedLayer} className={selectedLayer?.maskEnabled ? 'active' : ''} title={selectedLayer?.maskEnabled ? t.removeMask : t.addMask} aria-label={selectedLayer?.maskEnabled ? t.removeMask : t.addMask} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { maskEnabled: !selectedLayer.maskEnabled, maskDensity: 1, maskInverted: false }))}>◐</button><button disabled={!enabled || !selectedLayer?.deletable} title={t.deleteLayer} aria-label={t.deleteLayer} onClick={() => selectedLayer && onDeleteLayer(selectedLayer.id)}>⌫</button></div><p className="layer-count">{document.layers.length} {t.layerUnit}<span>{document.strokeCount} {t.strokes}</span></p>
      </>}
    </section>}
  </aside>;
}
