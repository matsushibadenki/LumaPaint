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
import { arrangeSelectedVectors, setVectorStrokeStyle, setVectorStrokeWidth, reorderVectorObjects, selectVectorObjects, setVectorObjectVisibility, selectLayer, addVectorLayer } from './bridge';
import { useCallback, useEffect, useRef, useState, type SetStateAction } from 'react';
import { CanvasPreview } from './CanvasPreview';
import { subscribeCanvasZoom } from './bridge';
import { beginTextEdit, updateTextEdit, setTextEditColor, finishTextEdit, subscribeTextSession, defaultVectorText, type TextSettings, subscribeCanvasText, subscribeCanvasColorSwap, subscribeCanvasTool, type CanvasTool, type DocumentEditAction, addPaintLayer, changeBitDepth, changeColorMode, changeColorProfile, changeDocumentSettings, closeDocument, combineSelectedVectors, groupSelectedVectors, ungroupSelectedVectors, editSelectedPaths, createDocument, deleteLayer, editDocument, getDocumentWorkspace, importSvgLayer, projectAction, reorderLayers, switchDocument, toggleLayer, updateLayer, emptyDocument, subscribeDocument, subscribeDocuments, type BitDepth, type Brush, type ColorMode, type ColorProfile, type DocumentSettings, type DocumentSnapshot, type DocumentTabSnapshot, type DocumentWorkspaceSnapshot, type LayerSettings, type PathEditAction, type PathOperation } from './bridge';
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

export function Workspace() {
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
  const [zoomTool, setZoomTool] = useState<ZoomTool | null>(null);
  const [lastZoomTool, setLastZoomTool] = useState<ZoomTool>('zoomIn');
  const [transformTool, setTransformTool] = useState<'vectorScale' | 'vectorRotate' | 'vectorSelect' | 'vectorDirectSelect' | null>(null);
  const [selectionTool, setSelectionTool] = useState<SelectionTool | null>(null);
  const canvasTool = zoomTool ?? transformTool ?? selectionTool ?? toolState.tools[toolMode];
  const [lastSelectionTool, setLastSelectionTool] = useState<SelectionTool>('rectangle');
  const [directControlOpen,setDirectControlOpen] = useState(false);
  const [transformAction, setTransformAction] = useState<TransformAction | null>(null);
  const [lastTransformTool, setLastTransformTool] = useState<'vectorScale' | 'vectorRotate'>('vectorScale');
  const [lastVectorSelectTool, setLastVectorSelectTool] = useState<'vectorSelect' | 'vectorDirectSelect'>('vectorSelect');
  const [lastPenTool, setLastPenTool] = useState<PenTool>('vectorPen');
  const [lastVectorShapeTool, setLastVectorShapeTool] = useState<VectorShapeTool>('vectorRectangle');
  const [lastTextTool, setLastTextTool] = useState<'text' | 'textFrame'>('text');
  const setToolMode = useCallback((mode: ToolMode) => {
    setZoomTool(null);
    setToolState(current => ({ ...current, mode }));
  }, []);
  const setTool = useCallback((next: CanvasTool) => {
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
    if (isPenTool(next)) setLastPenTool(next);
    if (next === 'vectorRectangle' || next === 'vectorEllipse') setLastVectorShapeTool(next);
    if (next === 'text' || next === 'textFrame') setLastTextTool(next);
  }, []);
  const [paintState, setPaintState] = useState<{ brush: Brush; backgroundColor: Brush['color'] }>({
    brush: { size: 16, hardness: 1, color: [32, 32, 32] }, backgroundColor: [255, 255, 255],
  });
  const [activeColor, setActiveColor] = useState<ColorTarget>('foreground');
  const colorPanelRequest = 0;
  const { brush, backgroundColor } = paintState;
  const setBrush = useCallback((next: SetStateAction<Brush>) => {
    setPaintState(current => ({ ...current, brush: typeof next === 'function' ? next(current.brush) : next }));
  }, []);
  const setBackgroundColor = useCallback((color: Brush['color']) => {
    setPaintState(current => ({ ...current, backgroundColor: color }));
  }, []);
  const [documentState, setDocumentState] = useState(emptyDocument);
  const [documents, setDocuments] = useState<DocumentTabSnapshot[]>([]);
  const [activeDocumentId, setActiveDocumentId] = useState<number | null>(null);
  const [ready, setReady] = useState(false);
  const [documentAvailable, setDocumentAvailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fileBusy, setFileBusy] = useState(false);
  const filePending = useRef(false);
  const [error, setError] = useState('');
  const [zoom, setZoom] = useState(1);
  const [channel, setChannel] = useState<DisplayChannel>(0);
  useEffect(() => setChannel(0), [activeDocumentId, documentState.colorMode]);
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
  const updateWorkspace = useCallback((next: DocumentWorkspaceSnapshot) => {
    setDocuments(next.documents);
    setActiveDocumentId(next.activeId);
    setDocumentAvailable(next.active !== null);
    setDocumentState(next.active ?? emptyDocument);
  }, []);

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
    setBrush(previous => ({ ...previous, color }));
    applyTextColor(color);
  }, [setBrush, applyTextColor]);
  const swapColors = useCallback(() => {
    setPaintState(current => ({
      brush: { ...current.brush, color: current.backgroundColor }, backgroundColor: current.brush.color,
    }));
    applyTextColor(backgroundColor);
  }, [applyTextColor, backgroundColor]);
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
  const endText = useCallback((commit: boolean) => {
    void finishTextEdit(commit).then(snapshot => {
      textSessionActive.current = false;
      updateDocument(snapshot);
    }).catch(cause => setError(String(cause)));
  }, [updateDocument]);
  const beginText = () => {
    const settings: TextSettings = activeText ?? { id: null, text: { ...defaultVectorText, content: textMessages[locale].defaultText }, position: [48, 48], color: brush.color };
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
    if (!ready || busy || documentState.selectedVectorObjects.length !== 2) return;
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

  const file = useCallback(async (action: 'open' | 'save' | 'saveAs') => {
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
    if (filePending.current) throw new Error('Another file operation is in progress');
    filePending.current = true; setFileBusy(true);
    try { updateWorkspace(await createDocument(settings)); setZoom(1); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [updateWorkspace]);

  const importSvg = useCallback(async () => {
    if (!documentEditable || filePending.current) return;
    filePending.current = true; setFileBusy(true); setError('');
    try { updateDocument(await importSvgLayer()); }
    catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [documentEditable, updateDocument]);

  const setLayerVisibility = useCallback(async (id: string) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await toggleLayer(id)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);
  const setLayerSettings = useCallback(async (settings: LayerSettings) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await updateLayer(settings)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);
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
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await reorderLayers(ids)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);

  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => {
      if (placingImage) {
        event.preventDefault();
        if (event.key === 'Enter' || event.key === 'Escape') void finishPlacement(event.key === 'Enter');
        return;
      }
      if (newDocumentOpen || importImageOpen || directControlOpen) return;

      if ((event.metaKey || event.ctrlKey) && ['s', 'o', 'w', 'n'].includes(event.key.toLowerCase())) {
        event.preventDefault();
        const key = event.key.toLowerCase();
        if ((event.metaKey || event.ctrlKey) && ['c', 'x', 'v'].includes(key)) { event.preventDefault(); void edit(key === 'c' ? 'copy' : key === 'x' ? 'cut' : 'paste'); return; }
        if (key === 'n') void documentAction('new');
        else if (key === 'w' && activeDocumentId !== null) void documentAction('close', activeDocumentId);
        else void file(key === 'o' ? 'open' : event.shiftKey ? 'saveAs' : 'save');
        return;
      }
      const target = event.target as HTMLElement;
      if (directControlOpen || target.closest('dialog[open]')) return;
      if (target.closest('input, textarea, select, [contenteditable=true]')) return;
      if (documentAvailable && !settingsOpen && !colorSettingsOpen) {
        const key = event.key.toLowerCase();
        if ((event.metaKey || event.ctrlKey) && key === 'g') {
          event.preventDefault(); void changeGroup(event.shiftKey ? 'ungroup' : 'group');
          return;
        }
        if ((event.metaKey || event.ctrlKey) && (key === 'a' || key === 'd' || (key === 'i' && event.shiftKey))) {
          event.preventDefault(); void edit(key === 'a' ? 'selectAll' : key === 'd' ? 'deselect' : 'invertSelection');
          return;
        }
        if (!event.metaKey && !event.ctrlKey && !event.altKey) {
          if (!inlineText && (event.key === 'Delete' || event.key === 'Backspace')) {
            event.preventDefault(); void edit('deleteSelectedObjects'); return;
          }
          if (key === 'e') { event.preventDefault(); setTool('eraser'); }
          if (key === 'b' || key === 'm') { event.preventDefault(); setTool(key === 'b' ? 'brush' : event.shiftKey ? 'ellipse' : 'rectangle'); }
          if (key === 'a' || key === 'v' || key === 'p' || key === 'n' || key === 'u') {
            event.preventDefault();
            setTool(key === 'a' ? 'vectorDirectSelect' : key === 'v' ? 'vectorSelect' : key === 'p' ? 'vectorPen' : key === 'n' ? 'vectorPencil' : event.shiftKey ? 'vectorEllipse' : 'vectorRectangle');
          }
          if (key === 't') { event.preventDefault(); setTool('text'); showTextPanel(); }
          if (key === 'x' && !event.repeat && !event.isComposing) { event.preventDefault(); swapColors(); }
          if (key === 'escape') { event.preventDefault(); void edit('deselect'); }
        }
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); void edit(event.shiftKey ? 'redo' : 'undo');
      }
    };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [activeDocumentId, documentAction, edit, file, documentAvailable, settingsOpen, colorSettingsOpen, newDocumentOpen, importImageOpen, directControlOpen, placingImage, finishPlacement, swapColors, toolMode, showTextPanel, inlineText, changeGroup]);

  return <div className="workspace" data-tool-mode={toolMode} data-panels={panels ? 'open' : 'closed'}>
    <header className="application-bar">
      <AppMenu locale={locale} onSettings={openSettings} onError={setError} />
      <WorkspaceMenu outlineDisplay={outlineDisplay} onOutlineDisplay={value => { void outlineView(value).then(setOutlineDisplay).catch(error => setError(String(error))); }} locale={locale} document={documentState} canFile={!fileBusy && !placingImage} hasDocument={documentAvailable} canEdit={documentEditable && ready && !busy && !fileBusy}
        zoom={zoom} panels={panels} onFile={action => void file(action)} onImportImage={() => setImportImageOpen(true)} onImportSvg={() => void importSvg()} onEdit={action => void edit(action)} onZoom={setZoom}
        onTransform={setTransformAction}
        onWritingMode={mode => { void setTextWritingMode(mode).then(updateDocument).catch(cause => setError(String(cause))); }}
        onOutlineText={() => { void outlineText().then(updateDocument).catch(cause => setError(String(cause))); }}
        onArrange={action => { void arrangeSelectedVectors(action).then(updateDocument).catch(cause => setError(String(cause))); }}
        onCompound={release => { void compoundPath(release).then(updateDocument).catch(cause => setError(String(cause))); }}
        onClipping={action => { void clippingPath(action).then(snapshot => { updateDocument(snapshot); if (action === 'edit') setTool('vectorDirectSelect'); }).catch(cause => setError(String(cause))); }}
        onGroup={action => void changeGroup(action)}
        onPathEdit={action => void editPath(action)}
        onNew={() => void documentAction('new')} onCloseDocument={() => activeDocumentId !== null && void documentAction('close', activeDocumentId)}
        onColorMode={mode => void setColorMode(mode)}
        onBitDepth={depth => void setBitDepth(depth)}
        onColorSettings={openColorSettings}
        onPanels={() => setPanels(value => !value)} onReset={() => { setPanels(window.innerWidth > 720); setZoom(1); }} onError={setError} />
      <div className="workspace-preferences">
        <button className="icon-button" title={t.panels} aria-label={t.panels} aria-pressed={panels} onClick={() => setPanels(value => !value)}><Icon name="panels" /></button>
      </div>
    </header>
    <div className="options-bar" aria-label={zoomTool ? common[zoomTool] : t[toolState.tools[toolMode]]}>
      <span className="current-tool"><Icon name={canvasTool} /><span><small className="current-mode">{t[modeLabels[toolMode]]}</small>{zoomTool ? common[zoomTool] : canvasTool === 'vectorDirectSelect' ? t.vectorDirectSelect : t[toolState.tools[toolMode]]}</span></span>
      {documentState.selectedVectorObjects.length > 0 && !inlineText && !zoomTool && canvasTool !== 'vectorDirectSelect' ? <SelectionOptions document={documentState} locale={locale} enabled={ready && !busy && documentEditable}
        onAppearance={async (opacity, blendMode) => { updateDocument(await setVectorAppearance([...documentState.selectedVectorObjects], opacity, blendMode)); }}
        onPaint={async (target,color) => { updateDocument(await setVectorPaint([...documentState.selectedVectorObjects],target,color)); }}
        onWidth={async width => { updateDocument(await setVectorStrokeWidth(width,brush.color)); }}
        onTransform={async (action,values) => { updateDocument(await transformObjects(action,values)); }}
        onCombine={async operation => { updateDocument(await combineSelectedVectors(operation)); }} onTransformMenu={setTransformAction} onError={setError} /> : zoomTool ? <span className="selection-hint">{zoomTool === 'hand' ? t.handHint : t.zoomClickHint}</span> : canvasTool.startsWith('vector') ? <><span className="selection-hint">{canvasTool === 'vectorSelect' && documentState.selectedVectorObjects.length > 0 ? `${documentState.selectedVectorObjects.length} ${t.vectorSelected}` : canvasTool === 'vectorRotate' ? t.rotateHint : canvasTool === 'vectorScale' ? t.scaleHint : canvasTool === 'vectorDirectSelect' ? t.directHint : canvasTool.startsWith('vectorAnchor') ? t.anchorHint : canvasTool === 'vectorPen' ? t.penHint : toolMode === 'layout' ? t.layoutHint : t.vectorHint}</span>{canvasTool === 'vectorDirectSelect' && <button disabled={!documentEditable || busy} onClick={()=>setDirectControlOpen(true)}>{directControlLabels[locale].title}</button>}{canvasTool === 'vectorSelect' && <select className="path-operations" aria-label={t.pathOperations} title={t.pathOperations} value="" disabled={!ready || busy || documentState.selectedVectorObjects.length !== 2} onChange={event => { const operation = event.target.value as PathOperation; event.currentTarget.value = ''; void combineVectors(operation); }}><option value="">{t.pathOperations}</option><option value="union">{t.pathUnion}</option><option value="difference">{t.pathDifference}</option><option value="intersection">{t.pathIntersection}</option><option value="xor">{t.pathXor}</option></select>}</> : (canvasTool === 'brush' || canvasTool === 'eraser') ? <>
      <label className="size-control">{t.size}<SizeInput label={t.size} value={brush.size} onChange={size => setBrush(previous => ({ ...previous, size }))} /></label>
      <label className="hardness-control">{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => setBrush(previous => ({ ...previous, hardness }))} /></label>
      <label className="color-control"><span>{t.foreground}</span><ColorPickerPopover locale={locale} color={brush.color} label={t.foreground} onChange={changeForeground} /></label>
      </> : <span className="selection-hint">{canvasTool === 'text' ? textPanelMessages[locale].hint : canvasTool === 'textFrame' ? t.textFrameHint : t.selectionHint}</span>}
      {toolMode === 'animation' && <span className="selection-hint animation-hint">{t.animationHint}</span>}
      {documentState.selection && <button className="selection-clear" disabled={!ready || busy} onClick={() => void edit('deselect')}>{t.deselect}</button>}
      <p className="session-note" role="status">{fileBusy ? t.fileBusy : !documentAvailable ? t.noDocument : !documentEditable ? t.tiledReadOnly : documentState.dirty ? t.sessionOnly : documentState.fileName ? t.saved : t.empty}</p>
    </div>
    <main className="editor-layout">
      {directControlOpen && <DirectControlDialog locale={locale} doc={documentState} onApply={updateDocument} onClose={()=>setDirectControlOpen(false)} />}
      {transformAction && <TransformDialog key={transformAction} action={transformAction} locale={locale} onClose={()=>setTransformAction(null)} onApply={async values=>{updateDocument(await transformObjects(transformAction,values));}} />}
      <nav className="tool-rail" aria-label={t.tools}>
        <ToolModeSwitch mode={toolMode} locale={locale} onChange={setToolMode} />
        <span className="tool-mode-divider" aria-hidden="true" />
        {/* Shared tools always occupy the left column; mode-only tools the right. */}
        <div className="tool-columns">
        <div className="common-tools">
        <IconToolMenu key="vector-selection" label={t.vectorSelect} selected={canvasTool === 'vectorSelect' || canvasTool === 'vectorDirectSelect' ? canvasTool : lastVectorSelectTool} active={canvasTool === 'vectorSelect' || canvasTool === 'vectorDirectSelect'} enabled={documentEditable} choices={[{ id: 'vectorSelect', label: t.vectorSelect, icon: 'vectorSelect', shortcut: 'V' }, { id: 'vectorDirectSelect', label: t.vectorDirectSelect, icon: 'vectorDirectSelect', shortcut: 'A' }]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} />
        <SelectionToolMenu locale={locale} selected={selectionTool ?? lastSelectionTool} active={selectionTool !== null} enabled={documentEditable} onSelect={setTool} onError={setError} />
        <IconToolMenu key="vector-transform" label={t.transformTools} selected={canvasTool === 'vectorScale' || canvasTool === 'vectorRotate' ? canvasTool : lastTransformTool} active={canvasTool === 'vectorScale' || canvasTool === 'vectorRotate'} enabled={documentEditable} choices={[{ id: 'vectorScale', label: t.vectorScale, icon: 'vectorScale' }, { id: 'vectorRotate', label: t.vectorRotate, icon: 'vectorRotate' }, ...(['move','reflect','shear','individual','reset'] as const).map((action,i)=>({id:action,label:transformLabels[locale][action],icon:(['transformMove','transformReflect','transformShear','transformEach','transformReset'] as const)[i]}))]} onSelect={tool => { if(tool==='vectorScale'||tool==='vectorRotate') setTool(tool); else setTransformAction(tool as TransformAction); }} onError={setError} />
        <ZoomToolMenu locale={locale} selected={zoomTool ?? lastZoomTool} active={zoomTool !== null} enabled={documentAvailable && ready} onSelect={setTool} onError={setError} />
        </div>
        <div className="mode-tools">
        {modeTools[toolMode].map(item => item === 'vectorEllipse' || item === 'textFrame' || (isPenTool(item) && item !== 'vectorPen') ? null : (item === 'brush' || item === 'eraser') && toolMode === 'paint' ?
          <IconToolMenu key={item} label={item === 'brush' ? t.drawTools : t.eraseTools} selected={item} active={canvasTool === item} enabled={documentEditable} choices={[{ id: item, label: t[item], icon: item, shortcut: item === 'brush' ? 'B' : 'E' }]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} /> :
          item === 'vectorPen' ? <IconToolMenu key="pen-tools" label={t.penTools} selected={isPenTool(canvasTool) ? canvasTool : lastPenTool} active={isPenTool(canvasTool)} enabled={documentEditable} choices={penTools.map(tool => ({ id: tool, label: t[tool], icon: tool, shortcut: tool === 'vectorPen' ? 'P' : tool === 'vectorPencil' ? 'N' : undefined })) as [IconToolChoice, ...IconToolChoice[]]} onSelect={tool => setTool(tool as CanvasTool)} onError={setError} /> :
          item === 'vectorRectangle' ?
          <VectorShapeToolMenu key="vector-shapes" locale={locale} selected={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse' ? canvasTool : lastVectorShapeTool} active={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse'} enabled={documentEditable} onSelect={setTool} onError={setError} /> :
          item === 'text' ? <IconToolMenu key="text-tools" label={t.textTool} selected={canvasTool === 'text' || canvasTool === 'textFrame' ? canvasTool : lastTextTool} active={canvasTool === 'text' || canvasTool === 'textFrame'} enabled={documentEditable} choices={[{ id: 'text', label: t.text, icon: 'text', shortcut: 'T' }, { id: 'textFrame', label: t.textFrame, icon: 'textFrame' }]} onSelect={tool => { setTool(tool as CanvasTool); showTextPanel(); }} onError={setError} /> :
          <button key={item} className={`tool-button${canvasTool === item ? ' selected' : ''}`} aria-label={t[item]} title={t[item]} aria-pressed={canvasTool === item} disabled={!documentEditable} onClick={() => setTool(item)}><Icon name={item} /></button>)}
        {(toolMode === 'vector' || toolMode === 'layout') && <button className="tool-button" aria-label={t.importVector} title={t.importVector} disabled={!documentEditable || fileBusy} onClick={() => void importSvg()}><Icon name="importVector" /></button>}
        {toolMode === 'animation' && <button className="tool-button" disabled aria-label={`${t.timelineTool} · ${t.toolPlanned}`} title={t.animationHint}><Icon name="timeline" /></button>}
        </div>
        <div className="tool-color-pickers" role="group" aria-label={`${t.foreground} · ${t.background}`}>
          <ColorPickerPopover locale={locale} color={backgroundColor} label={t.background} target="background" onOpen={() => setActiveColor('background')} onChange={setBackgroundColor} />
          <ColorPickerPopover locale={locale} color={brush.color} label={t.foreground} onOpen={() => setActiveColor('foreground')} onChange={changeForeground} />
          <button type="button" className="tool-color-swap" onClick={swapColors} title={`${t.swapColors} (X)`} aria-label={t.swapColors}>↔</button>
        </div>
        </div>
      </nav>
      <div className="document-area">
        <div className="document-tabs">
          <div className="document-tab-list" role="tablist" aria-label={t.openDocuments}>
            {documents.map((document, index) => <div key={document.id} className="document-tab-shell" aria-current={document.id === activeDocumentId}>
              <button type="button" role="tab" aria-selected={document.id === activeDocumentId} className="document-tab" title={document.format === 'tiled' ? t.recoveryTiled : t.recoveryLegacy} onClick={() => void documentAction('switch', document.id)}>
                <span>{document.fileName ?? `${t.untitledBase}-${index + 1}`}</span>{document.format === 'tiled' && <span className="document-format" aria-label={t.recoveryTiled}>T</span>}{document.dirty && <span className="unsaved-dot" aria-label={t.sessionOnly} />}
              </button>
              <button type="button" className="document-close" aria-label={`${document.fileName ?? `${t.untitledBase}-${index + 1}`} · ${t.closeDocument}`} onClick={() => void documentAction('close', document.id)}>×</button>
            </div>)}
            {!documentAvailable && <span className="no-document">{t.noDocument}</span>}
          </div>
          {documentAvailable && <span className="document-dimensions">{documentState.width} × {documentState.height} · {documentState.colorMode.toUpperCase()} · {documentState.bitDepth} bits</span>}
        </div>
        <CanvasPreview channel={channel} locale={locale} theme={theme} brush={brush} tool={canvasTool} zoom={zoom} hasDocument={documentAvailable} visible={documentAvailable && !settingsOpen && !colorSettingsOpen && !newDocumentOpen && !importImageOpen && !transformAction && !directControlOpen} occlusion={colorPickerOcclusion}
          footerAccessory={placingImage ? <div className="image-placement-controls">
            <span>{ {ja:'画像を配置：辺・角で拡大縮小、角の外側で回転', en:'Place image: resize with handles, rotate outside corners', 'zh-CN':'放置图片：拖动控制点缩放，在角外旋转'}[locale] }</span>
            <button disabled={placementBusy} onClick={() => void finishPlacement(false)}>{ {ja:'キャンセル',en:'Cancel','zh-CN':'取消'}[locale] }</button>
            <button disabled={placementBusy} onClick={() => void finishPlacement(true)}>{ {ja:'確定',en:'Confirm','zh-CN':'确认'}[locale] }</button>
          </div> : <RecoveryControls locale={locale} document={documentState} onDocument={updateDocument} />}
          onZoom={setZoom} onDocument={updateDocument} onReady={setReady} />
      </div>
      {panels && <Inspector thumbnailDocumentKey={activeDocumentId === null ? '' : String(activeDocumentId)} onSavedPathAction={async (action, id, name) => { updateDocument(await savedPathAction(action, id, name)); }} vectorColors={vectorColors} channel={channel} onChannel={setChannel} onStrokeStyle={async patch => { updateDocument(await setVectorStrokeStyle(patch)); }} onStrokeWidth={async width => { updateDocument(await setVectorStrokeWidth(width, brush.color)); }} textPanelRequest={textPanelRequest} textSettings={activeText} textEditing={inlineText !== null}
        textEnabled={documentEditable && ready && !busy && !fileBusy && (inlineText !== null || !selectedText || selectedText.editable)} onTextChange={changeText} onTextBegin={beginText} onTextFinish={endText} locale={locale} brush={brush} backgroundColor={backgroundColor} activeColor={activeColor} onSelectColor={setActiveColor} colorPanelRequest={colorPanelRequest} onBrush={setBrush} onForegroundChange={changeForeground} onBackgroundChange={setBackgroundColor} onSwapColors={swapColors} document={documentState} enabled={documentEditable && ready && !busy}
        onDocumentSettings={settings => void setDocumentSettings(settings)} onColorMode={mode => void setColorMode(mode)} onBitDepth={depth => void setBitDepth(depth)} onColorProfile={profile => void setColorProfile(profile)} onToggleLayer={id => void setLayerVisibility(id)} onLayerSettings={settings => void setLayerSettings(settings)} onDeleteLayer={id => void removeLayer(id)} onSelectLayer={id => { void selectLayer(id, true).then(updateDocument).catch(cause => setError(String(cause))); }} onSelectObject={(layerId, objectId) => { void selectLayer(layerId).then(() => selectVectorObjects([objectId])).then(updateDocument).catch(cause => setError(String(cause))); }} onToggleObject={(layerId, objectId, visible) => { void setVectorObjectVisibility(layerId, objectId, visible).then(updateDocument).catch(cause => setError(String(cause))); }} onReorderObjects={(layerId, ids) => { void reorderVectorObjects(layerId, ids).then(updateDocument).catch(cause => setError(String(cause))); }} onAddLayer={() => void createLayer('paint')} onAddVectorLayer={() => void createLayer('vector')} onReorderLayer={ids => void moveLayer(ids)} />}
    </main>
    {error && <div className="workspace-error" role="alert">{error}<button aria-label={common.dismiss} onClick={() => setError('')}>×</button></div>}
    {settingsOpen && <SettingsDialog locale={locale} theme={theme} onLocale={setLocale} onTheme={setTheme} onClose={closeSettings} />}
    {importImageOpen && <ImportImageDialog locale={locale} onClose={() => setImportImageOpen(false)} onImport={async format => {
      if (filePending.current) throw new Error('Another file operation is in progress');
      filePending.current = true; setFileBusy(true);
      try { updateDocument(await importRasterLayer(format)); }
      finally { filePending.current = false; setFileBusy(false); }
    }} />}
    {newDocumentOpen && <NewDocumentDialog locale={locale} onCreate={createFromPreset} onClose={() => setNewDocumentOpen(false)} />}
    {colorSettingsOpen && <ColorSettingsDialog locale={locale} document={documentState} enabled={ready && !busy} onProfile={profile => void setColorProfile(profile)} onClose={closeColorSettings} />}
  </div>;
}
