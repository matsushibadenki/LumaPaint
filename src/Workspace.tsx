import { reorderVectorObjects, selectVectorObjects, setVectorObjectVisibility, selectLayer, addVectorLayer } from './bridge';
import { useCallback, useEffect, useRef, useState, type SetStateAction } from 'react';
import { CanvasPreview } from './CanvasPreview';
import { subscribeCanvasZoom } from './bridge';
import { beginTextEdit, updateTextEdit, finishTextEdit, subscribeTextSession, defaultVectorText, type TextSettings, subscribeCanvasText, subscribeCanvasColorSwap, subscribeCanvasTool, type CanvasTool, type DocumentEditAction, addPaintLayer, changeBitDepth, changeColorMode, changeColorProfile, changeDocumentSettings, closeDocument, combineSelectedVectors, createDocument, deleteLayer, editDocument, getDocumentWorkspace, importSvgLayer, projectAction, reorderLayers, switchDocument, toggleLayer, updateLayer, emptyDocument, subscribeDocument, subscribeDocuments, type BitDepth, type Brush, type ColorMode, type ColorProfile, type DocumentSettings, type DocumentSnapshot, type DocumentTabSnapshot, type DocumentWorkspaceSnapshot, type LayerSettings, type PathOperation } from './bridge';
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
import { initialTools, modeForTool, modeLabels, modeTools, type ToolMode } from './tool-modes';
import { PercentInput, SizeInput, fromHex, toHex } from './components/BrushControls';
import { ColorPairControl, type ColorTarget } from './components/ColorPanel';

export function Workspace() {
  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [theme, setTheme] = useState<Theme>(() => readPreference('theme') ? initialTheme() : 'dark');
  const [toolState, setToolState] = useState({ mode: 'paint' as ToolMode, tools: initialTools });
  const toolMode = toolState.mode;
  const [zoomTool, setZoomTool] = useState<ZoomTool | null>(null);
  const [lastZoomTool, setLastZoomTool] = useState<ZoomTool>('zoomIn');
  const canvasTool = zoomTool ?? toolState.tools[toolMode];
  const [lastSelectionTool, setLastSelectionTool] = useState<SelectionTool>('rectangle');
  const [lastVectorShapeTool, setLastVectorShapeTool] = useState<VectorShapeTool>('vectorRectangle');
  const setToolMode = useCallback((mode: ToolMode) => {
    setZoomTool(null);
    setToolState(current => ({ ...current, mode }));
  }, []);
  const setTool = useCallback((next: CanvasTool) => {
    if (next === 'zoomIn' || next === 'zoomOut' || next === 'hand') { setZoomTool(next); setLastZoomTool(next); return; }
    setZoomTool(null);
    setToolState(current => {
      const mode = modeForTool(current.mode, next);
      return { mode, tools: { ...current.tools, [mode]: next } };
    });
    if (next === 'rectangle' || next === 'ellipse') setLastSelectionTool(next);
    if (next === 'vectorRectangle' || next === 'vectorEllipse') setLastVectorShapeTool(next);
  }, []);
  const [paintState, setPaintState] = useState<{ brush: Brush; backgroundColor: Brush['color'] }>({
    brush: { size: 16, hardness: 1, color: [32, 32, 32] }, backgroundColor: [255, 255, 255],
  });
  const [activeColor, setActiveColor] = useState<ColorTarget>('foreground');
  const [colorPanelRequest, setColorPanelRequest] = useState(0);
  const { brush, backgroundColor } = paintState;
  const setBrush = useCallback((next: SetStateAction<Brush>) => {
    setPaintState(current => ({ ...current, brush: typeof next === 'function' ? next(current.brush) : next }));
  }, []);
  const setBackgroundColor = useCallback((color: Brush['color']) => {
    setPaintState(current => ({ ...current, backgroundColor: color }));
  }, []);
  const swapColors = useCallback(() => {
    setPaintState(current => ({
      brush: { ...current.brush, color: current.backgroundColor }, backgroundColor: current.brush.color,
    }));
  }, []);
  useEffect(() => {
    let active = true;
    let stop = () => {};
    subscribeCanvasColorSwap(() => { if (active) swapColors(); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [swapColors]);
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
  const [panels, setPanels] = useState(() => window.innerWidth > 720);
  const [inlineText, setInlineText] = useState<TextSettings | null>(null);
  const textSessionActive = useRef(false);
  const [textPanelRequest, setTextPanelRequest] = useState(0);
  const showTextPanel = useCallback(() => { setPanels(true); setTextPanelRequest(value => value + 1); }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [colorSettingsOpen, setColorSettingsOpen] = useState(false);
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const openColorSettings = useCallback(() => setColorSettingsOpen(true), []);
  const closeColorSettings = useCallback(() => setColorSettingsOpen(false), []);
  const t = workspaceMessages[locale];
  const common = messages[locale];
  const documentEditable = documentAvailable && documents.find(document => document.id === activeDocumentId)?.format !== 'tiled';
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
    subscribeDocuments(next => { if (active) updateWorkspace(next); })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, [updateWorkspace]);

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
      setInlineText(settings);
      if (settings && !wasEditing) showTextPanel();
      wasEditing = settings !== null;
    }).then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); }).catch(cause => setError(String(cause)));
    return () => { active = false; stop(); };
  }, [showTextPanel]);
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
    filePending.current = true; setFileBusy(true); setError('');
    try {
      const next = action === 'new' ? await createDocument() : action === 'switch' ? await switchDocument(id!) : await closeDocument(id!);
      updateWorkspace(next);
    } catch (cause) { setError(String(cause)); }
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

      if ((event.metaKey || event.ctrlKey) && ['s', 'o', 'w', 'n'].includes(event.key.toLowerCase())) {
        event.preventDefault();
        const key = event.key.toLowerCase();
        if (key === 'n') void documentAction('new');
        else if (key === 'w' && activeDocumentId !== null) void documentAction('close', activeDocumentId);
        else void file(key === 'o' ? 'open' : event.shiftKey ? 'saveAs' : 'save');
        return;
      }
      const target = event.target as HTMLElement;
      if (target.closest('input, textarea, select, [contenteditable=true]')) return;
      if (documentAvailable && !settingsOpen && !colorSettingsOpen) {
        const key = event.key.toLowerCase();
        if ((event.metaKey || event.ctrlKey) && (key === 'a' || key === 'd' || (key === 'i' && event.shiftKey))) {
          event.preventDefault(); void edit(key === 'a' ? 'selectAll' : key === 'd' ? 'deselect' : 'invertSelection');
          return;
        }
        if (!event.metaKey && !event.ctrlKey && !event.altKey) {
          if (!inlineText && (event.key === 'Delete' || event.key === 'Backspace')) {
            event.preventDefault(); void edit('deleteSelectedObjects'); return;
          }
          if (key === 'b' || key === 'm') { event.preventDefault(); setTool(key === 'b' ? 'brush' : event.shiftKey ? 'ellipse' : 'rectangle'); }
          if (key === 'v' || key === 'p' || key === 'u') {
            event.preventDefault();
            setTool(key === 'v' ? 'vectorSelect' : key === 'p' ? 'vectorPen' : event.shiftKey ? 'vectorEllipse' : 'vectorRectangle');
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
  }, [activeDocumentId, documentAction, edit, file, documentAvailable, settingsOpen, colorSettingsOpen, swapColors, toolMode, showTextPanel, inlineText]);

  return <div className="workspace" data-tool-mode={toolMode} data-panels={panels ? 'open' : 'closed'}>
    <header className="application-bar">
      <AppMenu locale={locale} onSettings={openSettings} onError={setError} />
      <WorkspaceMenu locale={locale} document={documentState} canFile={!fileBusy} hasDocument={documentAvailable} canEdit={documentEditable && ready && !busy && !fileBusy}
        zoom={zoom} panels={panels} onFile={action => void file(action)} onImportSvg={() => void importSvg()} onEdit={action => void edit(action)} onZoom={setZoom}
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
      <span className="current-tool"><Icon name={canvasTool} /><span><small className="current-mode">{t[modeLabels[toolMode]]}</small>{zoomTool ? common[zoomTool] : t[toolState.tools[toolMode]]}</span></span>
      {zoomTool ? <span className="selection-hint">{zoomTool === 'hand' ? t.handHint : t.zoomClickHint}</span> : canvasTool.startsWith('vector') ? <><span className="selection-hint">{canvasTool === 'vectorSelect' && documentState.selectedVectorObjects.length > 0 ? `${documentState.selectedVectorObjects.length} ${t.vectorSelected}` : toolMode === 'layout' ? t.layoutHint : t.vectorHint}</span>{canvasTool === 'vectorSelect' && <select className="path-operations" aria-label={t.pathOperations} title={t.pathOperations} value="" disabled={!ready || busy || documentState.selectedVectorObjects.length !== 2} onChange={event => { const operation = event.target.value as PathOperation; event.currentTarget.value = ''; void combineVectors(operation); }}><option value="">{t.pathOperations}</option><option value="union">{t.pathUnion}</option><option value="difference">{t.pathDifference}</option><option value="intersection">{t.pathIntersection}</option><option value="xor">{t.pathXor}</option></select>}</> : canvasTool === 'brush' ? <>
      <label className="size-control">{t.size}<SizeInput label={t.size} value={brush.size} onChange={size => setBrush(previous => ({ ...previous, size }))} /></label>
      <label className="hardness-control">{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => setBrush(previous => ({ ...previous, hardness }))} /></label>
      <label className="color-control"><span>{t.foreground}</span><input type="color" value={toHex(brush.color)} onChange={event => setBrush(previous => ({ ...previous, color: fromHex(event.target.value) }))} aria-label={t.foreground} /></label>
      </> : <span className="selection-hint">{canvasTool === 'text' ? textPanelMessages[locale].hint : t.selectionHint}</span>}
      {toolMode === 'animation' && <span className="selection-hint animation-hint">{t.animationHint}</span>}
      {documentState.selection && <button className="selection-clear" disabled={!ready || busy} onClick={() => void edit('deselect')}>{t.deselect}</button>}
      <p className="session-note" role="status">{fileBusy ? t.fileBusy : !documentAvailable ? t.noDocument : !documentEditable ? t.tiledReadOnly : documentState.dirty ? t.sessionOnly : documentState.fileName ? t.saved : t.empty}</p>
    </div>
    <main className="editor-layout">
      <nav className="tool-rail" aria-label={t.tools}>
        <ToolModeSwitch mode={toolMode} locale={locale} onChange={setToolMode} />
        <span className="tool-mode-divider" aria-hidden="true" />
        {modeTools[toolMode].map(item => item === 'ellipse' || item === 'vectorEllipse' ? null : item === 'rectangle' ?
          <SelectionToolMenu key="selection" locale={locale} selected={canvasTool === 'rectangle' || canvasTool === 'ellipse' ? canvasTool : lastSelectionTool} active={canvasTool === 'rectangle' || canvasTool === 'ellipse'} enabled={documentEditable} onSelect={setTool} onError={setError} /> :
          item === 'vectorRectangle' ?
          <VectorShapeToolMenu key="vector-shapes" locale={locale} selected={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse' ? canvasTool : lastVectorShapeTool} active={canvasTool === 'vectorRectangle' || canvasTool === 'vectorEllipse'} enabled={documentEditable} onSelect={setTool} onError={setError} /> :
          <button key={item} className={`tool-button${canvasTool === item ? ' selected' : ''}`} aria-label={t[item]} title={`${t[item]} (${item === 'text' ? 'T' : item === 'brush' ? 'B' : item === 'vectorSelect' ? 'V' : item === 'vectorPen' ? 'P' : item === 'vectorEllipse' ? 'Shift＋U' : 'U'})`} aria-pressed={canvasTool === item} disabled={!documentEditable} onClick={() => { setTool(item); if (item === 'text') showTextPanel(); }}><Icon name={item} /></button>)}
        {(toolMode === 'vector' || toolMode === 'layout') && <button className="tool-button" aria-label={t.importVector} title={t.importVector} disabled={!documentEditable || fileBusy} onClick={() => void importSvg()}><Icon name="importVector" /></button>}
        {toolMode === 'animation' && <button className="tool-button" disabled aria-label={`${t.timelineTool} · ${t.toolPlanned}`} title={t.animationHint}><Icon name="timeline" /></button>}
        <div className="common-tools">
        <ZoomToolMenu locale={locale} selected={zoomTool ?? lastZoomTool} active={zoomTool !== null} enabled={documentAvailable && ready} onSelect={setTool} onError={setError} />
        <ColorPairControl locale={locale} foreground={brush.color} background={backgroundColor} compact activeColor={activeColor} onSelectColor={target => { setActiveColor(target); setPanels(true); setColorPanelRequest(current => current + 1); }} onSwap={swapColors} />
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
        <CanvasPreview locale={locale} theme={theme} brush={brush} tool={canvasTool} zoom={zoom} hasDocument={documentAvailable} visible={documentAvailable && !settingsOpen && !colorSettingsOpen}
          footerAccessory={<RecoveryControls locale={locale} document={documentState} onDocument={updateDocument} />}
          onZoom={setZoom} onDocument={updateDocument} onReady={setReady} />
      </div>
      {panels && <Inspector textPanelRequest={textPanelRequest} textSettings={activeText} textEditing={inlineText !== null}
        textEnabled={documentEditable && ready && !busy && !fileBusy && (inlineText !== null || !selectedText || selectedText.editable)} onTextChange={changeText} onTextBegin={beginText} onTextFinish={endText} locale={locale} brush={brush} backgroundColor={backgroundColor} activeColor={activeColor} onSelectColor={setActiveColor} colorPanelRequest={colorPanelRequest} onBrush={setBrush} onBackgroundChange={setBackgroundColor} onSwapColors={swapColors} document={documentState} enabled={documentEditable && ready && !busy}
        onDocumentSettings={settings => void setDocumentSettings(settings)} onColorMode={mode => void setColorMode(mode)} onBitDepth={depth => void setBitDepth(depth)} onColorProfile={profile => void setColorProfile(profile)} onToggleLayer={id => void setLayerVisibility(id)} onLayerSettings={settings => void setLayerSettings(settings)} onDeleteLayer={id => void removeLayer(id)} onSelectLayer={id => { void selectLayer(id).then(updateDocument).catch(cause => setError(String(cause))); }} onSelectObject={(layerId, objectId) => { void selectLayer(layerId).then(() => selectVectorObjects([objectId])).then(updateDocument).catch(cause => setError(String(cause))); }} onToggleObject={(layerId, objectId, visible) => { void setVectorObjectVisibility(layerId, objectId, visible).then(updateDocument).catch(cause => setError(String(cause))); }} onReorderObjects={(layerId, ids) => { void reorderVectorObjects(layerId, ids).then(updateDocument).catch(cause => setError(String(cause))); }} onAddLayer={() => void createLayer('paint')} onAddVectorLayer={() => void createLayer('vector')} onReorderLayer={ids => void moveLayer(ids)} />}
    </main>
    {error && <div className="workspace-error" role="alert">{error}<button aria-label={common.dismiss} onClick={() => setError('')}>×</button></div>}
    {settingsOpen && <SettingsDialog locale={locale} theme={theme} onLocale={setLocale} onTheme={setTheme} onClose={closeSettings} />}
    {colorSettingsOpen && <ColorSettingsDialog locale={locale} document={documentState} enabled={ready && !busy} onProfile={profile => void setColorProfile(profile)} onClose={closeColorSettings} />}
  </div>;
}
