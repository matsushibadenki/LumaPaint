import { PagesPanel,pagesLabels } from './PagesPanel';
import { useMeasurementUnit, setMeasurementUnit, unitName } from '../measurement-units';
import {LinksPanel,linkLabels} from './LinksPanel';
import { FloatingPanel, fitPlacement, dockingLabels, type PanelPlacement } from './FloatingPanel';
import type { SwatchDraft } from '../bridge';
import { SwatchesPanel, swatchLabels } from './SwatchesPanel';
import { GradientPanel, gradientLabels } from './GradientPanel';
import { PathfinderPanel, pathfinderLabels } from './PathfinderPanel';
import { usePanelThumbnails } from './usePanelThumbnails';
import { PathsPanel, type SavedPathAction } from './PathsPanel';
import { TransformPanel, transformLabels } from './TransformPanel';
import { StrokePanel, strokeLabels } from './StrokePanel';
import { ChannelsPanel } from './ChannelsPanel';
import { CompactSlider } from './CompactSlider';
import { Fragment, useEffect, useState, useRef, type FormEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import type { DisplayChannel, BitDepth, Brush, ColorMode, ColorProfile, DocumentSettings, DocumentSnapshot, LayerSettings, TextSettings } from '../bridge';
import { BrushPresets } from './BrushPresets';
import { readPreference, savePreference, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { HexInput, MAX_BRUSH_SIZE, PercentInput, SizeInput } from './BrushControls';
import { Icon } from './Icon';
import { LayerList } from './LayerList';
import { TextPanel } from './TextPanel';
import { textPanelMessages } from '../text-panel-i18n';
import { ColorPanel, colorPanelLabels, type ColorTarget, type VectorColorControls } from './ColorPanel';

const panelIds = ['color', 'swatches', 'gradient', 'brush', 'stroke', 'transform', 'pathfinder', 'pages', 'links', 'text', 'layers', 'document'] as const;
type PanelId = (typeof panelIds)[number];

function pixelsPerUnit(unit: DocumentSettings['unit'], resolution: number) {
  const dpi = Math.max(1, resolution || 72);
  if (unit === 'inches') return dpi;
  if (unit === 'centimeters') return dpi / 2.54;
  if (unit === 'millimeters') return dpi / 25.4;
  if (unit === 'points') return dpi / 72;
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
  const stored = readPreference('inspectorOrder-grouped-v1');
  if (!stored) return [...panelIds];
  try {
    const parsed: unknown = JSON.parse(stored);
    if (Array.isArray(parsed) && parsed.every(isPanelId)) {
      const stored = [...new Set<PanelId>(parsed)];
      return ['color', ...[...stored, ...panelIds.filter(panel => !stored.includes(panel))].filter(panel => panel !== 'color' && panel !== 'document'), 'document'];
    }
  } catch { /* Use the default order when an old preference cannot be read. */ }
  return [...panelIds];
}

const panelIcons = { pages:'document', links:'links', swatches: 'swatches', gradient: 'gradient', brush: 'brush', color: 'palette', document: 'document', layers: 'layers', text: 'text', stroke: 'stroke', transform: 'transformEach', pathfinder: 'pathfinder' } as const;

export function Inspector({ linksPanelRequest, gradientTool, gradientPanelRequest, onTransformUpdate, thumbnailDocumentKey, onSavedPathAction, vectorColors, channel, onChannel, onStrokeStyle, onStrokeWidth, textPanelRequest, textSettings, textEditing, textEnabled, onTextChange, onTextBegin, onTextFinish, locale, brush, backgroundColor, activeColor, onSelectColor, colorPanelRequest, onBrush, onForegroundChange, onBackgroundChange, onSwapColors, document, onDocumentSettings, onColorMode, onBitDepth, onColorProfile, onToggleLayer, onLayerSettings, onDeleteLayer, onAddLayer, onAddVectorLayer, onReorderLayer, onSelectLayer, onSelectObject, onToggleObject, onReorderObjects, enabled }: {
  linksPanelRequest:number;
  onTransformUpdate: (snapshot: DocumentSnapshot) => void;
  thumbnailDocumentKey: string;
  onSavedPathAction: (action: SavedPathAction, id: string | null, name: string) => Promise<void>;
  vectorColors?: VectorColorControls;
  channel: DisplayChannel; onChannel: (channel: DisplayChannel) => void;
  onStrokeStyle: (patch: Partial<import('../bridge').StrokeStyle>) => Promise<void>;
  onStrokeWidth: (width: number) => Promise<void>;
  onSelectLayer: (id: string) => void;
  gradientTool: boolean; gradientPanelRequest: number;
  textPanelRequest: number; textSettings: TextSettings | null; textEditing: boolean; textEnabled: boolean;
  onTextChange: (settings: TextSettings) => Promise<void>; onTextBegin: () => void; onTextFinish: (commit: boolean) => void;
  locale: Locale; brush: Brush; onBrush: (brush: Brush) => void; document: DocumentSnapshot; onToggleLayer: (id: string) => void; enabled: boolean;
  onForegroundChange: (color: Brush['color']) => void;
  backgroundColor: Brush['color']; activeColor: ColorTarget; onSelectColor: (target: ColorTarget) => void; colorPanelRequest: number; onBackgroundChange: (color: Brush['color']) => void; onSwapColors: () => void;
  onSelectObject: (layerId: string, objectId: string) => void;
  onToggleObject: (layerId: string, objectId: string, visible: boolean) => void;
  onReorderObjects: (layerId: string, ids: string[]) => void;
  onDocumentSettings: (settings: DocumentSettings) => void; onColorMode: (mode: ColorMode) => void; onBitDepth: (depth: BitDepth) => void; onColorProfile: (profile: ColorProfile) => void; onLayerSettings: (settings: LayerSettings) => void; onDeleteLayer: (id: string) => void; onAddLayer: () => void; onAddVectorLayer: () => void; onReorderLayer: (ids: string[]) => void;
}) {
  const t = workspaceMessages[locale];
  const [order, setOrder] = useState<PanelId[]>(initialPanelOrder);
  useEffect(() => setOrder(initialPanelOrder()), []);
  const [swatchDraft,setSwatchDraft]=useState<SwatchDraft|undefined>();
  const openSwatches=()=>{setSwatchDraft(undefined);setActivePanel('swatches');};
  const swatchLink=locale==='ja'?'スウォッチを開く':locale==='en'?'Open swatches':'打开色板';
  const [activePanel, setActivePanel] = useState<PanelId>(() => textPanelRequest > 0 ? 'text' : colorPanelRequest > 0 ? 'color' : 'layers');
  useEffect(()=>{if(linksPanelRequest>0)setActivePanel('links');},[linksPanelRequest]);
  useEffect(()=>{if(gradientPanelRequest>0)setActivePanel('gradient');},[gradientPanelRequest]);
  useEffect(() => { if (textPanelRequest > 0) setActivePanel('text'); }, [textPanelRequest]);
  useEffect(() => { if (colorPanelRequest > 0) setActivePanel('color'); }, [colorPanelRequest]);
  const [draggedPanel, setDraggedPanel] = useState<PanelId | null>(null);
  const [floating,setFloating]=useState<Partial<Record<PanelId,PanelPlacement>>>(()=>{try{const saved=JSON.parse(readPreference('floating-panels-v1')??'{}');return Object.fromEntries(Object.entries(saved).filter(([id,p])=>isPanelId(id)&&p&&typeof p==='object'&&['x','y','width','height'].every(k=>Number.isFinite((p as Record<string,unknown>)[k]))).map(([id,p])=>[id,fitPlacement(p as PanelPlacement)]));}catch{return {};}});
  useEffect(()=>savePreference('floating-panels-v1',JSON.stringify(floating)),[floating]);
  const dockText=dockingLabels[locale];
  const [dockWidth,setDockWidth]=useState(()=>Math.max(260,Math.min(600,Number(readPreference('dock-width-v1'))||308)));
  const resizeDock=useRef<{x:number;width:number}|null>(null);
  useEffect(()=>{window.document.documentElement.style.setProperty('--inspector-width',`${dockWidth}px`);savePreference('dock-width-v1',String(dockWidth));window.dispatchEvent(new Event('resize'));},[dockWidth]);

  function dock(panel:PanelId){setFloating(v=>{const next={...v};delete next[panel];return next;});setActivePanel(panel);}
  function float(panel:PanelId,x=window.innerWidth-680,y=120){setFloating(v=>({...v,[panel]:fitPlacement({x,y,width:308,height:480})}));setActivePanel(order.find(p=>p!==panel&&!floating[p])??panel);}
  function panelView(panel:PanelId,content:ReactNode){if(activePanel!==panel&&!floating[panel])return null;return floating[panel]?<FloatingPanel key={panel} title={labels[panel]} locale={locale} placement={floating[panel]!} onMove={p=>setFloating(v=>({...v,[panel]:p}))} onDock={()=>dock(panel)}>{content}</FloatingPanel>:content;}
  const panelDrag=useRef<{panel:PanelId;x:number;y:number;moved:boolean}|null>(null);
  const suppressClick=useRef(false);
  function panelPointerDown(e:ReactPointerEvent<HTMLElement>,panel:PanelId){if(e.button!==0||(e.target as HTMLElement).closest('.dock-panel-header button'))return;e.currentTarget.setPointerCapture(e.pointerId);panelDrag.current={panel,x:e.clientX,y:e.clientY,moved:false};}
  function panelPointerMove(e:ReactPointerEvent<HTMLElement>){const d=panelDrag.current;if(d&&!d.moved&&Math.hypot(e.clientX-d.x,e.clientY-d.y)>8){d.moved=true;setDraggedPanel(d.panel);}}
  function panelPointerUp(e:ReactPointerEvent<HTMLElement>){const d=panelDrag.current;panelDrag.current=null;setDraggedPanel(null);setDropTarget(null);if(!d?.moved)return;suppressClick.current=true;window.setTimeout(()=>{suppressClick.current=false;},0);const r=e.currentTarget.closest('.inspector')?.getBoundingClientRect();if(r&&(e.clientX<r.left-20||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom))float(d.panel,e.clientX-100,e.clientY);else{const target=order.find(p=>{const b=window.document.getElementById(`inspector-tab-${p}`)?.getBoundingClientRect();return b&&e.clientX>=b.left&&e.clientX<=b.right&&e.clientY>=b.top&&e.clientY<=b.bottom;});if(target)movePanel(d.panel,target);}}
  const [dropTarget, setDropTarget] = useState<PanelId | null>(null);
  const measurementUnit=useMeasurementUnit();
  const [settings, setSettings] = useState<DocumentSettings>(() => ({ name: document.name, width: document.width, height: document.height, unit: measurementUnit, resolution: document.resolution, artboards: document.artboards, canvasColor: document.canvasColor, pixelAspectRatio: document.pixelAspectRatio }));
  const [displayDimensions, setDisplayDimensions] = useState(() => ({ width: displaySize(document.width, measurementUnit, document.resolution), height: displaySize(document.height, measurementUnit, document.resolution) }));
  const selectedLayerId = document.layerId;
  const thumbnails = usePanelThumbnails(thumbnailDocumentKey, document.revision);
  const [layerPanelMode, setLayerPanelMode] = useState<'layers' | 'channels' | 'paths'>('layers');
  const [pathTabError, setPathTabError] = useState('');
  const switchLayerPanel = async (mode: 'layers' | 'channels' | 'paths') => {
    try {
      setPathTabError('');
      if (mode !== 'paths' && document.activeSavedPath) await onSavedPathAction('deactivate', null, '');
      setLayerPanelMode(mode);
    } catch (cause) { setPathTabError(String(cause)); }
  };
  const labels: Record<PanelId, string> = { pages:pagesLabels[locale].title, links:linkLabels[locale].title, swatches: swatchLabels[locale].title, gradient: gradientLabels[locale].title, brush: t.brush, color: colorPanelLabels[locale].color, document: t.document, layers: t.layers, text: textPanelMessages[locale].title, stroke: strokeLabels[locale].title, transform: transformLabels[locale].title, pathfinder: pathfinderLabels[locale].title };

  useEffect(() => savePreference('inspectorOrder-grouped-v1', JSON.stringify(order)), [order]);
  useEffect(() => {
    setSettings({ name: document.name, width: document.width, height: document.height, unit: measurementUnit, resolution: document.resolution, artboards: document.artboards, canvasColor: document.canvasColor, pixelAspectRatio: document.pixelAspectRatio });
    setDisplayDimensions({ width: displaySize(document.width, measurementUnit, document.resolution), height: displaySize(document.height, measurementUnit, document.resolution) });
  }, [document.name, document.width, document.height, document.unit, document.resolution, document.artboards, document.canvasColor, document.pixelAspectRatio, measurementUnit]);
  const selectedLayer = document.layers.find(layer => layer.id === selectedLayerId) ?? document.layers.at(-1);
  const layerSettings = (layer: NonNullable<typeof selectedLayer>, changes: Partial<LayerSettings> = {}): LayerSettings => ({ id: layer.id, name: layer.name, opacity: layer.opacity, locked: layer.locked, alphaLocked: layer.alphaLocked, maskEnabled: layer.maskEnabled, maskInverted: layer.maskInverted, maskDensity: layer.maskDensity, ...changes });
  function submitDocument(event: FormEvent) {
    event.preventDefault();
    const factor = pixelsPerUnit(settings.unit, settings.resolution);
    onDocumentSettings({ ...settings, width: Math.max(1, Math.round(displayDimensions.width * factor)), height: Math.max(1, Math.round(displayDimensions.height * factor)) });
  }



  function movePanel(source: PanelId, target: PanelId) {
    if (source === target || source === 'color' || source === 'document' || target === 'color' || target === 'document') return;
    setOrder(current => {
      const targetIndex = current.indexOf(target);
      const next = current.filter(panel => panel !== source);
      next.splice(targetIndex, 0, source);
      return next;
    });
    setActivePanel(source);
  }

  function handleTabKeyDown(event: KeyboardEvent<HTMLButtonElement>, panel: PanelId) {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const index = order.indexOf(panel);
    const nextIndex = (index + (event.key === 'ArrowDown' ? 1 : -1) + order.length) % order.length;
    const nextPanel = order[nextIndex];
    if (event.altKey) movePanel(panel, nextPanel);
    else {
      setActivePanel(nextPanel);
      window.document.getElementById(`inspector-tab-${nextPanel}`)?.focus();
    }
  }

  return <aside className={`inspector${draggedPanel?' panel-drag-active':''}`} aria-label={t.properties}>
    <div className="dock-resizer" role="separator" aria-orientation="vertical" aria-label={locale==='ja'?'パネルの幅':locale==='en'?'Panel width':'面板宽度'} aria-valuemin={260} aria-valuemax={600} aria-valuenow={dockWidth} tabIndex={0} onPointerDown={e=>{e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);resizeDock.current={x:e.clientX,width:dockWidth};}} onPointerMove={e=>{if(resizeDock.current)setDockWidth(Math.max(260,Math.min(600,window.innerWidth*.6,resizeDock.current.width+resizeDock.current.x-e.clientX)));}} onPointerUp={()=>{resizeDock.current=null;}} onPointerCancel={()=>{resizeDock.current=null;}} onKeyDown={e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();setDockWidth(w=>Math.max(260,Math.min(600,w+(e.key==='ArrowLeft'?16:-16))));}}}/>
    <header className="dock-panel-header" draggable={false} onPointerDown={e=>panelPointerDown(e,activePanel)} onPointerMove={panelPointerMove} onPointerUp={panelPointerUp} onPointerCancel={()=>{panelDrag.current=null;setDraggedPanel(null);}}><strong>{floating[activePanel]?t.properties:labels[activePanel]}</strong><button disabled={!!floating[activePanel]} title={dockText.float} aria-label={dockText.float} onClick={()=>float(activePanel)}>↗</button><button title={dockText.reset} aria-label={dockText.reset} onClick={()=>{setFloating({});setDockWidth(308);setOrder([...panelIds]);setActivePanel('layers');}}>↺</button></header>
    <div className="inspector-tabs" role="tablist" aria-label={t.properties} aria-orientation="vertical">
      {order.map(panel => <Fragment key={panel}><button
        id={`inspector-tab-${panel}`}
        className={`tool-button inspector-tab${panel === 'document' ? ' inspector-tab-bottom' : ''}${activePanel === panel ? ' selected' : ''}${floating[panel] ? ' floating-tab' : ''}${draggedPanel === panel ? ' dragging' : ''}${dropTarget === panel && draggedPanel !== panel ? ' drop-target' : ''}`}
        role="tab"
        aria-selected={activePanel === panel}
        aria-controls={`inspector-panel-${panel}`}
        tabIndex={activePanel === panel ? 0 : -1}
        draggable={false}
        onPointerDown={e=>panelPointerDown(e,panel)} onPointerMove={panelPointerMove} onPointerUp={panelPointerUp} onPointerCancel={()=>{panelDrag.current=null;setDraggedPanel(null);}}
        title={labels[panel]}
        aria-label={labels[panel]}
        onClick={() => {if(suppressClick.current){suppressClick.current=false;return;} setSwatchDraft(undefined); if(floating[panel])dock(panel);else setActivePanel(panel); }}
        onKeyDown={event => handleTabKeyDown(event, panel)}
      ><Icon name={panelIcons[panel]} /></button>{panel === 'color' && <span className="inspector-tab-separator" aria-hidden="true" />}</Fragment>)}
    </div>

    {panelView('swatches',<section className="inspector-panel" role="tabpanel" id="inspector-panel-swatches" aria-labelledby="inspector-tab-swatches"><SwatchesPanel initialDraft={swatchDraft} locale={locale} document={document} enabled={enabled && !textEditing} brush={brush} backgroundColor={backgroundColor} onForegroundChange={onForegroundChange} onBackgroundChange={onBackgroundChange} vectorColors={vectorColors} onUpdate={onTransformUpdate}/></section>)}
    {panelView('pages',<section className="inspector-panel" role="tabpanel" id="inspector-panel-pages" aria-labelledby="inspector-tab-pages"><PagesPanel key={thumbnailDocumentKey} locale={locale} document={document} enabled={enabled} onUpdate={onTransformUpdate}/></section>)}
    {panelView('links',<section className="inspector-panel" role="tabpanel" id="inspector-panel-links" aria-labelledby="inspector-tab-links"><LinksPanel key={thumbnailDocumentKey} locale={locale} document={document} enabled={enabled} onUpdate={onTransformUpdate}/></section>)}
    {panelView('gradient',<section className="inspector-panel" role="tabpanel" id="inspector-panel-gradient" aria-labelledby="inspector-tab-gradient"><GradientPanel toolActive={gradientTool} onOpenSwatches={openSwatches} onRegister={gradient=>{setSwatchDraft({name:swatchLabels[locale].newGradient,paint:{kind:'gradient',gradient}});setActivePanel('swatches');}} locale={locale} document={document} enabled={enabled && !textEditing} onUpdate={onTransformUpdate} /></section>)}
    {panelView('pathfinder',<section className="inspector-panel" role="tabpanel" id="inspector-panel-pathfinder" aria-labelledby="inspector-tab-pathfinder"><PathfinderPanel locale={locale} document={document} enabled={enabled && !textEditing} onUpdate={onTransformUpdate} /></section>)}
    {panelView('transform',<section className="inspector-panel" role="tabpanel" id="inspector-panel-transform" aria-labelledby="inspector-tab-transform"><TransformPanel locale={locale} document={document} enabled={enabled && !textEditing} onUpdate={onTransformUpdate} /></section>)}
    {panelView('stroke',<section className="inspector-panel" role="tabpanel" id="inspector-panel-stroke" aria-labelledby="inspector-tab-stroke"><StrokePanel locale={locale} document={document} enabled={enabled} onChange={onStrokeWidth} onStyle={onStrokeStyle} /></section>)}
    {panelView('text',<section className="inspector-panel" role="tabpanel" id="inspector-panel-text" aria-labelledby="inspector-tab-text">
      <TextPanel locale={locale} settings={textSettings} resolution={document.resolution} enabled={textEnabled} editing={textEditing} onChange={onTextChange} onBegin={onTextBegin} onFinish={onTextFinish} />
    </section>)}

    {panelView('color',<section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-color" aria-labelledby="inspector-tab-color">
      <button onClick={openSwatches}>{swatchLink}</button>
      <ColorPanel vectorColors={vectorColors} locale={locale} color={brush.color} backgroundColor={backgroundColor} activeColor={activeColor} onSelectColor={onSelectColor} onChange={onForegroundChange} onBackgroundChange={onBackgroundChange} onSwap={onSwapColors} />
    </section>)}
    {panelView('brush',<section className="inspector-panel property-section" role="tabpanel" id="inspector-panel-brush" aria-labelledby="inspector-tab-brush">
      <button onClick={openSwatches}>{swatchLink}</button>
      <div className="diameter-row"><label htmlFor="brush-size">{t.diameter}</label><CompactSlider id="brush-size" min="1" max={MAX_BRUSH_SIZE} value={brush.size} onChange={event => onBrush({ ...brush, size: Number(event.target.value) })} /><SizeInput resolution={document.resolution} label={t.diameter} value={brush.size} onChange={size => onBrush({ ...brush, size })} /></div>
      <div className="diameter-row"><label htmlFor="brush-hardness">{t.hardness}</label><CompactSlider id="brush-hardness" min="0" max="100" value={Math.round(brush.hardness * 100)} onChange={event => onBrush({ ...brush, hardness: Number(event.target.value) / 100 })} /><PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => onBrush({ ...brush, hardness })} /></div>
      <BrushPresets locale={locale} brush={brush} enabled={enabled} onChange={onBrush} />
      <HexInput label={t.hex} invalid={t.invalidColor} color={brush.color} onChange={onForegroundChange} />
    </section>)}

    {panelView('document',<section className="inspector-panel document-settings-panel" role="tabpanel" id="inspector-panel-document" aria-labelledby="inspector-tab-document">
      <form className="document-settings-form" onSubmit={submitDocument}>
        <label className="document-name-field"><span>{t.documentName}</span><input disabled={!enabled} value={settings.name} maxLength={120} onChange={event => setSettings(value => ({ ...value, name: event.target.value }))} /></label>
        <div className="document-size-grid">
          <label><span>{t.width}</span><input disabled={!enabled} type="number" min={settings.unit === 'pixels' ? 1 : 0.001} step={settings.unit === 'pixels' ? 1 : 0.001} value={displayDimensions.width} onChange={event => setDisplayDimensions(value => ({ ...value, width: Number(event.target.value) }))} /></label>
          <label><span>{t.height}</span><input disabled={!enabled} type="number" min={settings.unit === 'pixels' ? 1 : 0.001} step={settings.unit === 'pixels' ? 1 : 0.001} value={displayDimensions.height} onChange={event => setDisplayDimensions(value => ({ ...value, height: Number(event.target.value) }))} /></label>
          <label className="document-unit"><span>{t.unit}</span><select disabled={!enabled} value={settings.unit} onChange={event => void setMeasurementUnit(event.target.value as DocumentSettings['unit']).catch(e=>setPathTabError(String(e)))}><option value="pixels">{t.pixels}</option><option value="inches">{t.inches}</option><option value="centimeters">{t.centimeters}</option><option value="millimeters">{t.millimeters}</option><option value="points">{unitName("points",locale)}</option></select></label>
        </div>
        <div className="document-direction"><span>{t.orientation}</span><button type="button" disabled={!enabled} aria-label={t.portrait} title={t.portrait} aria-pressed={displayDimensions.height >= displayDimensions.width} onClick={() => setDisplayDimensions(value => value.height >= value.width ? value : ({ width: value.height, height: value.width }))}><Icon name="portrait" /></button><button type="button" disabled={!enabled} aria-label={t.landscape} title={t.landscape} aria-pressed={displayDimensions.width > displayDimensions.height} onClick={() => setDisplayDimensions(value => value.width > value.height ? value : ({ width: value.height, height: value.width }))}><Icon name="landscape" /></button></div>
        <label className="document-check"><input disabled={!enabled} type="checkbox" checked={settings.artboards} onChange={event => setSettings(value => ({ ...value, artboards: event.target.checked }))} />{t.artboards}</label>
        <label><span>{t.resolution}</span><div className="field-with-unit"><input disabled={!enabled} type="number" min="1" max="1200" value={settings.resolution} onChange={event => setSettings(value => ({ ...value, resolution: Number(event.target.value) }))} /><span>{t.pixelsPerInch}</span></div></label>
        <div className="document-size-grid"><label><span>{t.colorMode}</span><select disabled={!enabled} value={document.colorMode} onChange={event => onColorMode(event.target.value as ColorMode)}><option value="rgb">RGB {t.color}</option><option value="cmyk">CMYK {t.color}</option></select></label><label><span>{t.bitDepth}</span><select disabled={!enabled} value={document.bitDepth} onChange={event => onBitDepth(Number(event.target.value) as BitDepth)}><option value="8">8 bit</option><option value="16">16 bit</option><option value="32">32 bit</option></select></label></div>
        <label><span>{t.canvasColor}</span><select disabled={!enabled} value={settings.canvasColor} onChange={event => setSettings(value => ({ ...value, canvasColor: event.target.value as DocumentSettings['canvasColor'] }))}><option value="white">{t.white}</option><option value="transparent">{t.transparent}</option></select></label>
        <details open><summary>{t.advancedOptions}</summary><label><span>{t.colorProfile}</span><select disabled={!enabled} value={document.colorProfile} onChange={event => onColorProfile(event.target.value as ColorProfile)}>{document.colorMode === 'rgb' ? <><option value="srgb">sRGB</option><option value="displayP3">Display P3</option><option value="adobeRgb1998">Adobe RGB (1998)</option></> : <option value="japanColor2001Coated">Japan Color 2001 Coated</option>}</select></label><label><span>{t.pixelAspectRatio}</span><select disabled={!enabled} value={settings.pixelAspectRatio} onChange={event => setSettings(value => ({ ...value, pixelAspectRatio: Number(event.target.value) }))}><option value="1">{t.squarePixels}</option><option value="1.2">1.2</option><option value="0.9">0.9</option></select></label></details>
        <button className="document-apply" disabled={!enabled} type="submit">{t.applyDocumentSettings}</button>
      </form>
    </section>)}

    {panelView('layers',<section className="inspector-panel layer-panel" role="tabpanel" id="inspector-panel-layers" aria-labelledby="inspector-tab-layers">
      <div className="layer-subtabs"><button className={layerPanelMode === 'layers' ? 'active' : ''} onClick={() => void switchLayerPanel('layers')}>{t.layers}</button><button className={layerPanelMode === 'channels' ? 'active' : ''} onClick={() => void switchLayerPanel('channels')}>{t.channels}</button><button className={layerPanelMode === 'paths' ? 'active' : ''} onClick={() => setLayerPanelMode('paths')}>{t.paths}</button></div>
      {pathTabError && <p role="alert">{pathTabError}</p>}
      {layerPanelMode === 'paths' ? <PathsPanel document={document} locale={locale} enabled={enabled} onAction={onSavedPathAction} /> : layerPanelMode === 'channels' ? <ChannelsPanel thumbnails={thumbnails.channels} thumbnailError={thumbnails.error} locale={locale} mode={document.colorMode} value={channel} enabled={enabled} onChange={onChannel} /> : <>
        <div className="layer-compositing"><label><span>{t.layerBlendMode}</span><select disabled><option>{t.normalBlend}</option></select></label><label><span>{t.layerOpacity}</span><div><CompactSlider disabled={!enabled || !selectedLayer} min="0" max="100" value={Math.round((selectedLayer?.opacity ?? 1) * 100)} onChange={event => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { opacity: Number(event.target.value) / 100 }))} /><output>{Math.round((selectedLayer?.opacity ?? 1) * 100)}%</output></div></label></div>
        <div className="layer-lock-row"><span>{t.lockLayer}</span><button type="button" disabled={!enabled || !selectedLayer} className={selectedLayer?.locked ? 'active' : ''} aria-pressed={selectedLayer?.locked ?? false} title={selectedLayer?.locked ? t.unlockLayer : t.lockLayer} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { locked: !selectedLayer.locked }))}>▣</button><button type="button" disabled={!enabled || !selectedLayer || selectedLayer.kind !== 'paint'} className={selectedLayer?.alphaLocked ? 'active' : ''} aria-pressed={selectedLayer?.alphaLocked ?? false} title={t.lockAlpha} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { alphaLocked: !selectedLayer.alphaLocked }))}>α</button><span className="layer-fill">{t.layerFill}: 100%</span></div>
        {selectedLayer?.maskEnabled && <div className="mask-controls"><label><span>{t.maskDensity}</span><CompactSlider min="0" max="100" value={Math.round(selectedLayer.maskDensity * 100)} onChange={event => onLayerSettings(layerSettings(selectedLayer, { maskDensity: Number(event.target.value) / 100 }))} /><output>{Math.round(selectedLayer.maskDensity * 100)}%</output></label><button className={selectedLayer.maskInverted ? 'active' : ''} onClick={() => onLayerSettings(layerSettings(selectedLayer, { maskInverted: !selectedLayer.maskInverted }))}>{t.invertMask}</button></div>}
        <LayerList thumbnails={Object.fromEntries(thumbnails.layers)} thumbnailError={thumbnails.error} layers={document.layers} textObjects={document.textObjects} selectedId={selectedLayerId} enabled={enabled} locale={locale}
          selectedObjects={document.selectedVectorObjects} onSelectObject={onSelectObject} onToggleObject={onToggleObject} onReorderObjects={onReorderObjects} onSelect={onSelectLayer} onToggle={onToggleLayer} onReorder={onReorderLayer}
          onToggleLock={layer => onLayerSettings(layerSettings(layer, { locked: !layer.locked }))}
          onRename={(layer, name) => onLayerSettings(layerSettings(layer, { name }))} />
        <div className="layer-actions">
          <div className="layer-action-group">
            <button disabled={!enabled} title={t.addPixelLayer} aria-label={t.addPixelLayer} onClick={onAddLayer}>＋</button>
            <button type="button" className="add-vector-layer" disabled={!enabled} title={t.addVectorLayer} aria-label={t.addVectorLayer} onClick={onAddVectorLayer}><Icon name="vector" /><span aria-hidden="true">＋</span></button>
            <button disabled={!enabled || !selectedLayer?.deletable} title={t.deleteLayer} aria-label={t.deleteLayer} onClick={() => selectedLayer && onDeleteLayer(selectedLayer.id)}>⌫</button>
          </div>
          <span className="layer-action-divider" aria-hidden="true" />
          <div className="layer-action-group">
            <button disabled title={t.layerOptions} aria-label={t.layerOptions}>fx</button>
            <button disabled={!enabled || !selectedLayer} className={selectedLayer?.maskEnabled ? 'active' : ''} title={selectedLayer?.maskEnabled ? t.removeMask : t.addMask} aria-label={selectedLayer?.maskEnabled ? t.removeMask : t.addMask} onClick={() => selectedLayer && onLayerSettings(layerSettings(selectedLayer, { maskEnabled: !selectedLayer.maskEnabled, maskDensity: 1, maskInverted: false }))}>◐</button>
          </div>
        </div><p className="layer-count">{document.layers.length} {t.layerUnit}<span>{document.strokeCount} {t.strokes}</span></p>
      </>}
    </section>)}
  </aside>;
}
