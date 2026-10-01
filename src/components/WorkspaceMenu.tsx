import { transformLabels } from './TransformDialog';
import type { ArrangeAction, TransformAction } from '../bridge';
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { isTauri } from '@tauri-apps/api/core';
import { Menu } from '@tauri-apps/api/menu';
import type { CheckMenuItemOptions, MenuItemOptions, PredefinedMenuItemOptions, SubmenuOptions } from '@tauri-apps/api/menu';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Locale } from '../i18n';
import { messages } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { menuMessages } from '../menu-i18n';
import type { BitDepth, ColorMode, DocumentEditAction, DocumentSnapshot, PathEditAction } from '../bridge';

type EntryItem = { label: string; action?: () => void; enabled?: boolean; checked?: boolean; shortcut?: string; planned?: boolean; children?: Entry[] };
type Entry = EntryItem | null;
type NativeEntry = MenuItemOptions | CheckMenuItemOptions | SubmenuOptions | PredefinedMenuItemOptions;
// New modes are registered here; both native and browser submenus derive from this list.
const colorModes: { value: ColorMode; label: string }[] = [{ value: 'rgb', label: 'RGB' }, { value: 'cmyk', label: 'CMYK' }];
const bitDepths: { value: BitDepth; label: string }[] = [{ value: 8, label: '8 bits' }, { value: 16, label: '16 bits' }, { value: 32, label: '32 bits' }];
type Props = {
  outlineDisplay: boolean; onOutlineDisplay: (value: boolean) => void;
  locale: Locale; document: DocumentSnapshot; canFile: boolean; hasDocument: boolean; canEdit: boolean;
  zoom: number; panels: boolean; onFile: (action: 'open' | 'save' | 'saveAs') => void;
  onNewWindow: () => void; onNew: () => void; onCloseDocument: () => void;
  onImportSvg: () => void;
  onImportImage: () => void;
  onArrange: (action: ArrangeAction) => void;
  onTransform: (action: TransformAction) => void;
  onOutlineText: () => void;
  onWritingMode: (mode: 'horizontal' | 'vertical') => void;
  onCompound: (release: boolean) => void;
  onClipping: (action: 'create' | 'release' | 'edit') => void;
  onGroup: (action: 'group' | 'ungroup' | 'ungroupAll') => void;
  onPathEdit: (action: PathEditAction) => void;
  onEdit: (action: DocumentEditAction) => void; onZoom: (value: number) => void;
  onColorMode: (mode: ColorMode) => void; onBitDepth: (depth: BitDepth) => void;
  onColorSettings: () => void; onPanels: () => void; onReset: () => void; onError: (error: string) => void;
};

export function WorkspaceMenu(props: Props) {
  const { locale, document: doc, canFile, hasDocument, canEdit, zoom, panels, onFile, onNew, onCloseDocument, onImportSvg, onGroup, onPathEdit, onEdit, onZoom, onColorMode, onBitDepth, onColorSettings, onPanels, onReset, onError } = props;
  const clip = { ja: ['クリッピングパス', '作成', '削除', 'マスクを編集'], en: ['Clipping Path', 'Make', 'Release', 'Edit Mask'], 'zh-CN': ['剪切路径', '建立', '释放', '编辑蒙版'] }[locale];
  const t = menuMessages[locale], common = messages[locale], w = workspaceMessages[locale];
  const selectedObjects = doc.layers.flatMap(layer => layer.objects.filter(object => doc.selectedVectorObjects.includes(object.id)).map(object => ({ layer, object })));
  const selectedEntities = new Set(selectedObjects.map(item => `${item.layer.id}:${item.object.groupPath[0] ?? item.object.id}`));
  const selectedTexts = doc.textObjects.filter(text => doc.selectedVectorObjects.includes(text.id));
  const directionLabels = { ja: ['組み方向', '横組み', '縦組み'], en: ['Writing Direction', 'Horizontal', 'Vertical'], 'zh-CN': ['文字方向', '横排', '竖排'] }[locale];
  const arrangeLabels = {
    ja: ['重ね順', '最前面へ', '前面へ', '背面へ', '最背面へ', '選択しているレイヤーへ移動'],
    en: ['Arrange', 'Bring to Front', 'Bring Forward', 'Send Backward', 'Send to Back', 'Move to Selected Layer'],
    'zh-CN': ['排列', '置于顶层', '上移一层', '下移一层', '置于底层', '移动到所选图层'],
  }[locale];
  const canArrange = canEdit && selectedObjects.length > 0 && selectedObjects.every(({layer}) => layer.visible && !layer.locked);
  const destination = doc.layers.find(layer => layer.id === doc.layerId);
  const canMoveToLayer = canArrange && !doc.activeSavedPath && destination?.kind === 'vector' && destination.visible && !destination.locked;
  const canGroup = canEdit && selectedEntities.size >= 2 && new Set(selectedObjects.map(item => item.layer.id)).size === 1;
  const canUngroup = canEdit && selectedObjects.some(item => item.object.groupPath.length > 0);
  const future = (label: string): Entry => ({ label, planned: true });
  const menus: Entry[][] = [
    [{ label: t.new, enabled: canFile, shortcut: 'CmdOrCtrl+N', action: onNew }, { label: w.open + '…', enabled: canFile, shortcut: 'CmdOrCtrl+O', action: () => onFile('open') },
      { label: t.closeDocument, enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+W', action: onCloseDocument },
      { label: t.importSvg, enabled: canFile && hasDocument, action: onImportSvg }, null,
      { label: w.save, enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+S', action: () => onFile('save') },
      { label: w.saveAs + '…', enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+Shift+S', action: () => onFile('saveAs') }, null, { label: { ja: '読み込み…', en: 'Import…', 'zh-CN': '导入…' }[locale], enabled: canFile && hasDocument && canEdit, action: props.onImportImage }, future(t.export)],
    [{ label: w.undo, enabled: canEdit && doc.canUndo, shortcut: 'CmdOrCtrl+Z', action: () => onEdit('undo') },
      { label: w.redo, enabled: canEdit && doc.canRedo, shortcut: 'CmdOrCtrl+Shift+Z', action: () => onEdit('redo') }, null,
      { label: t.colorSettings, enabled: hasDocument, action: onColorSettings }, null, { label: t.cut, enabled: canEdit, shortcut: 'CmdOrCtrl+X', action: () => onEdit('cut') }, { label: t.copy, enabled: canEdit, shortcut: 'CmdOrCtrl+C', action: () => onEdit('copy') }, { label: t.paste, enabled: canEdit, shortcut: 'CmdOrCtrl+V', action: () => onEdit('paste') }, null,
      { label: t.clearLayer, enabled: canEdit && !doc.layers.find(layer => layer.id === doc.layerId)?.locked, action: () => onEdit('clearLayer') }],
    [{ label: t.colorMode, children: colorModes.map(mode => ({ label: mode.label, checked: doc.colorMode === mode.value, enabled: canEdit, action: () => onColorMode(mode.value) })) },
      { label: t.bitDepth, children: bitDepths.map(depth => ({ label: depth.label, checked: doc.bitDepth === depth.value, enabled: canEdit, action: () => onBitDepth(depth.value) })) }, null,
      future(t.imageSize), future(t.canvasSize), future(t.rotate)],
    [{ label: arrangeLabels[0], enabled: canArrange, children: (['front', 'forward', 'backward', 'back', 'moveToLayer'] as const).map((action, index) => ({
      label: arrangeLabels[index + 1], enabled: action === 'moveToLayer' ? canMoveToLayer : canArrange, action: () => props.onArrange(action),
    })) }, { label: transformLabels[locale].title, enabled: canEdit && selectedObjects.length > 0, children: (['move','rotate','reflect','scale','shear','individual','reset'] as const).map(action => ({label:transformLabels[locale][action],action:()=>props.onTransform(action)})) }, { label: t.path, enabled: canEdit, children: [
      { label: t.pathJoin, enabled: doc.selectedVectorObjects.length >= 1 && doc.selectedVectorObjects.length <= 2, shortcut: 'CmdOrCtrl+J', action: () => onPathEdit('join') },
      { label: t.pathAverage, enabled: selectedObjects.length > 0, action: () => onPathEdit('average') }, null,
      { label: t.pathOutline, enabled: selectedObjects.length > 0, action: () => onPathEdit('outline') },
      { label: t.pathOffset, enabled: selectedObjects.length > 0, action: () => onPathEdit('offset') },
      { label: t.pathReverse, enabled: selectedObjects.length > 0, action: () => onPathEdit('reverse') }, null,
      { label: t.pathSimplify, enabled: selectedObjects.length > 0, action: () => onPathEdit('simplify') },
      { label: t.pathSmooth, enabled: selectedObjects.length > 0, action: () => onPathEdit('smooth') },
      { label: t.pathAddAnchors, enabled: selectedObjects.length > 0, action: () => onPathEdit('addAnchors') },
      { label: t.pathRemoveAnchors, enabled: selectedObjects.length > 0, action: () => onPathEdit('removeAnchors') },
      { label: t.pathDivideBelow, enabled: selectedObjects.length === 2, action: () => onPathEdit('divideBelow') }, null,
      { label: t.pathSplitGrid, enabled: selectedObjects.length > 0, action: () => onPathEdit('splitGrid') }, null,
      { label: t.pathCleanUp, enabled: selectedObjects.length > 0, action: () => onPathEdit('cleanUp') },
    ]}, { label: clip[0], enabled: canEdit, children: [
      { label: clip[1], enabled: selectedObjects.length >= 2, action: () => props.onClipping('create') },
      { label: clip[2], enabled: selectedObjects.some(({ object }) => object.clippingMask), action: () => props.onClipping('release') },
      { label: clip[3], enabled: selectedObjects.some(({ object }) => object.clippingMask), action: () => props.onClipping('edit') },
    ]}, { label: { ja: '複合パス', en: 'Compound Path', 'zh-CN': '复合路径' }[locale], enabled: canEdit, children: [
      { label: clip[1], enabled: selectedObjects.length >= 2, action: () => props.onCompound(false) },
      { label: clip[2], enabled: selectedObjects.length === 1 && selectedObjects[0].object.kind === 'compound', action: () => props.onCompound(true) },
    ]}],
    [future(t.newLayer), future(t.duplicateLayer), future(t.deleteLayer), null,
      { label: t.group, enabled: canGroup, shortcut: 'CmdOrCtrl+G', action: () => onGroup('group') },
      { label: t.ungroup, enabled: canUngroup, shortcut: 'CmdOrCtrl+Shift+G', action: () => onGroup('ungroup') },
      { label: t.ungroupAll, enabled: canUngroup, action: () => onGroup('ungroupAll') }, null,
      { label: w.showLayer, enabled: canEdit, checked: doc.layerVisible, action: () => onEdit('toggleLayer') }],
    [{label: {ja:'アウトラインを作成',en:'Create Outlines','zh-CN':'创建轮廓'}[locale],enabled:canEdit && selectedObjects.some(({object,layer})=>object.kind==='text' && layer.visible && !layer.locked),action:props.onOutlineText}, null, { label: directionLabels[0], enabled: canEdit && selectedTexts.length > 0 && selectedTexts.every(text => text.editable), children: (['horizontal', 'vertical'] as const).map((mode, index) => ({ label: directionLabels[index + 1], checked: selectedTexts.length > 0 && selectedTexts.every(text => (text.text.writingMode ?? 'horizontal') === mode), action: () => props.onWritingMode(mode) })) }, null, future(t.font), future(t.fontSize), future(t.paragraph)],
    [{ label: t.selectAll, enabled: canEdit, shortcut: 'CmdOrCtrl+A', action: () => onEdit('selectAll') },
      { label: t.deselect, enabled: canEdit && !!doc.selection, shortcut: 'CmdOrCtrl+D', action: () => onEdit('deselect') },
      { label: t.invert, enabled: canEdit && !!doc.selection, shortcut: 'CmdOrCtrl+Shift+I', action: () => onEdit('invertSelection') }],
    [future(t.blur), future(t.sharpen), future(t.adjustments)],
    [{ label: {ja:'プレビュー表示',en:'Preview','zh-CN':'预览'}[locale], checked: !props.outlineDisplay, enabled: hasDocument, action: () => props.onOutlineDisplay(false) },
      { label: {ja:'アウトライン表示',en:'Outline','zh-CN':'轮廓'}[locale], checked: props.outlineDisplay, enabled: hasDocument, action: () => props.onOutlineDisplay(true) }, null,
      { label: common.zoomIn, enabled: canEdit && zoom < 4, action: () => onZoom(Math.min(4, zoom * 1.25)) },
      { label: common.zoomOut, enabled: canEdit && zoom > .25, action: () => onZoom(Math.max(.25, zoom / 1.25)) },
      { label: common.fit, enabled: canEdit, action: () => onZoom(1) }],
    [future(t.managePlugins), future(t.browsePlugins)],
    [{ label: { ja: '新規ウインドウ', en: 'New Window', 'zh-CN': '新建窗口' }[locale], shortcut: 'CmdOrCtrl+Shift+N', enabled: isTauri(), action: props.onNewWindow }, null, { label: w.panels, checked: panels, action: onPanels }, { label: t.resetWorkspace, action: onReset }],
    [{ label: 'LumaPaint 0.1.0' }, null, { label: t.guide }, { label: t.drawHint }, { label: t.saveHint }, { label: t.recoveryHint }],
  ];
  const [open, setOpen] = useState<number | null>(null);
  const [focused, setFocused] = useState(0);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const [submenuOpen, setSubmenuOpen] = useState<number | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const triggers = useRef<(HTMLButtonElement | null)[]>([]);
  const nativeBusy = useRef(false);
  const lastItem = useRef(false);
  const native = isTauri();
  const close = (focus = true) => {
    if (focus && open !== null) triggers.current[open]?.focus();
    setOpen(null); setSubmenuOpen(null);
  };
  const nativeEntry = (entry: Entry): NativeEntry => {
    if (entry === null) return { item: 'Separator' } as PredefinedMenuItemOptions;
    if (entry.children) return { text: entry.label, enabled: entry.enabled !== false, items: entry.children.map(nativeEntry) } as SubmenuOptions;
    const base = {
      text: entry.label + (entry.planned ? ` (${t.unavailable})` : ''), enabled: !!entry.action && entry.enabled !== false,
      ...(entry.shortcut ? { accelerator: entry.shortcut } : {}), action: () => entry.action?.(),
    };
    return entry.checked === undefined ? base as MenuItemOptions : { ...base, checked: entry.checked } as CheckMenuItemOptions;
  };
  const show = async (index: number, last = false) => {
    if (nativeBusy.current) return;
    setFocused(index); lastItem.current = last;
    if (!native) { setOpen(index); return; }
    nativeBusy.current = true; setOpen(index);
    let menu: Menu | undefined;
    try {
      menu = await Menu.new({ items: menus[index].map(nativeEntry) });
      const rect = triggers.current[index]!.getBoundingClientRect();
      const window = getCurrentWindow();
      const [size, scale] = await Promise.all([window.innerSize(), window.scaleFactor()]);
      // AppKit's content view can include the title-bar safe area excluded from the DOM.
      const topInset = Math.max(0, size.height / scale - globalThis.innerHeight);
      await menu.popup(new LogicalPosition(rect.left, rect.bottom + topInset), window);
    } catch (cause) { onError(String(cause)); }
    finally {
      nativeBusy.current = false; setOpen(null);
      if (menu) await menu.close().catch(cause => onError(String(cause)));
    }
  };
  useLayoutEffect(() => {
    if (open === null || native) return;
    const rect = triggers.current[open]!.getBoundingClientRect();
    const panel = popup.current!;
    setPosition({ left: Math.max(8, Math.min(rect.left, innerWidth - panel.offsetWidth - 8)), top: Math.min(rect.bottom + 2, Math.max(8, innerHeight - panel.offsetHeight - 8)) });
    const items = panel.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]');
    (lastItem.current ? items[items.length - 1] : items[0])?.focus();
  }, [open, native]);
  useEffect(() => {
    if (open === null || native) return;
    const dismiss = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!bar.current?.contains(target) && !popup.current?.contains(target)) setOpen(null);
    };
    const resize = () => setOpen(null);
    document.addEventListener('pointerdown', dismiss);
    window.addEventListener('resize', resize);
    return () => { document.removeEventListener('pointerdown', dismiss); window.removeEventListener('resize', resize); };
  }, [open, native]);
  const move = (index: number, direction: number, expand: boolean) => {
    const next = (index + direction + menus.length) % menus.length;
    setFocused(next); triggers.current[next]?.focus();
    if (expand) void show(next);
  };
  const triggerKey = (event: KeyboardEvent, index: number) => {
    if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { event.preventDefault(); move(index, event.key === 'ArrowRight' ? 1 : -1, open !== null); }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); void show(index, event.key === 'ArrowUp'); }
    if (event.key === 'Escape') { event.preventDefault(); close(); }
  };
  const popupKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    if (event.key === 'Tab') { close(); return; }
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); move(open!, event.key === 'ArrowRight' ? 1 : -1, true); }
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const items = [...popup.current!.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]')];
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
    }
  };
  return <>
    <div ref={bar} role="menubar" aria-label={t.bar} className="workspace-menubar">
      {t.names.map((name, index) => <button key={index} ref={node => { triggers.current[index] = node; }} id={`menu-trigger-${index}`} role="menuitem" aria-haspopup="menu" aria-expanded={open === index} aria-controls={!native && open === index ? 'workspace-dropdown' : undefined} tabIndex={focused === index ? 0 : -1}
        onFocus={() => setFocused(index)} onKeyDown={event => triggerKey(event, index)} onClick={() => open === index ? close() : void show(index)} onPointerEnter={() => { if (!native && open !== null && open !== index) void show(index); }}>{name}</button>)}
    </div>
    {!native && open !== null && createPortal(<div ref={popup} id="workspace-dropdown" role="menu" aria-labelledby={`menu-trigger-${open}`} className="workspace-dropdown" style={position} onKeyDown={popupKey}>
      {menus[open].map((entry, index) => entry === null ? <div role="separator" key={index} /> : entry.children ? <div className="menu-nested" key={index} onPointerLeave={() => setSubmenuOpen(null)}>
        <button role="menuitem" aria-haspopup="menu" aria-expanded={submenuOpen === index} tabIndex={-1} onPointerEnter={() => setSubmenuOpen(index)} onClick={() => setSubmenuOpen(index)} onKeyDown={event => {
          if (event.key === 'ArrowRight') { event.preventDefault(); event.stopPropagation(); setSubmenuOpen(index); }
        }}><span className="menu-check" /><span>{entry.label}</span><span className="menu-chevron" aria-hidden="true">›</span></button>
        {submenuOpen === index && <div className="workspace-submenu" role="menu" aria-label={entry.label} onKeyDown={event => {
          if (event.key === 'ArrowLeft') { event.preventDefault(); event.stopPropagation(); setSubmenuOpen(null); }
        }}>{entry.children.map((child, childIndex) => child === null ? <div role="separator" key={childIndex} /> : <button key={childIndex} role={child.checked === undefined ? 'menuitem' : 'menuitemradio'} aria-checked={child.checked} aria-disabled={!child.action || child.enabled === false} tabIndex={-1} onClick={() => {
          if (!child.action || child.enabled === false) return; close(); child.action();
        }}><span className="menu-check" aria-hidden="true">{child.checked ? '✓' : ''}</span><span>{child.label}</span></button>)}</div>}
      </div> : <button key={index} role={entry.checked === undefined ? 'menuitem' : 'menuitemcheckbox'} aria-checked={entry.checked} aria-disabled={!entry.action || entry.enabled === false} tabIndex={-1}
          onClick={() => { if (!entry.action || entry.enabled === false) return; close(); entry.action(); }}>
          <span className="menu-check" aria-hidden="true">{entry.checked ? '✓' : ''}</span><span>{entry.label}</span>
          {entry.planned ? <small>{t.unavailable}</small> : entry.shortcut && <kbd>{entry.shortcut.replace('CmdOrCtrl+', /Mac/.test(navigator.platform) ? '⌘' : 'Ctrl+').replace('Shift+', '⇧')}</kbd>}
        </button>)}
    </div>, document.body)}
  </>;
}
