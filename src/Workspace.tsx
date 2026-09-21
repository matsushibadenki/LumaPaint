import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { CanvasPreview } from './CanvasPreview';
import { subscribeCanvasTool, type CanvasTool, type DocumentEditAction, addPaintLayer, changeBitDepth, changeColorMode, changeColorProfile, changeDocumentSettings, closeDocument, createDocument, deleteLayer, editDocument, getDocumentWorkspace, importSvgLayer, projectAction, reorderLayers, switchDocument, toggleLayer, updateLayer, emptyDocument, subscribeDocument, subscribeDocuments, type BitDepth, type Brush, type ColorMode, type ColorProfile, type DocumentSettings, type DocumentSnapshot, type DocumentTabSnapshot, type DocumentWorkspaceSnapshot, type LayerSettings } from './bridge';
import { initialLocale, initialTheme, messages, readPreference, savePreference, type Locale, type Theme } from './i18n';
import { workspaceMessages } from './workspace-i18n';
import { RecoveryControls } from './components/RecoveryControls';
import { WorkspaceMenu } from './components/WorkspaceMenu';
import { AppMenu } from './components/AppMenu';
import { SettingsDialog } from './components/SettingsDialog';
import { ColorSettingsDialog } from './components/ColorSettingsDialog';
import { Inspector } from './components/Inspector';
import { Icon } from './components/Icon';
import { SelectionToolMenu, type SelectionTool } from './components/SelectionToolMenu';
import { ToolModeSwitch, type ToolMode } from './components/ToolModeSwitch';
import { PercentInput, SizeInput, fromHex, toHex } from './components/BrushControls';

export function Workspace() {
  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [theme, setTheme] = useState<Theme>(() => readPreference('theme') ? initialTheme() : 'dark');
  const [toolMode, setToolMode] = useState<ToolMode>('paint');
  const [tool, updateTool] = useState<'brush' | 'rectangle' | 'ellipse'>('brush');
  const [vectorTool, setVectorTool] = useState<'vectorSelect' | 'vectorPen' | 'vectorRectangle' | 'vectorEllipse'>('vectorSelect');
  const [lastSelectionTool, setLastSelectionTool] = useState<SelectionTool>('rectangle');
  const setTool = useCallback((next: 'brush' | 'rectangle' | 'ellipse') => {
    setToolMode('paint');
    updateTool(next);
    if (next !== 'brush') setLastSelectionTool(next);
  }, []);
  const canvasTool: CanvasTool = toolMode === 'vector' ? vectorTool : tool;
  const [brush, setBrush] = useState<Brush>({ size: 16, hardness: 1, color: [32, 32, 32] });
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [colorSettingsOpen, setColorSettingsOpen] = useState(false);
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const openColorSettings = useCallback(() => setColorSettingsOpen(true), []);
  const closeColorSettings = useCallback(() => setColorSettingsOpen(false), []);
  const t = workspaceMessages[locale];
  const common = messages[locale];
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
      if (next.startsWith('vector')) { setToolMode('vector'); setVectorTool(next as typeof vectorTool); }
      else setTool(next as typeof tool);
    })
      .then(unsubscribe => { if (active) stop = unsubscribe; else unsubscribe(); })
      .catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; stop(); };
  }, []);

  const edit = useCallback(async (action: DocumentEditAction) => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await editDocument(action)); } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [ready, busy, updateDocument]);

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
    if (!documentAvailable || filePending.current) return;
    filePending.current = true; setFileBusy(true); setError('');
    try { updateDocument(await importSvgLayer()); }
    catch (cause) { setError(String(cause)); }
    finally { filePending.current = false; setFileBusy(false); }
  }, [documentAvailable, updateDocument]);

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
  const createLayer = useCallback(async () => {
    if (!ready || busy) return;
    setBusy(true); setError('');
    try { updateDocument(await addPaintLayer()); } catch (cause) { setError(String(cause)); }
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
          if (key === 'b' || key === 'm') { event.preventDefault(); setTool(key === 'b' ? 'brush' : event.shiftKey ? 'ellipse' : 'rectangle'); }
          if (key === 'v' || key === 'p' || key === 'u') {
            event.preventDefault();
            setToolMode('vector');
            setVectorTool(key === 'v' ? 'vectorSelect' : key === 'p' ? 'vectorPen' : event.shiftKey ? 'vectorEllipse' : 'vectorRectangle');
          }
          if (key === 'escape') { event.preventDefault(); void edit('deselect'); }
        }
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); void edit(event.shiftKey ? 'redo' : 'undo');
      }
    };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [activeDocumentId, documentAction, edit, file, documentAvailable, settingsOpen, colorSettingsOpen]);

  return <div className="workspace" data-panels={panels ? 'open' : 'closed'}>
    <header className="application-bar">
      <AppMenu locale={locale} onSettings={openSettings} onError={setError} />
      <WorkspaceMenu locale={locale} document={documentState} canFile={!fileBusy} hasDocument={documentAvailable} canEdit={documentAvailable && ready && !busy && !fileBusy}
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
    <div className="options-bar" aria-label={t[canvasTool]}>
      <span className="current-tool"><Icon name={canvasTool} />{t[canvasTool]}</span>
      {toolMode === 'vector' ? <span className="selection-hint">{t.vectorHint}</span> : tool === 'brush' ? <>
      <label className="size-control">{t.size}<SizeInput label={t.size} value={brush.size} onChange={size => setBrush(previous => ({ ...previous, size }))} /></label>
      <label className="hardness-control">{t.hardness}<PercentInput label={t.hardness} value={brush.hardness} onChange={hardness => setBrush(previous => ({ ...previous, hardness }))} /></label>
      <label className="color-control"><span>{t.foreground}</span><input type="color" value={toHex(brush.color)} onChange={event => setBrush(previous => ({ ...previous, color: fromHex(event.target.value) }))} aria-label={t.foreground} /></label>
      </> : <span className="selection-hint">{t.selectionHint}</span>}
      {documentState.selection && <button className="selection-clear" disabled={!ready || busy} onClick={() => void edit('deselect')}>{t.deselect}</button>}
      <p className="session-note" role="status">{fileBusy ? t.fileBusy : !documentAvailable ? t.noDocument : documentState.dirty ? t.sessionOnly : documentState.fileName ? t.saved : t.empty}</p>
    </div>
    <main className="editor-layout">
      <nav className="tool-rail" aria-label={t.tools}>
        <ToolModeSwitch mode={toolMode} paintLabel={t.paintTools} vectorLabel={t.vectorTools} switchLabel={t.switchToolMode} onChange={setToolMode} />
        <span className="tool-mode-divider" aria-hidden="true" />
        {toolMode === 'paint' ? <>
        <button className={`tool-button${tool === 'brush' ? ' selected' : ''}`} aria-label={t.brush} title={`${t.brush} (B)`} aria-pressed={tool === 'brush'} disabled={!documentAvailable} onClick={() => setTool('brush')}><Icon name="brush" /></button>
        <SelectionToolMenu locale={locale} selected={lastSelectionTool} active={tool !== 'brush'} enabled={documentAvailable} onSelect={setTool} onError={setError} />
        </> : <>
        {(['vectorSelect', 'vectorPen', 'vectorRectangle', 'vectorEllipse'] as const).map(item =>
          <button key={item} className={`tool-button${vectorTool === item ? ' selected' : ''}`} aria-label={t[item]} title={`${t[item]} (${item === 'vectorSelect' ? 'V' : item === 'vectorPen' ? 'P' : 'U'})`} aria-pressed={vectorTool === item} disabled={!documentAvailable} onClick={() => setVectorTool(item)}><Icon name={item} /></button>)}
        <button className="tool-button" aria-label={t.importVector} title={t.importVector} disabled={!documentAvailable || fileBusy} onClick={() => void importSvg()}><Icon name="importVector" /></button>
        </>}
        <button className="tool-button" aria-label={common.zoomIn} title={common.zoomIn} disabled={!ready || zoom >= 4} onClick={() => setZoom(value => Math.min(4, value * 1.25))}><Icon name="zoom" /></button>
        {toolMode === 'paint' && <span className="foreground-indicator" title={toHex(brush.color)} style={{ '--swatch': toHex(brush.color) } as CSSProperties} />}
      </nav>
      <div className="document-area">
        <div className="document-tabs">
          <div className="document-tab-list" role="tablist" aria-label={t.openDocuments}>
            {documents.map((document, index) => <div key={document.id} className="document-tab-shell" aria-current={document.id === activeDocumentId}>
              <button type="button" role="tab" aria-selected={document.id === activeDocumentId} className="document-tab" onClick={() => void documentAction('switch', document.id)}>
                <span>{document.fileName ?? `${t.untitledBase}-${index + 1}`}</span>{document.dirty && <span className="unsaved-dot" aria-label={t.sessionOnly} />}
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
      {panels && <Inspector locale={locale} brush={brush} onBrush={setBrush} document={documentState} enabled={documentAvailable && ready && !busy}
        onDocumentSettings={settings => void setDocumentSettings(settings)} onColorMode={mode => void setColorMode(mode)} onBitDepth={depth => void setBitDepth(depth)} onColorProfile={profile => void setColorProfile(profile)} onToggleLayer={id => void setLayerVisibility(id)} onLayerSettings={settings => void setLayerSettings(settings)} onDeleteLayer={id => void removeLayer(id)} onAddLayer={() => void createLayer()} onReorderLayer={ids => void moveLayer(ids)} />}
    </main>
    {error && <div className="workspace-error" role="alert">{error}<button aria-label={common.dismiss} onClick={() => setError('')}>×</button></div>}
    {settingsOpen && <SettingsDialog locale={locale} theme={theme} onLocale={setLocale} onTheme={setTheme} onClose={closeSettings} />}
    {colorSettingsOpen && <ColorSettingsDialog locale={locale} document={documentState} enabled={ready && !busy} onProfile={profile => void setColorProfile(profile)} onClose={closeColorSettings} />}
  </div>;
}
