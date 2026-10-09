import { useFileDrop, type FileDrop } from './file-drop';
import { dropFiles } from './bridge';
import { BrushSettings } from './components/BrushSettings';
import {ShortcutDialog} from './components/ShortcutDialog';
import {useShortcutCommands,useShortcutInputBlocked,useShortcuts,keyFromAccelerator,commandShortcut} from './shortcuts';
import {toolCommands} from './tool-shortcuts';
import { LayerMaskDialog } from './components/LayerMaskDialog';
import {SavedSelectionsDialog} from './components/SavedSelectionsDialog';
import {vectorSelectionAction} from './bridge';
import {RetouchControls,retouchTools,isRetouch} from './components/RetouchControls';
import { SelectionPathControls, pathSelectionTools, isPathSelection } from './components/SelectionPathControls';
import { PaintBucketControls } from './components/PaintBucketControls';
import { CloneStampControls } from './components/CloneStampControls';
import { setLayerEffects, setRasterBlendMode, type RasterBlendMode } from './bridge';
import { invoke } from '@tauri-apps/api/core';
import { DocumentTabMenu } from './components/DocumentTabMenu';
import { moveDocumentToWindow, openDocumentView } from './bridge';
import { GuideOptions } from './components/GuideOptions';
import { editGuides,editLayerGroups } from './bridge';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from './measurement-units';
import {placeImage,subscribePlaceImage} from './bridge';
import { ToolSettingsDialog, toolSettingsLabels } from './components/ToolSettingsDialog';
import { lazy, Suspense } from 'react';
import { ToneStudio, toneStudioLabels } from './components/ToneStudio';
import { DocumentDock } from './components/DocumentDock';
import { NativeModal } from './NativeModal';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { DirectControlDialog, directControlLabels } from './components/DirectControlDialog';
import { ImportImageDialog } from './components/ImportImageDialog';
import { importRasterLayer, finishRasterImport, subscribeRasterPlacement } from './bridge';
import { SelectionOptions } from './components/SelectionOptions';
import { ColorPickerPopover, colorPickerVisibilityEvent, type ColorPickerOcclusion } from './components/ColorPickerPopover';
import { setVectorPaint, setVectorAppearance, savedPathAction } from './bridge';
import type { VectorColorControls } from './components/ColorPanel';
import { TransformDialog, transformLabels } from './components/TransformDialog';
import { transformObjects, type TransformAction } from './bridge';
import { clippingPath, compoundPath, outlineText, outlineView, setTextWritingMode } from './bridge';
import { IconToolMenu, type IconToolChoice } from './components/IconToolMenu';
import { arrangeSelectedVectors, setVectorStrokeStyle, setVectorStrokeWidth, reorderVectorObjects, selectVectorObjects, setVectorObjectVisibility, selectLayer, selectLayerTarget, selectChannel, addVectorLayer } from './bridge';
import { useCallback, useEffect, useRef, useState, type SetStateAction } from 'react';
import { CanvasPreview } from './CanvasPreview';
import { subscribeCanvasZoom, newEditorWindow } from './bridge';
import { beginTextEdit, updateTextEdit, setTextEditColor, finishTextEdit, subscribeTextSession, defaultVectorText, type TextSettings, subscribeCanvasText, subscribeCanvasColorSwap, subscribeCanvasSampledColor, subscribeCanvasTool, type CanvasTool, type DocumentEditAction, addPaintLayer, changeBitDepth, changeColorMode, changeColorProfile, changeDocumentSettings, closeDocument, combineSelectedVectors, groupSelectedVectors, ungroupSelectedVectors, editSelectedPaths, createDocument, deleteLayer, editDocument, getDocumentWorkspace, importSvgLayer, projectAction, reorderLayers, switchDocument, toggleLayer, updateLayer, emptyDocument, subscribeDocument, subscribeDocuments, type BitDepth, type Brush, type ColorMode, type ColorProfile, type DocumentSettings, type DocumentSnapshot, type DocumentTabSnapshot, type DocumentWorkspaceSnapshot, type LayerSettings, type PathEditAction, type PathOperation } from './bridge';
import { initialLocale, initialTheme, messages, readPreference, savePreference, type Locale, type Theme } from './i18n';
import { workspaceMessages } from './workspace-i18n';
import { RecoveryControls } from './components/RecoveryControls';
import { WorkspaceMenu } from './components/WorkspaceMenu';
import { AppMenu } from './components/AppMenu';
import { SettingsDialog } from './components/SettingsDialog';
import { ColorSettingsDialog } from './components/ColorSettingsDialog';
import { Inspector } from './components/Inspector';
import { Icon } from './components/Icon';
import { ZoomToolMenu, type ZoomTool } from './components/ZoomToolMenu';
import { SelectionToolMenu, type SelectionTool } from './components/SelectionToolMenu';
import { VectorShapeToolMenu, type VectorShapeTool } from './components/VectorShapeToolMenu';
import { textPanelMessages } from './text-panel-i18n';
import { textMessages } from './text-i18n';
import { ToolModeSwitch } from './components/ToolModeSwitch';
import { initialTools, isPenTool, penTools, type PenTool, modeForTool, modeLabels, modeTools, type ToolMode } from './tool-modes';
import { PercentInput, SizeInput } from './components/BrushControls';
import { type ColorTarget } from './components/ColorPanel';
import { NewDocumentDialog } from './components/NewDocumentDialog';
import { subscribeNewDocument, type NewDocumentSettings, type DisplayChannel } from './bridge';

const mediaBrowserTitle = { ja: '画像・動画ブラウザー', en: 'Media browser', 'zh-CN': '图片与视频浏览器' };
const MediaBrowser = lazy(() => import('./components/MediaBrowser').then(module => ({ default: module.MediaBrowser })));

export function Workspace() {
  const [mediaBrowserOpen,setMediaBrowserOpen]=useState(false);
  const closeMediaBrowser=useCallback(()=>setMediaBrowserOpen(false),[]);
  const [toneStudioOpen,setToneStudioOpen]=useState(false);
  const [toneStudioLoaded,setToneStudioLoaded]=useState(false);
  const [colorPickerOcclusion, setColorPickerOcclusion] = useState<ColorPickerOcclusion | null>(null);
  useEffect(() => {
    const update = (event: Event) => {
      const detail = (event as CustomEvent<{ open?: boolean; rect?: ColorPickerOcclusion }>).detail;
      setColorPickerOcclusion(detail?.open && detail.rect ? detail.rect : null);
    };
    window.addEventListener(colorPickerVisibilityEvent, update);
    return () => window.removeEventListener(colorPickerVisibilityEvent, update);
  }, []);
  const [outlineDisplay, setOutlineDisplay] = useState(false);
  useEffect(() => { void outlineView().then(setOutlineDisplay).catch(() => {}); }, []);

  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [theme, setTheme] = useState<Theme>(() => readPreference('theme') ? initialTheme() : 'dark');
  const [toolState, setToolState] = useState({ mode: 'paint' as ToolMode, tools: initialTools });
  const toolMode = toolState.mode;
  const [linksPanelRequest,setLinksPanelRequest]=useState(0);
  const [lastFrameTool,setLastFrameTool]=useState<'imageFrameRectangle'|'imageFrameEllipse'>('imageFrameRectangle');
  const [cropTool,setCropTool]=useState(false);
  const [gradientTool,setGradientTool]=useState(false);
  const [gradientPanelRequest,setGradientPanelRequest]=useState(0);
  const gradientLabel=locale==='ja'?'グラデーション':locale==='en'?'Gradient':'渐变';
  const showGradientPanel=()=>{setPanels(true);setGradientPanelRequest(n=>n+1);};
  const [sampling, setSampling] = useState(false);
  const [zoomTool, setZoomTool] = useState<ZoomTool | null>(null);
  const [lastZoomTool, setLastZoomTool] = useState<ZoomTool>('zoomIn');
  const [transformTool, setTransformTool] = useState<'vectorScale' | 'vectorRotate' | 'vectorSelect' | 'vectorDirectSelect' | null>(null);
  const [layerMaskTarget,setLayerMaskTarget]=useState<{id:string;documentId:number|null}|null>(null);
  const [savedSelectionDialog,setSavedSelectionDialog]=useState<'save'|'edit'|null>(null);
  const [selectionTool, setSelectionTool] = useState<SelectionTool | null>(null);
  const canvasTool: CanvasTool = cropTool ? 'crop' : gradientTool ? 'gradient' : sampling ? 'eyedropper' : zoomTool ?? transformTool ?? selectionTool ?? toolState.tools[toolMode];
  const [lastSelectionTool, setLastSelectionTool] = useState<SelectionTool>('rectangle');
  const [toolSettings,setToolSettings]=useState<CanvasTool|null>(null);
  const openToolSettings=(tool:string)=>{if(tool==="gradient"){showGradientPanel();}else if(tool==="vectorScale"||tool==="vectorRotate"){setTransformAction(tool==="vectorScale"?"scale":"rotate");}else if(tool==="vectorSelect"){setTransformAction("move");}else if(tool==="vectorDirectSelect"||tool.startsWith("vectorAnchor")){setDirectControlOpen(true);}else{setToolSettings(tool as CanvasTool);}};
  const [directControlOpen,setDirectControlOpen] = useState(false);
  const [transformAction, setTransformAction] = useState<TransformAction | null>(null);
  const [lastTransformTool, setLastTransformTool] = useState<'vectorScale' | 'vectorRotate'>('vectorScale');
  const [lastVectorSelectTool, setLastVectorSelectTool] = useState<'vectorSelect' | 'vectorDirectSelect'>('vectorSelect');
  const [lastPenTool, setLastPenTool] = useState<PenTool>('vectorPen');
  const [lastVectorShapeTool, setLastVectorShapeTool] = useState<VectorShapeTool>('vectorRectangle');
  const [lastTextTool, setLastTextTool] = useState<'text' | 'textVertical' | 'textFrame' | 'textFrameVertical'>('text');
  const setToolMode = useCallback((mode: ToolMode) => {
    setCropTool(false);
    setGradientTool(false);
    setSampling(false);
    setZoomTool(null);
    setToolState(current => ({ ...current, mode }));
  }, []);
  const setTool = useCallback((next: CanvasTool) => {
    setCropTool(next === 'crop');
    if(next==='crop'){setGradientTool(false);setSampling(false);setSelectionTool(null);setTransformTool(null);setZoomTool(null);return;}
    setGradientTool(next === 'gradient');
    if(next==='gradient'){setSampling(false);setSelectionTool(null);setTransformTool(null);setZoomTool(null);return;}
    setSampling(next === 'eyedropper');
    if (next === 'eyedropper') { setSelectionTool(null); setTransformTool(null); setZoomTool(null); return; }
    if (next === 'zoomIn' || next === 'zoomOut' || next === 'hand') { setSelectionTool(null); setTransformTool(null); setZoomTool(next); setLastZoomTool(next); return; }
    setZoomTool(null);
    if (next === 'vectorScale' || next === 'vectorRotate' || next === 'vectorSelect' || next === 'vectorDirectSelect') { setSelectionTool(null); setTransformTool(next); if(next === 'vectorScale' || next === 'vectorRotate') setLastTransformTool(next); else setLastVectorSelectTool(next); return; }
    setTransformTool(null);
    if (next === 'rectangle' || next === 'ellipse') { setSelectionTool(next); setLastSelectionTool(next); return; }
    setSelectionTool(null);
    setToolState(current => {
      const mode = modeForTool(current.mode, next);
      return { mode, tools: { ...current.tools, [mode]: next } };
    });
    if(next==='imageFrameRectangle'||next==='imageFrameEllipse')setLastFrameTool(next);
    if (isPenTool(next)) setLastPenTool(next);
    if (next === 'vectorRectangle' || next === 'vectorEllipse') setLastVectorShapeTool(next);
    if (next === 'text' || next === 'textVertical' || next === 'textFrame' || next === 'textFrameVertical') setLastTextTool(next);
  }, []);
  const [paintState, setPaintState] = useState<{ brush: Brush; backgroundColor: Brush['color']; backgroundNone?: boolean }>({
    brush: { size: 16, hardness: 1, color: [32, 32, 32] }, backgroundColor: [255, 255, 255],
  });
  useEffect(()=>{if(!isTauri())return;const listener=getCurrentWebviewWindow().listen<Brush>('canvas-brush-changed',event=>setBrush(event.payload));return()=>{void listener.then(fn=>fn());};},[]);
  const [activeColor, setActiveColor] = useState<ColorTarget>('foreground');
  const colorPanelRequest = 0;
  const { brush, backgroundColor, backgroundNone = false } = paintState;
  const setBrush = useCallback((next: SetStateAction<Brush>) => {
    setPaintState(current => ({ ...current, brush: typeof next === 'function' ? next(current.brush) : next }));
  }, []);
  const setBackgroundColor = useCallback((color: Brush['color']) => {
    setPaintState(current => ({ ...current, backgroundColor: color, backgroundNone: false }));
  }, []);
  const measurementUnit=useMeasurementUnit();
  const [documentState, setDocumentState] = useState(emptyDocument);
  const horizontalResolution = documentState.rasterResolution?.xPpi ?? documentState.resolution;
  const verticalResolution = documentState.rasterResolution?.yPpi ?? documentState.resolution;
  const [documents, setDocuments] = useState<DocumentTabSnapshot[]>([]);
  const [activeDocumentId, setActiveDocumentId] = useState<number | null>(null);
  const [ready, setReady] = useState(false);
  const [documentAvailable, setDocumentAvailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fileBusy, setFileBusy] = useState(false);
  const filePending = useRef(false);
  const [error, setError] = useState('');
  const [tabMenu,setTabMenu]=useState<{id:number;x:number;y:number}|null>(null);
  const [zoom, setZoom] = useState(1);
  const [zoomCommand, setZoomCommand] = useState({ zoom: 0, revision: 0 });
  const changeZoom = useCallback((next: number) => {
    if (next > 0) setZoom(next);
    setZoomCommand(current => ({ zoom: next, revision: current.revision + 1 }));
  }, []);
  const channel: DisplayChannel = documentState.editingChannel ?? 0;
  const [shortcutsOpen,setShortcutsOpen]=useState(false);
  useShortcuts();
  const [panels, setPanels] = useState(() => window.innerWidth > 720);
  const [inlineText, setInlineText] = useState<TextSettings | null>(null);
  const textSessionActive = useRef(false);
  const textSessionId = useRef<string | null>(null);
  const [textPanelRequest, setTextPanelRequest] = useState(0);
  const showTextPanel = useCallback(() => { setPanels(true); setTextPanelRequest(value => value + 1); }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [placingImage, setPlacingImage] = useState(false);
  const [placementBusy, setPlacementBusy] = useState(false);
  const [importImageOpen, setImportImageOpen] = useState(false);
  const [pdfImportToken, setPdfImportToken] = useState<number | null>(null);
  const [newDocumentOpen, setNewDocumentOpen] = useState(false);
  const [colorSettingsOpen, setColorSettingsOpen] = useState(false);
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const openColorSettings = useCallback(() => setColorSettingsOpen(true), []);
  const closeColorSettings = useCallback(() => setColorSettingsOpen(false), []);
  const t = workspaceMessages[locale];
  const common = messages[locale];
  const documentEditable = !placingImage && documentAvailable && documents.find(document => document.id === activeDocumentId)?.format !== 'tiled';
  const updateDocument = useCallback((next: DocumentSnapshot) => {
    setDocumentState(next);
  }, []);
  const placeLinkedImage=useCallback(async()=>{if(fileBusy||!documentAvailable)return;setFileBusy(true);try{updateDocument(await placeImage());setPanels(true);setLinksPanelRequest(n=>n+1);}catch(e){setError(String(e));}finally{setFileBusy(false);}},[fileBusy,documentAvailable,updateDocument]);
  useEffect(()=>{let live=true;let stop=()=>{};void subscribePlaceImage(()=>{if(live)void placeLinkedImage();}).then(f=>{if(live)stop=f;else f();});return()=>{live=false;stop();};},[placeLinkedImage]);
  const updateWorkspace = useCallback((next: DocumentWorkspaceSnapshot) => {
    setDocuments(next.documents);
    setActiveDocumentId(next.activeId);
    setDocumentAvailable(next.active !== null);
    setDocumentState(next.active ?? emptyDocument);
  }, []);

  const acceptFileDrop = useCallback(async ({ paths, targetId }: FileDrop) => {
    if (filePending.current || fileBusy) {
      setError({ja:'他のファイル操作が進行中です',en:'Another file operation is in progress','zh-CN':'正在进行其他文件操作'}[locale]);
      return;
    }
    filePending.current = true; setFileBusy(true); setError('');
    try {
      const result = await dropFiles(paths, targetId);
      updateWorkspace(result.workspace);
      if (result.errors.length) setError(result.errors.join('\n'));
      if (targetId === null && result.workspace.active) changeZoom(0);
    } catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [fileBusy, locale, updateWorkspace, changeZoom]);
  useFileDrop(activeDocumentId, acceptFileDrop, setError);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true; let stop = () => {};
    void getCurrentWebviewWindow().listen('memory-settings-changed', () => {
      void getDocumentWorkspace().then(next => { if (live) updateWorkspace(next); }).catch(cause => { if (live) setError(String(cause)); });
    }).then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop(); };
  }, [updateWorkspace]);

  useEffect(() => { document.documentElement.lang = locale; savePreference('locale', locale); }, [locale]);
  useEffect(() => { document.documentElement.dataset.theme = theme; savePreference('theme', theme); }, [theme]);
  useEffect(() => {
    const media = matchMedia('(max-width: 720px)');
    const update = () => setPanels(!media.matches);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);
  useEffect(() => {
    const preventPanelPan = (event: WheelEvent) => {
      const target = event.target as Element | null;
      if (target?.closest('.inspector') && Math.abs(event.deltaX) > Math.abs(event.deltaY)) {
        event.preventDefault();
      }
    };
    document.addEventListener('wheel', preventPanelPan, { passive: false });
    return () => document.removeEventListener('wheel', preventPanelPan);
  }, []);
  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeDocument(next => { if (active) updateDocument(next); }, message => { if (active) setError(message); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [updateDocument]);
  useEffect(() => {
    let active = true;
    let stop = () => {};
    getDocumentWorkspace().then(next => { if (active) updateWorkspace(next); }).catch(cause => { if (active) setError(String(cause)); });
    subscribeDocuments(next => { if (active) { updateWorkspace(next); if (next.active) setNewDocumentOpen(false); } })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [updateWorkspace]);

  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeNewDocument(() => { if (active) setNewDocumentOpen(true); }).then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); }).catch(cause => setError(String(cause)));
    return () => { active = false; stop(); };
  }, []);

  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeCanvasTool(next => {
      if (!active) return;
      setTool(next);
      if(next==='gradient'){setPanels(true);setGradientPanelRequest(n=>n+1);}
    })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, []);

  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeCanvasZoom(next => { if (active) setZoom(next); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, []);

  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeCanvasText(() => {
      if (active && documentAvailable && !fileBusy && !settingsOpen && !colorSettingsOpen) { setTool('text'); showTextPanel(); }
    }).then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [documentAvailable, fileBusy, settingsOpen, colorSettingsOpen, setTool, showTextPanel]);

  useEffect(() => {
    let active = true; let stop = () => {}; let wasEditing = false;
    subscribeTextSession(settings => {
      if (!active) return;
      textSessionActive.current = settings !== null;
      textSessionId.current = settings?.id ?? null;
      setInlineText(settings);
      if (settings && !wasEditing) showTextPanel();
      wasEditing = settings !== null;
    }).then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); }).catch(cause => setError(String(cause)));
    return () => { active = false; stop(); };
  }, [showTextPanel]);
  const applyTextColor = useCallback((color: Brush['color']) => {
    if (textSessionActive.current) {
      void setTextEditColor(textSessionId.current, color).catch(cause => setError(String(cause)));
    }
  }, []);
  const changeForeground = useCallback((color: Brush['color']) => {
    setBrush(previous => ({ ...previous, color, noColor: false }));
    applyTextColor(color);
  }, [setBrush, applyTextColor]);
  useEffect(() => {
    let active = true; let stop = () => {};
    subscribeCanvasSampledColor(color => { if (active) changeForeground(color); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [changeForeground]);
  const swapColors = useCallback(() => {
    setPaintState(current => ({
      brush: { ...current.brush, color: current.backgroundColor, noColor: current.backgroundNone ?? false }, backgroundColor: current.brush.color, backgroundNone: current.brush.noColor ?? false,
    }));
    if (inlineText && backgroundNone) {
      void updateTextEdit({ ...inlineText, stylePatch: { noColor: true } }).then(updateDocument).catch(cause => setError(String(cause)));
    } else applyTextColor(backgroundColor);
  }, [applyTextColor, backgroundColor, backgroundNone, inlineText, updateDocument]);
  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeCanvasColorSwap(() => { if (active) swapColors(); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [swapColors]);
  useEffect(() => {
    let active = true; let stop = () => {};
    subscribeRasterPlacement(value => { if (active) setPlacingImage(value); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => setError(String(cause)));
    return () => { active = false; stop(); };
  }, []);
  const finishPlacement = useCallback(async (commit: boolean) => {
    if (placementBusy) return;
    setPlacementBusy(true); setError('');
    try { updateDocument(await finishRasterImport(commit)); }
    catch (cause) { setError(String(cause)); }
    finally { setPlacementBusy(false); }
  }, [placementBusy, updateDocument]);
  const selectedPaths = documentState.layers.flatMap(layer=>layer.objects).filter(object=>documentState.selectedVectorObjects.includes(object.id));
  const vectorColorMode = !inlineText && selectedPaths.length > 0 && selectedPaths.every(object=>object.kind!=='text');
  const pathColor = (key:'fillColor'|'strokeColor') => (selectedPaths[0]?.[key]?.slice(0,3) ?? [0,0,0]) as Brush['color'];
  const pathStatus = (key:'fillColor'|'strokeColor'): 'none'|'mixed'|null => selectedPaths.some(object=>JSON.stringify(object[key])!==JSON.stringify(selectedPaths[0]?.[key])) ? 'mixed' : selectedPaths[0]?.[key] == null ? 'none' : null;
  const vectorColors: VectorColorControls | undefined = vectorColorMode ? {
    fill:pathColor('fillColor'),stroke:pathColor('strokeColor'),fillStatus:pathStatus('fillColor'),strokeStatus:pathStatus('strokeColor'),
    change:(target,color)=>{void setVectorPaint([...documentState.selectedVectorObjects],target,color).then(updateDocument).catch(cause=>setError(String(cause)));},
    swap:()=>{void setVectorPaint([...documentState.selectedVectorObjects],'swap',null).then(updateDocument).catch(cause=>setError(String(cause)));},
  } : undefined;
  const selectedText = documentState.textObjects.find(item => documentState.selectedVectorObjects.includes(item.id)) ?? null;
  const activeText = inlineText ?? selectedText;
  const changeText = useCallback(async (settings: TextSettings) => {
    if (!textSessionActive.current) {
      textSessionActive.current = true;
      try {
        await beginTextEdit(activeText ?? settings);
      } catch (cause) {
        textSessionActive.current = false;
        throw cause;
      }
    }
    updateDocument(await updateTextEdit(settings));
  }, [activeText, updateDocument]);
  const foregroundNone = activeText ? !!(activeText.selection?.style?.noColor ?? activeText.text.runs?.[0]?.style.noColor ?? activeText.text.noColor) : !!brush.noColor;
  const changeForegroundNone = () => {
    if (vectorColors) { vectorColors.change('fill', null); return; }
    if (activeText) {
      void changeText({ ...activeText, stylePatch: { noColor: true } }).catch(cause => setError(String(cause)));
    } else setBrush(previous => ({ ...previous, noColor: true }));
  };
  const changeBackgroundNone = () => {
    if (vectorColors) vectorColors.change('stroke', null);
    else setPaintState(previous => ({ ...previous, backgroundNone: true }));
  };
  const changePaintForeground = (color: Brush['color']) => {
    if (vectorColors) vectorColors.change('fill', color);
    else if (activeText) void changeText({ ...activeText, stylePatch: { color, noColor: false } }).catch(cause => setError(String(cause)));
    else changeForeground(color);
  };
  const changePaintBackground = (color: Brush['color']) => vectorColors ? vectorColors.change('stroke', color) : setBackgroundColor(color);
  const endText = useCallback((commit: boolean) => {
    void finishTextEdit(commit).then(snapshot => {
      textSessionActive.current = false;
      updateDocument(snapshot);
    }).catch(cause => setError(String(cause)));
  }, [updateDocument]);
  const beginText = () => {
    const vertical = canvasTool === 'textVertical' || canvasTool === 'textFrameVertical';
    const pointText = canvasTool !== 'textFrame' && canvasTool !== 'textFrameVertical';
    const settings: TextSettings = activeText ?? { id: null, text: { ...defaultVectorText, pointText, noColor: brush.noColor,
      writingMode: vertical ? 'vertical' : 'horizontal',
      boxWidth: vertical ? defaultVectorText.fontSize * defaultVectorText.lineHeight : defaultVectorText.boxWidth,
      boxHeight: pointText ? null : vertical ? defaultVectorText.boxWidth : 240,
      content: textMessages[locale].defaultText }, position: [48, 48], color: brush.color };
    textSessionActive.current = true;
    void beginTextEdit(settings).catch(cause => {
      textSessionActive.current = false;
      setError(String(cause));
    });
  };

  const edit = useCallback(async (action: DocumentEditAction) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await editDocument(action)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);

  const combineVectors = useCallback(async (operation: PathOperation) => {
    if (!ready || busy || (documentState.selectedVectorObjects.length < 2 || documentState.selectedVectorObjects.length > 64)) return;
    setBusy(true); setError('');
    try { updateDocument(await combineSelectedVectors(operation)); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, documentState.selectedVectorObjects.length, updateDocument]);
  const changeGroup = useCallback(async (action: 'group' | 'ungroup' | 'ungroupAll') => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(action === 'group' ? await groupSelectedVectors() : await ungroupSelectedVectors(action === 'ungroupAll')); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);
  const editPath = useCallback(async (action: PathEditAction) => {
    if (!ready || busy || documentState.selectedVectorObjects.length === 0) return;
    setBusy(true); setError('');
    try { updateDocument(await editSelectedPaths(action)); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, documentState.selectedVectorObjects.length, updateDocument]);

  const setColorMode = useCallback(async (mode: ColorMode) => {
    if (!ready || busy || documentState.colorMode === mode) return;
    setBusy(true); setError('');
    try { updateDocument(await changeColorMode(mode)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, documentState.colorMode, updateDocument]);

  const setBitDepth = useCallback(async (depth: BitDepth) => {
    if (!ready || busy || documentState.bitDepth === depth) return;
    setBusy(true); setError('');
    try { updateDocument(await changeBitDepth(depth)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, documentState.bitDepth, updateDocument]);

  const setColorProfile = useCallback(async (profile: ColorProfile) => {
    if (!ready || busy || documentState.colorProfile === profile) return;
    setBusy(true); setError('');
    try { updateDocument(await changeColorProfile(profile)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, documentState.colorProfile, updateDocument]);
  const setDocumentSettings = useCallback(async (settings: DocumentSettings) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await changeDocumentSettings(settings)); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);

  const file = useCallback(async (action: 'open' | 'save' | 'saveAs' | 'export') => {
    if ((action !== 'open' && !documentAvailable) || filePending.current) return;
    filePending.current = true; setFileBusy(true); setError('');
    try { await projectAction(action); updateWorkspace(await getDocumentWorkspace()); }
    catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [documentAvailable, updateWorkspace]);

  const documentAction = useCallback(async (action: 'new' | 'switch' | 'close', id?: number) => {
    if (filePending.current) return;
    if (action === 'new') { setNewDocumentOpen(true); return; }
    filePending.current = true; setFileBusy(true); setError('');
    try {
      const next = action === 'switch' ? await switchDocument(id!) : await closeDocument(id!);
      updateWorkspace(next);
    } catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [updateWorkspace]);

  const createFromPreset = useCallback(async (settings: NewDocumentSettings) => {
    if (filePending.current) throw new Error({ja:'他のファイル操作が進行中です',en:'Another file operation is in progress','zh-CN':'正在进行其他文件操作'}[locale]);
    filePending.current = true; setFileBusy(true);
    try { updateWorkspace(await createDocument(settings)); changeZoom(0); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [updateWorkspace, changeZoom]);

  const importSvg = useCallback(async () => {
    if (!documentEditable || filePending.current) return;
    filePending.current = true; setFileBusy(true); setError('');
    try { updateDocument(await importSvgLayer()); }
    catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [documentEditable, updateDocument]);

  const setLayerVisibility = useCallback(async (id: string) => {
    if (!ready || busy || fileBusy) return;
    setBusy(true); setError('');
    try { updateDocument(await toggleLayer(id)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, fileBusy, updateDocument]);
  const changeRasterBlendMode = useCallback(async (id: string, mode: RasterBlendMode) => {
    if (!ready || busy || fileBusy) return;
    setBusy(true); setError('');
    try { updateDocument(await setRasterBlendMode(id, mode)); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, fileBusy, updateDocument]);
  const setLayerSettings = useCallback(async (settings: LayerSettings) => {
    if (!ready || busy || fileBusy) return;
    setBusy(true); setError('');
    try { updateDocument(await updateLayer(settings)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, fileBusy, updateDocument]);
  const removeLayer = useCallback(async (id: string) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await deleteLayer(id)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);
  const createLayer = useCallback(async (kind: 'paint' | 'vector') => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await (kind === 'paint' ? addPaintLayer() : addVectorLayer())); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);
  const moveLayer = useCallback(async (ids: string[]) => {
    if (!ready || busy || fileBusy) return;
    setBusy(true); setError('');
    try { updateDocument(await reorderLayers(ids)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, fileBusy, updateDocument]);

  useShortcutInputBlocked(placingImage||mediaBrowserOpen||toneStudioOpen||settingsOpen||colorSettingsOpen||newDocumentOpen||importImageOpen||directControlOpen||!!transformAction||shortcutsOpen||!!toolSettings||!!layerMaskTarget||!!savedSelectionDialog);
  useShortcutCommands('tools',[
    ...toolCommands(locale,documentEditable&&!placingImage&&!shortcutsOpen,tool=>{setTool(tool);if(tool==='text')showTextPanel();if(tool==='gradient')showGradientPanel();}),
    {id:'layer.addPaint',label:{ja:'新規ピクセルレイヤー',en:'New Pixel Layer','zh-CN':'新建像素图层'}[locale],category:t.layers,defaultKey:'Primary+Alt+Shift+KeyN',enabled:documentEditable,action:()=>void createLayer('paint')},
    {id:'layer.addVector',label:{ja:'新規ベクターレイヤー',en:'New Vector Layer','zh-CN':'新建矢量图层'}[locale],category:t.layers,defaultKey:'Primary+Alt+Shift+KeyV',enabled:documentEditable,action:()=>void createLayer('vector')},
    {id:'layer.mask',label:{ja:'レイヤーマスク設定',en:'Layer Mask Settings','zh-CN':'图层蒙版设置'}[locale],category:t.layers,defaultKey:'Primary+Alt+Shift+KeyM',enabled:documentEditable,action:()=>setLayerMaskTarget({id:documentState.layerId,documentId:activeDocumentId})},
    ...([0,1,2,3,4] as const).map(value=>({id:`channel.${value}`,label:({ja:['合成','レッド','グリーン','ブルー','アルファ'],en:['Composite','Red','Green','Blue','Alpha'],'zh-CN':['复合','红','绿','蓝','Alpha']}[locale])[value],category:t.channels,defaultKey:`Primary+Alt+Shift+Digit${value+1}`,enabled:documentEditable&&(documentState.layerEditTarget!=='mask'||value===0||value===4),action:()=>{void selectChannel(value).then(updateDocument).catch(e=>setError(String(e)));}})),
    {id:'color.swap' ,label:t.swapColors,category:t.color,defaultKey:keyFromAccelerator('X'),enabled:documentAvailable&&!placingImage,action:swapColors},
    {id:'app.settings',label:{ja:'環境設定',en:'Preferences','zh-CN':'首选项'}[locale],category:'LumaPaint',defaultKey:keyFromAccelerator('CmdOrCtrl+,'),action:()=>setSettingsOpen(true)},
  ]);

  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => {
      if (placingImage) {
        event.preventDefault();
        if (event.key === 'Enter' || event.key === 'Escape') void finishPlacement(event.key === 'Enter');
        return;
      }
      if (mediaBrowserOpen || toneStudioOpen || newDocumentOpen || importImageOpen || directControlOpen || transformAction) return;

      const target = event.target as HTMLElement;
      if (directControlOpen || target.closest('dialog[open]')) return;
      if (target.closest('input, textarea, select, [contenteditable=true]')) return;
      if (documentAvailable && !settingsOpen && !colorSettingsOpen && !shortcutsOpen) {
        if (!event.metaKey && !event.ctrlKey && !event.altKey) {
          if (!inlineText && documentEditable && documentState.guides.selected.length>0 && ['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) {
            event.preventDefault();const amount=documentState.guides.nudge[event.shiftKey?1:0];
            const delta:[number,number]=event.key==='ArrowLeft'?[-amount,0]:event.key==='ArrowRight'?[amount,0]:event.key==='ArrowUp'?[0,-amount]:[0,amount];
            void editGuides({action:'moveSelected',delta}).then(updateDocument).catch(e=>setError(String(e)));return;
          }
          if ((event.key==='Delete'||event.key==='Backspace')&&target.closest('.layer-panel')) {
            const id=documentState.layerGroups.selected.find(id=>documentState.layerGroups.groups.some(g=>g.id===id));
            if(id){event.preventDefault();void editLayerGroups({action:'delete',id}).then(updateDocument).catch(cause=>setError(String(cause)));return;}
          }
          if (!inlineText && (event.key==='Delete'||event.key==='Backspace')) {event.preventDefault();void edit('deleteSelectedObjects');return;}
          if(event.key==='Escape'){event.preventDefault();void edit('deselect');}
        }
      }
    };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [shortcutsOpen, documentState.layerGroups, locale, documentState.guides.selected, documentState.guides.nudge, documentEditable, updateDocument, placeLinkedImage, mediaBrowserOpen, toneStudioOpen, activeDocumentId, documentAction, edit, file, documentAvailable, settingsOpen, colorSettingsOpen, newDocumentOpen, importImageOpen, directControlOpen, transformAction, placingImage, finishPlacement, swapColors, toolMode, showTextPanel, inlineText, changeGroup]);

  useEffect(() => {
    if (!isTauri()) return;
    const listener = getCurrentWebviewWindow().listen<{ type: string; locale: Locale; theme: Theme }>('modal-change', event => {
      if (event.payload.type === 'preferences') { setLocale(event.payload.locale); setTheme(event.payload.theme); }
      if (event.payload.type === 'createdDocument') changeZoom(0);
    });
    return () => { void listener.then(unlisten => unlisten()); };
  }, [changeZoom]);

  useEffect(() => {
    if (!isTauri()) return;
    const listener = getCurrentWebviewWindow().listen<number>('pdf-import-request', event => setPdfImportToken(event.payload));
    return () => { void listener.then(unlisten => unlisten()); };
  }, []);

  useEffect(()=>{setLayerMaskTarget(null);},[activeDocumentId]);

  return <div className="workspace" data-tool-mode={toolMode} data-panels={panels ? 'open' : 'closed'}>
    <header className="application-bar">
      <AppMenu locale={locale} onSettings={openSettings} onError={setError} />
      <WorkspaceMenu onShortcuts={()=>setShortcutsOpen(true)} onVectorSelection={request=>{void vectorSelectionAction(request).then(updateDocument).catch(e=>setError(String(e)));}} onSavedSelections={setSavedSelectionDialog} onLayerGroupEdit={edit=>{void editLayerGroups(edit).then(updateDocument).catch(cause=>setError(String(cause)));}} onGuides={action=>{void editGuides({action}).then(updateDocument).catch(e=>setError(String(e)));}} outlineDisplay={outlineDisplay} onOutlineDisplay={value => { void outlineView(value).then(setOutlineDisplay).catch(error => setError(String(error))); }} locale={locale} document={documentState} canFile={!fileBusy && !placingImage} hasDocument={documentAvailable} canHistory={documentAvailable && !placingImage && ready && !busy && !fileBusy} canEdit={documentEditable && ready && !busy && !fileBusy}
        zoom={zoom} panels={panels} onPlace={()=>void placeLinkedImage()} onFile={action => void file(action)} onImportImage={() => setImportImageOpen(true)} onImportSvg={() => void importSvg()} onEdit={action => void edit(action)} onZoom={changeZoom}
        onTransform={setTransformAction}
        onWritingMode={mode => { void setTextWritingMode(mode).then(updateDocument).catch(cause => setError(String(cause))); }}
        onOutlineText={() => { void outlineText().then(updateDocument).catch(cause => setError(String(cause))); }}
        onArrange={action => { void arrangeSelectedVectors(action).then(updateDocument).catch(cause => setError(String(cause))); }}
        onCompound={release => { void compoundPath(release).then(updateDocument).catch(cause => setError(String(cause))); }}
        onClipping={action => { void clippingPath(action).then(snapshot => { updateDocument(snapshot); if (action === 'edit') setTool('vectorDirectSelect'); }).catch(cause => setError(String(cause))); }}
        onGroup={action => void changeGroup(action)}
        onPathEdit={action => void editPath(action)}
        onNewWindow={() => { void newEditorWindow().catch(cause => setError(String(cause))); }} onNew={() => void documentAction('new')} onCloseDocument={() => activeDocumentId !== null && void documentAction('close', activeDocumentId)}
        onColorMode={mode => void setColorMode(mode)}
        onBitDepth={depth => void setBitDepth(depth)}
        onColorSettings={openColorSettings}
        onPanels={() => setPanels(value => !value)} onReset={() => { setPanels(window.innerWidth > 720); changeZoom(0); }} onError={setError} />
      <div className="workspace-preferences">
        <button className="icon-button media-browser-launcher" title={mediaBrowserTitle[locale]} aria-label={mediaBrowserTitle[locale]} aria-pressed={mediaBrowserOpen} aria-controls="media-browser-overlay" onClick={()=>{if(inlineText)endText(true);setToneStudioOpen(false);setMediaBrowserOpen(value=>!value);}}><Icon name="image"/></button>
        <button className="icon-button tone-studio-launcher" title={toneStudioLabels[locale].title} aria-label={toneStudioLabels[locale].title} aria-pressed={toneStudioOpen} aria-controls="tone-studio-overlay" onClick={()=>{if(inlineText)endText(true);setMediaBrowserOpen(false);setToneStudioLoaded(true);setToneStudioOpen(value=>!value);}}><Icon name="palette"/></button>
        <button className="icon-button" title={t.panels} aria-label={t.panels} aria-pressed={panels} onClick={() => setPanels(value => !value)}><Icon name="panels" /></button>
      </div>
    </header>
    <div className="options-bar" inert={toneStudioOpen || mediaBrowserOpen} aria-label={cropTool ? t.crop : gradientTool ? gradientLabel : zoomTool ? common[zoomTool] : t[toolState.tools[toolMode]]}>
      <span className="current-tool"><Icon name={canvasTool} /><span><small className="current-mode">{t[modeLabels[toolMode]]}</small>{cropTool ? t.crop : gradientTool ? gradientLabel : zoomTool ? common[zoomTool] : canvasTool === 'vectorDirectSelect' ? t.vectorDirectSelect : t[toolState.tools[toolMode]]}</span></span>
      {isRetouch(canvasTool)?<><label>{t.size}<SizeInput label={t.size} resolution={horizontalResolution} value={brush.size} onChange={size=>setBrush(previous=>({...previous,size}))}/></label><label>{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness=>setBrush(previous=>({...previous,hardness}))}/></label><RetouchControls key={canvasTool} locale={locale} tool={canvasTool} onError={setError}/></>:isPathSelection(canvasTool)?<SelectionPathControls key={canvasTool} locale={locale} tool={canvasTool} onError={setError}/>:canvasTool==='paintBucket'?<><PaintBucketControls key={`${activeDocumentId}:${documentState.pages.pages[documentState.pages.active]?.id}`} locale={locale} layers={documentState.layers} onError={setError}/><label className="color-control">{t.foreground}<ColorPickerPopover locale={locale} color={brush.color} label={t.foreground} noColor={!!brush.noColor} onNone={()=>setBrush(previous=>({...previous,noColor:true}))} onChange={changeForeground}/></label></>:canvasTool==='cloneStamp'? <><label>{t.size}<SizeInput label={t.size} resolution={horizontalResolution} value={brush.size} onChange={size=>setBrush(previous=>({...previous,size}))}/></label><label>{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness=>setBrush(previous=>({...previous,hardness}))}/></label><CloneStampControls locale={locale} onError={setError}/></> : cropTool ? <><span className="selection-hint">{t.cropHint}</span><button disabled={!documentEditable || !ready} onClick={()=>void invoke('crop_action',{confirm:true}).catch(cause=>setError(String(cause)))}>{t.cropApply}</button><button onClick={()=>void invoke('crop_action',{confirm:false}).catch(cause=>setError(String(cause)))}>{t.cropCancel}</button></> : documentState.guides.selected.length > 0 ? <GuideOptions key={documentState.guides.selected.join('|')+measurementUnit+documentState.guides.origin.join(',')} document={documentState} locale={locale} enabled={ready && !busy && documentEditable} onUpdate={updateDocument} onError={setError}/> : documentState.selectedVectorObjects.length > 0 && !inlineText && !zoomTool && canvasTool !== 'vectorDirectSelect' ? <SelectionOptions document={documentState} locale={locale} enabled={ready && !busy && documentEditable}
        onAppearance={async (opacity, blendMode) => { updateDocument(await setVectorAppearance([...documentState.selectedVectorObjects], opacity, blendMode)); }}
        onPaint={async (target,color) => { updateDocument(await setVectorPaint([...documentState.selectedVectorObjects],target,color)); }}
        onWidth={async width => { updateDocument(await setVectorStrokeWidth(width,brush.color)); }}
        onTransform={async (action,values) => { updateDocument(await transformObjects(action,values)); }}
        onCombine={async operation => { updateDocument(await combineSelectedVectors(operation)); }} onTransformMenu={setTransformAction} onError={setError} /> : zoomTool ? <span className="selection-hint">{zoomTool === 'hand' ? t.handHint : t.zoomClickHint}</span> : canvasTool.startsWith('vector') ? <><span className="selection-hint">{canvasTool === 'vectorSelect' && documentState.selectedVectorObjects.length > 0 ? `${documentState.selectedVectorObjects.length} ${t.vectorSelected}` : canvasTool === 'vectorRotate' ? t.rotateHint : canvasTool === 'vectorScale' ? t.scaleHint : canvasTool === 'vectorDirectSelect' ? t.directHint : canvasTool.startsWith('vectorAnchor') ? t.anchorHint : canvasTool === 'vectorPen' ? t.penHint : toolMode === 'layout' ? t.layoutHint : t.vectorHint}</span>{canvasTool === 'vectorDirectSelect' && <button disabled={!documentEditable || busy} onClick={()=>setDirectControlOpen(true)}>{directControlLabels[locale].title}</button>}{canvasTool === 'vectorSelect' && <select className="path-operations" aria-label={t.pathOperations} title={t.pathOperations} value="" disabled={!ready || busy || (documentState.selectedVectorObjects.length < 2 || documentState.selectedVectorObjects.length > 64)} onChange={event => { const operation = event.target.value as PathOperation; event.currentTarget.value = ''; void combineVectors(operation); }}><option value="">{t.pathOperations}</option><option value="union">{t.pathUnion}</option><option value="difference">{t.pathDifference}</option><option value="intersection">{t.pathIntersection}</option><option value="xor">{t.pathXor}</option></select>}</> : (canvasTool === 'brush' || canvasTool === 'eraser') ? <>
      <label className="size-control">{t.size}<SizeInput resolution={horizontalResolution} label={t.size} value={brush.size} onChange={size => setBrush(previous => ({ ...previous, size }))} /></label>
      <label className="hardness-control">{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => setBrush(previous => ({ ...previous, hardness }))} /></label>
      <BrushSettings compact brush={brush} locale={locale} onChange={setBrush} eraser={canvasTool==='eraser'} alphaLocked={documentState.layers.find(layer=>layer.id===documentState.layerId)?.alphaLocked}/>
      <label className="color-control"><span>{t.foreground}</span><ColorPickerPopover locale={locale} color={vectorColors?.fill ?? brush.color} label={t.foreground} noColor={vectorColors ? vectorColors.fillStatus === 'none' : foregroundNone} onNone={changeForegroundNone} onChange={changePaintForeground} /></label>
      </> : <span className="selection-hint">{canvasTool.startsWith('imageFrame')?t.frameHint:canvasTool==='gradient'?t.gradientHint:(canvasTool === 'text' || canvasTool === 'textVertical') ? textPanelMessages[locale].hint : (canvasTool === 'textFrame' || canvasTool === 'textFrameVertical') ? t.textFrameHint : t.selectionHint}</span>}
      {toolMode === 'animation' && <span className="selection-hint animation-hint">{t.animationHint}</span>}
      {documentState.selection && <button className="selection-clear" disabled={!ready || busy} onClick={() => void edit('deselect')}>{t.deselect}</button>}
      <p className="session-note" role="status">{fileBusy ? t.fileBusy : !documentAvailable ? t.noDocument : !documentEditable ? t.tiledReadOnly : documentState.dirty ? t.sessionOnly : documentState.fileName ? t.saved : t.empty}</p>
    </div>
    <main className="editor-layout" inert={toneStudioOpen || mediaBrowserOpen}>
      {shortcutsOpen && <NativeModal kind="shortcuts" locale={locale} theme={theme} onClose={()=>setShortcutsOpen(false)} onError={setError}><ShortcutDialog locale={locale} onClose={()=>setShortcutsOpen(false)}/></NativeModal>}
      {layerMaskTarget && layerMaskTarget.documentId===activeDocumentId && <NativeModal kind="layerMask" action={layerMaskTarget.id} locale={locale} theme={theme} onClose={()=>setLayerMaskTarget(null)} onError={setError}><LayerMaskDialog locale={locale} document={documentState} targetId={layerMaskTarget.id} onClose={()=>setLayerMaskTarget(null)} onUpdate={updateDocument}/></NativeModal>}
      {savedSelectionDialog&&<NativeModal kind="vectorSelections" action={savedSelectionDialog} locale={locale} theme={theme} onClose={()=>setSavedSelectionDialog(null)} onError={setError}><SavedSelectionsDialog locale={locale} document={documentState} mode={savedSelectionDialog} onClose={()=>setSavedSelectionDialog(null)} onUpdate={updateDocument}/></NativeModal>}
      {toolSettings && <NativeModal kind="toolSettings" action={toolSettings} locale={locale} theme={theme} onClose={()=>setToolSettings(null)} onError={setError}><ToolSettingsDialog tool={toolSettings} locale={locale} document={documentState} brush={brush} onBrush={setBrush} onZoom={changeZoom} onUpdate={updateDocument} onClose={()=>setToolSettings(null)}/></NativeModal>}
      {directControlOpen && <NativeModal kind="directControls" locale={locale} theme={theme} onClose={()=>setDirectControlOpen(false)} onError={setError}><DirectControlDialog locale={locale} doc={documentState} onApply={updateDocument} onClose={()=>setDirectControlOpen(false)} /></NativeModal>}
      {transformAction && <NativeModal kind="transform" locale={locale} theme={theme} onClose={()=>setTransformAction(null)} onError={setError} action={transformAction}><TransformDialog resolution={horizontalResolution} key={transformAction} action={transformAction} locale={locale} onClose={()=>setTransformAction(null)} onApply={async values=>{updateDocument(await transformObjects(transformAction,values));}} /></NativeModal>}
      <nav className="tool-rail" aria-label={t.tools} title={toolSettingsLabels[locale].hint}>
        <ToolModeSwitch mode={toolMode} locale={locale} onChange={setToolMode} />
        <span className="tool-mode-divider" aria-hidden="true" />
        {/* Shared tools always occupy the left column; mode-only tools the right. */}
        <div className="tool-columns">
        <div className="common-tools">
        <button className={`tool-button${cropTool?' selected':''}`} aria-label={t.crop} title={`${t.crop} (C)`} aria-pressed={cropTool} disabled={!documentEditable || !ready} onClick={()=>{setTool('crop');if(cropTool)openToolSettings('crop');}} onDoubleClick={()=>openToolSettings('crop')}><Icon name="crop"/></button>
        <IconToolMenu key="vector-selection" label={t.vectorSelect} selected={canvasTool === 'vectorSelect' || canvasTool === 'vectorDirectSelect' ? canvasTool : lastVectorSelectTool} active={canvasTool === 'vectorSelect' || canvasTool === 'vectorDirectSelect'} enabled={documentEditable} choices={[{ id: 'vectorSelect', label: t.vectorSelect, icon: 'vectorSelect', shortcut: 'V' }, { id: 'vectorDirectSelect', label: t.vectorDirectSelect, icon: 'vectorDirectSelect', shortcut: 'A' }]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings} />
        <SelectionToolMenu locale={locale} selected={selectionTool ?? lastSelectionTool} active={selectionTool !== null} enabled={documentEditable} onSelect={setTool} onError={setError} onSettings={openToolSettings} />
        <IconToolMenu key="vector-transform" label={t.transformTools} selected={canvasTool === 'vectorScale' || canvasTool === 'vectorRotate' ? canvasTool : lastTransformTool} active={canvasTool === 'vectorScale' || canvasTool === 'vectorRotate'} enabled={documentEditable} choices={[{ id: 'vectorScale', label: t.vectorScale, icon: 'vectorScale' }, { id: 'vectorRotate', label: t.vectorRotate, icon: 'vectorRotate' }, ...(['move','reflect','shear','individual','reset'] as const).map((action,i)=>({id:action,label:transformLabels[locale][action],icon:(['transformMove','transformReflect','transformShear','transformEach','transformReset'] as const)[i]}))]} onSelect={tool => { if(tool==='vectorScale'||tool==='vectorRotate') setTool(tool); else setTransformAction(tool as TransformAction); }} onError={setError} onSettings={openToolSettings} />
        <button className={`tool-button${gradientTool?' selected':''}`} aria-label={gradientLabel} title={`${gradientLabel} (G)`} aria-pressed={gradientTool} disabled={!documentEditable} onClick={()=>{setTool('gradient');showGradientPanel();}} onDoubleClick={showGradientPanel}><Icon name="gradient"/></button>
        <button className={`tool-button${sampling ? ' selected' : ''}`} aria-label={t.eyedropper} title={`${t.eyedropper} (I)`} aria-pressed={sampling} disabled={!documentAvailable || !ready} onClick={()=>{setTool("eyedropper");if(sampling)openToolSettings("eyedropper");}} onDoubleClick={()=>openToolSettings("eyedropper")}><Icon name="eyedropper" /></button>
        <ZoomToolMenu locale={locale} selected={zoomTool ?? lastZoomTool} active={zoomTool !== null} enabled={documentAvailable && ready} onSelect={setTool} onError={setError} onSettings={openToolSettings} />
        </div>
        <div className="mode-tools">
        {modeTools[toolMode].map(item => (isRetouch(item)&&item!=='blur')?null:item==='blur'?<IconToolMenu key="retouch-tools" label={t.blur} selected={isRetouch(canvasTool)?canvasTool:'blur'} active={isRetouch(canvasTool)} enabled={documentEditable} choices={retouchTools.map(tool=>({id:tool,label:t[tool],icon:tool,shortcut:'U'})) as [IconToolChoice,...IconToolChoice[]]} onSelect={tool=>setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings}/>:(isPathSelection(item)&&item!=='lasso')?null:item==='lasso'?<IconToolMenu key="lasso-tools" label={t.lasso} selected={isPathSelection(canvasTool)?canvasTool:'lasso'} active={isPathSelection(canvasTool)} enabled={documentEditable} choices={pathSelectionTools.map(tool=>({id:tool,label:t[tool],icon:tool,shortcut:'L'})) as [IconToolChoice,...IconToolChoice[]]} onSelect={tool=>setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings}/>:item === 'imageFrameEllipse' || item === 'vectorEllipse' || item === 'textVertical' || item === 'textFrame' || item === 'textFrameVertical' || (isPenTool(item) && item !== 'vectorPen') ? null : (item === 'brush' || item === 'eraser') && toolMode === 'paint' ?
          <IconToolMenu key={item} label={item === 'brush' ? t.drawTools : t.eraseTools} selected={item} active={canvasTool === item} enabled={documentEditable} choices={[{ id: item, label: t[item], icon: item, shortcut: item === 'brush' ? 'B' : 'E' }]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings} /> :
          item === 'vectorPen' ? <IconToolMenu key="pen-tools" label={t.penTools} selected={isPenTool(canvasTool) ? canvasTool : lastPenTool} active={isPenTool(canvasTool)} enabled={documentEditable} choices={penTools.map(tool => ({ id: tool, label: t[tool], icon: tool, shortcut: tool === 'vectorPen' ? 'P' : tool === 'vectorPencil' ? 'N' : undefined })) as [IconToolChoice, ...IconToolChoice[]]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings} /> :
          item === 'vectorRectangle' ?
          <VectorShapeToolMenu key="vector-shapes" locale={locale} selected={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse' ? canvasTool : lastVectorShapeTool} active={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse'} enabled={documentEditable} onSelect={setTool} onError={setError} onSettings={openToolSettings} /> :
          item === 'imageFrameRectangle' ? <IconToolMenu key="image-frames" label={t.imageFrames} selected={canvasTool==='imageFrameRectangle'||canvasTool==='imageFrameEllipse'?canvasTool:lastFrameTool} active={canvasTool==='imageFrameRectangle'||canvasTool==='imageFrameEllipse'} enabled={documentEditable} choices={[{id:'imageFrameRectangle',label:t.imageFrameRectangle,icon:'imageFrameRectangle',shortcut:'F'},{id:'imageFrameEllipse',label:t.imageFrameEllipse,icon:'imageFrameEllipse',shortcut:'Shift+F'}]} onSelect={tool=>setTool(tool as CanvasTool)} onError={setError} onSettings={openToolSettings}/> :
          item === 'text' ? <IconToolMenu key="text-tools" label={t.textTool} selected={canvasTool === 'text' || canvasTool === 'textVertical' || (canvasTool === 'textFrame' || canvasTool === 'textFrameVertical') ? canvasTool : lastTextTool} active={canvasTool === 'text' || canvasTool === 'textVertical' || (canvasTool === 'textFrame' || canvasTool === 'textFrameVertical')} enabled={documentEditable} choices={[{ id: 'text', label: t.text, icon: 'text', shortcut: 'T' }, { id: 'textVertical', label: t.textVertical, icon: 'textVertical' }, { id: 'textFrame', label: t.textFrame, icon: 'textFrame' }, { id: 'textFrameVertical', label: t.textFrameVertical, icon: 'textFrameVertical' }]} onSelect={tool => { setTool(tool as CanvasTool); showTextPanel(); }} onError={setError} onSettings={openToolSettings} /> :
          <button key={item} className={`tool-button${canvasTool === item ? ' selected' : ''}`} aria-label={t[item]} title={`${t[item]}${commandShortcut(`tool.${item}`)?` (${commandShortcut(`tool.${item}`)})`:''}`} aria-pressed={canvasTool === item} disabled={!documentEditable} onClick={()=>{setTool(item);if(canvasTool===item)openToolSettings(item);}} onDoubleClick={()=>openToolSettings(item)}><Icon name={item} /></button>)}
        {(toolMode === 'vector' || toolMode === 'layout') && <button className="tool-button" aria-label={t.importVector} title={t.importVector} disabled={!documentEditable || fileBusy} onClick={() => void importSvg()}><Icon name="importVector" /></button>}
        {toolMode === 'animation' && <button className="tool-button" disabled aria-label={`${t.timelineTool} · ${t.toolPlanned}`} title={t.animationHint}><Icon name="timeline" /></button>}
        </div>
        <div className="tool-color-pickers" role="group" aria-label={`${t.foreground} · ${t.background}`}>
          <ColorPickerPopover locale={locale} color={vectorColors?.stroke ?? backgroundColor} label={t.background} noColor={vectorColors ? vectorColors.strokeStatus === 'none' : backgroundNone} onNone={changeBackgroundNone} target="background" onOpen={() => setActiveColor('background')} onChange={changePaintBackground} />
          <ColorPickerPopover locale={locale} color={vectorColors?.fill ?? brush.color} label={t.foreground} noColor={vectorColors ? vectorColors.fillStatus === 'none' : foregroundNone} onNone={changeForegroundNone} onOpen={() => setActiveColor('foreground')} onChange={changePaintForeground} />
          <button type="button" className="tool-color-swap" onClick={swapColors} title={`${t.swapColors}${commandShortcut('color.swap')?` (${commandShortcut('color.swap')})`:''}`} aria-label={t.swapColors}>↔</button>
        </div>
        </div>
      </nav>
      <div className="document-area">
        <div className="document-tabs">
          <div className="document-tab-list" role="tablist" aria-label={t.openDocuments}>
            {documents.map((document, index) => <div key={document.id} className="document-tab-shell" aria-current={document.id === activeDocumentId}>
              <button type="button" role="tab" aria-selected={document.id === activeDocumentId} className="document-tab" onContextMenu={event=>{if(isTauri()&&!fileBusy){event.preventDefault();setTabMenu({id:document.id,x:event.clientX,y:event.clientY});}}} title={document.format === 'tiled' ? t.recoveryTiled : t.recoveryLegacy} onClick={() => void documentAction('switch', document.id)}>
                <span>{document.fileName ?? `${t.untitledBase}-${index + 1}`}</span>{document.format === 'tiled' && <span className="document-format" aria-label={t.recoveryTiled}>T</span>}{document.dirty && <span className="unsaved-dot" aria-label={t.sessionOnly} />}
              </button>
              <button type="button" className="document-close" aria-label={`${document.fileName ?? `${t.untitledBase}-${index + 1}`} · ${t.closeDocument}`} onClick={() => void documentAction('close', document.id)}>×</button>
            </div>)}
            {!documentAvailable && <span className="no-document">{t.noDocument}</span>}
          </div>
          {documentAvailable && <span className="document-dimensions">{Number((documentState.width/pixelsPerMeasurement(measurementUnit,horizontalResolution)).toFixed(3))} × {Number((documentState.height/pixelsPerMeasurement(measurementUnit,verticalResolution)).toFixed(3))} {unitSymbols[measurementUnit]} · {documentState.colorMode.toUpperCase()} · {documentState.bitDepth} bits</span>}
        </div>
        {tabMenu&&<DocumentTabMenu key={tabMenu.id} locale={locale} x={tabMenu.x} y={tabMenu.y} onClose={()=>setTabMenu(null)} onMove={async target=>{
          if(filePending.current)throw new Error({ja:'他のファイル操作が進行中です',en:'Another file operation is in progress','zh-CN':'正在进行其他文件操作'}[locale]);
          filePending.current=true;setFileBusy(true);
          try{updateWorkspace(await moveDocumentToWindow(tabMenu.id,target));}finally{filePending.current=false;setFileBusy(false);}
        }} onView={async target=>{
          if(filePending.current)throw new Error({ja:'他のファイル操作が進行中です',en:'Another file operation is in progress','zh-CN':'正在进行其他文件操作'}[locale]);
          filePending.current=true;setFileBusy(true);
          try{updateWorkspace(await openDocumentView(tabMenu.id,target));}finally{filePending.current=false;setFileBusy(false);}
        }}/>}
        <DocumentDock locale={locale}><CanvasPreview resolution={horizontalResolution} verticalResolution={verticalResolution} channel={channel} locale={locale} theme={theme} brush={brush} tool={canvasTool} zoom={zoom} onDisplayZoom={setZoom} hasDocument={documentAvailable} visible={documentAvailable && !toneStudioOpen && !mediaBrowserOpen && (isTauri() || (!settingsOpen && !colorSettingsOpen && !newDocumentOpen && !importImageOpen && !transformAction && !directControlOpen))} occlusion={colorPickerOcclusion}
          footerAccessory={placingImage ? <div className="image-placement-controls">
            <span>{ {ja:'画像を配置：辺・角で拡大縮小、角の外側で回転', en:'Place image: resize with handles, rotate outside corners', 'zh-CN':'放置图片：拖动控制点缩放，在角外旋转'}[locale] }</span>
            <button disabled={placementBusy} onClick={() => void finishPlacement(false)}>{ {ja:'キャンセル',en:'Cancel','zh-CN':'取消'}[locale] }</button>
            <button disabled={placementBusy} onClick={() => void finishPlacement(true)}>{ {ja:'確定',en:'Confirm','zh-CN':'确认'}[locale] }</button>
          </div> : <RecoveryControls locale={locale} document={documentState} onDocument={updateDocument} />}
          zoomCommand={zoomCommand} onZoom={changeZoom} onDocument={updateDocument} onReady={setReady} /></DocumentDock>
      </div>
      {panels && <Inspector documentAvailable={documentAvailable} onOpenLayerMask={id=>setLayerMaskTarget(current=>current??{id,documentId:activeDocumentId})} onLayerEffects={(id, effects) => { if (!ready || busy || fileBusy) return; setBusy(true); setError(''); void setLayerEffects(id, effects).then(updateDocument).catch(cause => setError(String(cause))).finally(() => setBusy(false)); }} rasterEnabled={!placingImage && documentAvailable && !documentEditable && ready && !busy && !fileBusy} onRasterBlendMode={(id, mode) => void changeRasterBlendMode(id, mode)} linksPanelRequest={linksPanelRequest} gradientTool={gradientTool} gradientPanelRequest={gradientPanelRequest} onTransformUpdate={updateDocument} thumbnailDocumentKey={activeDocumentId === null ? '' : String(activeDocumentId)} onSavedPathAction={async (action, id, name) => { updateDocument(await savedPathAction(action, id, name)); }} vectorColors={vectorColors} channel={channel} onChannel={value => {void selectChannel(value).then(updateDocument).catch(cause=>setError(String(cause)));}} onStrokeStyle={async patch => { updateDocument(await setVectorStrokeStyle(patch)); }} onStrokeWidth={async width => { updateDocument(await setVectorStrokeWidth(width, brush.color)); }} textPanelRequest={textPanelRequest} textSettings={activeText} textEditing={inlineText !== null}
        textEnabled={documentEditable && ready && !busy && !fileBusy && (inlineText !== null || !selectedText || selectedText.editable)} onTextChange={changeText} onTextBegin={beginText} onTextFinish={endText} locale={locale} brush={brush} backgroundColor={backgroundColor} activeColor={activeColor} onSelectColor={setActiveColor} colorPanelRequest={colorPanelRequest} onBrush={setBrush} onForegroundChange={changePaintForeground} onBackgroundChange={changePaintBackground} foregroundNone={foregroundNone} backgroundNone={backgroundNone} onForegroundNone={changeForegroundNone} onBackgroundNone={changeBackgroundNone} onSwapColors={swapColors} document={documentState} enabled={documentEditable && ready && !busy}
        onLayerGroupEdit={edit=>{void editLayerGroups(edit).then(updateDocument).catch(cause=>setError(String(cause)));}} onDocumentSettings={settings => void setDocumentSettings(settings)} onColorMode={mode => void setColorMode(mode)} onBitDepth={depth => void setBitDepth(depth)} onColorProfile={profile => void setColorProfile(profile)} onToggleLayer={id => void setLayerVisibility(id)} onLayerSettings={settings => void setLayerSettings(settings)} onDeleteLayer={id => void removeLayer(id)} onSelectLayerTarget={(id,target) => {void selectLayerTarget(id,target).then(updateDocument).catch(cause=>setError(String(cause)));}} onSelectLayer={id => { void selectLayer(id, true).then(updateDocument).catch(cause => setError(String(cause))); }} onSelectObject={(layerId, objectId) => { void selectLayer(layerId).then(() => selectVectorObjects([objectId])).then(updateDocument).catch(cause => setError(String(cause))); }} onToggleObject={(layerId, objectId, visible) => { void setVectorObjectVisibility(layerId, objectId, visible).then(updateDocument).catch(cause => setError(String(cause))); }} onReorderObjects={(layerId, ids) => { void reorderVectorObjects(layerId, ids).then(updateDocument).catch(cause => setError(String(cause))); }} onAddLayer={() => void createLayer('paint')} onAddVectorLayer={() => void createLayer('vector')} onReorderLayer={ids => void moveLayer(ids)} />}
    </main>
    {error && <div className="workspace-error" role="alert">{error}<button aria-label={common.dismiss} onClick={() => setError('')}>×</button></div>}
    {mediaBrowserOpen&&<Suspense fallback={null}><MediaBrowser locale={locale} onClose={closeMediaBrowser}/></Suspense>}
    {toneStudioLoaded&&<ToneStudio locale={locale} open={toneStudioOpen} onClose={()=>setToneStudioOpen(false)}/>}
    {settingsOpen && <NativeModal kind="settings" locale={locale} theme={theme} onClose={closeSettings} onError={setError}><SettingsDialog locale={locale} theme={theme} onLocale={setLocale} onTheme={setTheme} onClose={closeSettings} /></NativeModal>}
    {importImageOpen && <NativeModal kind="importImage" locale={locale} theme={theme} onClose={()=>setImportImageOpen(false)} onError={setError}><ImportImageDialog locale={locale} onClose={() => setImportImageOpen(false)} onImport={async format => {
      if (filePending.current) throw new Error({ja:'他のファイル操作が進行中です',en:'Another file operation is in progress','zh-CN':'正在进行其他文件操作'}[locale]);
      filePending.current = true; setFileBusy(true);
      try { updateDocument(await importRasterLayer(format)); }
      finally { filePending.current = false; setFileBusy(false); }
    }} /></NativeModal>}
    {pdfImportToken !== null && <NativeModal kind="pdfImport" locale={locale} theme={theme} onClose={() => { const token = pdfImportToken; setPdfImportToken(null); void invoke('pdf_import_cancel', { token }).catch(cause => setError(String(cause))); }} onError={setError}>{null}</NativeModal>}
    {newDocumentOpen && <NativeModal kind="newDocument" locale={locale} theme={theme} onClose={()=>setNewDocumentOpen(false)} onError={setError}><NewDocumentDialog locale={locale} onCreate={createFromPreset} onClose={() => setNewDocumentOpen(false)} /></NativeModal>}
    {colorSettingsOpen && <NativeModal kind="colorSettings" locale={locale} theme={theme} onClose={closeColorSettings} onError={setError}><ColorSettingsDialog locale={locale} document={documentState} enabled={ready && !busy} onProfile={profile => void setColorProfile(profile)} onClose={closeColorSettings} /></NativeModal>}
  </div>;
}
