import {useShortcutCommands,useShortcuts,binding,accelerator,keyFromAccelerator,displayKey,type Command} from '../shortcuts';
import {shortcutLabels} from '../shortcut-i18n';
import {vectorSelectionLabels,sameCriteria,objectCriteria} from '../vector-selection-i18n';
import type {VectorSelectionRequest} from '../bridge';
import {colorModes as documentModes, modeLabels} from '../document-color-modes';
import {layerGroupLabels} from './layer-group-labels';
import { MIN_ZOOM, MAX_ZOOM, stepZoom } from '../zoom';
import { transformLabels } from './TransformDialog';
import type { ArrangeAction, TransformAction } from '../bridge';
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { isTauri } from '@tauri-apps/api/core';
import { createNativeMenu, type NativeMenu } from '../native-menu';
import type { CheckMenuItemOptions, MenuItemOptions, PredefinedMenuItemOptions, SubmenuOptions } from '@tauri-apps/api/menu';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Locale } from '../i18n';
import { messages } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { menuMessages } from '../menu-i18n';
import type { BitDepth, ColorMode, DocumentEditAction, DocumentSnapshot, PathEditAction } from '../bridge';

type EntryItem = { id?: string; label: string; action?: () => void; enabled?: boolean; checked?: boolean; shortcut?: string; planned?: boolean; children?: Entry[] };
type Entry = EntryItem | null;
type NativeEntry = MenuItemOptions | CheckMenuItemOptions | SubmenuOptions | PredefinedMenuItemOptions;
// New modes are registered here; both native and browser submenus derive from this list.

const bitDepths: { value: BitDepth; label: string }[] = [{ value: 8, label: '8 bits' }, { value: 16, label: '16 bits' }, { value: 32, label: '32 bits' }];
type Props = {
  onShortcuts:()=>void;
  onVectorSelection:(request:VectorSelectionRequest)=>void;
  onSavedSelections:(mode:'save'|'edit')=>void;
  onLayerGroupEdit:(edit:import('../bridge').LayerGroupEdit)=>void;
  onGuides:(action:'visibility'|'lock'|'make'|'release'|'clear'|'snap'|'selectAll'|'invertSelection'|'deselect'|'thirds'|'quarters'|'margins'|'saveLayout'|'loadLayout')=>void;
  outlineDisplay: boolean; onOutlineDisplay: (value: boolean) => void;
  locale: Locale; document: DocumentSnapshot; canFile: boolean; hasDocument: boolean; canEdit: boolean; canHistory?: boolean;
  zoom: number; panels: boolean; onFile: (action: 'open' | 'save' | 'saveAs' | 'export') => void;
  onNewWindow: () => void; onNew: () => void; onCloseDocument: () => void;
  onPlace:()=>void;
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
  const canHistory = props.canHistory ?? canEdit;
  const guides={ja:['ガイド','ガイドを隠す','ガイドを表示','ガイドのロックを解除','ガイドをロック','ガイドを作成','ガイドを解除','ガイドを消去'],en:['Guides','Hide Guides','Show Guides','Unlock Guides','Lock Guides','Make Guides','Release Guides','Clear Guides'],'zh-CN':['参考线','隐藏参考线','显示参考线','解锁参考线','锁定参考线','建立参考线','释放参考线','清除参考线']}[locale];
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
  const vs=vectorSelectionLabels[locale];
  const vectorReady=canEdit&&!doc.activeSavedPath;
  const hasVectors=doc.layers.some(l=>l.kind==='vector'&&l.visible&&!l.locked&&l.objects.some(o=>o.visible&&!o.locked));
  const vectorAction=(action:string,criterion?:string,name?:string)=>()=>props.onVectorSelection({action,criterion,name});
  const sameEntry=(index:number):Entry=>({id:`vector.same.${sameCriteria[index]}`,label:vs.sameNames[index],enabled:index<9||selectedTexts.length>0,action:vectorAction('same',sameCriteria[index])});
  const sameEntries:Entry[]=[sameEntry(0),sameEntry(1),future(vs.unavailable[3]),...sameCriteria.slice(2,8).map((_,i)=>sameEntry(i+2)),future(vs.unavailable[4]),sameEntry(8),future(vs.unavailable[5]),future(vs.unavailable[6]),null,{id:'command.text',label: {ja:'テキスト',en:'Text','zh-CN':'文本'}[locale],enabled:false},...sameCriteria.slice(9).map((_,i)=>sameEntry(i+9))];
  const savedKey=(name:string)=>{let hash=0xcbf29ce484222325n;for(const byte of new TextEncoder().encode(name)){hash=BigInt.asUintN(64,(hash^BigInt(byte))*0x100000001b3n);}return hash.toString(16);};
  const vectorMenus:Entry[]=[
    { id: 'vector.selectAll', label: t.selectAll,enabled:vectorReady&&hasVectors,shortcut:'',action:vectorAction('all')},
    {id:'vector.artboard',label:vs.artboard,enabled:vectorReady&&hasVectors,shortcut:'CmdOrCtrl+Alt+A',action:vectorAction('artboard')},
    { id: 'vector.deselect', label: t.deselect,enabled:vectorReady&&selectedObjects.length>0,shortcut:'',action:vectorAction('deselect')},
    {id:'vector.reselect',label:vs.reselect,enabled:vectorReady&&doc.canReselectVectors,shortcut:'CmdOrCtrl+6',action:vectorAction('reselect')},
    { id: 'vector.invert', label: t.invert,enabled:vectorReady&&hasVectors,action:vectorAction('invert')},null,
    {id:'vector.above',label:vs.above,enabled:vectorReady&&selectedObjects.length>0,shortcut:'CmdOrCtrl+Alt+]',action:vectorAction('above')},
    {id:'vector.below',label:vs.below,enabled:vectorReady&&selectedObjects.length>0,shortcut:'CmdOrCtrl+Alt+[',action:vectorAction('below')},null,
    {id:'vector.same',label:vs.same,enabled:vectorReady&&selectedObjects.length>0,children:sameEntries},
    {id:'vector.object',label:vs.object,enabled:vectorReady&&hasVectors,children:[{id:'vector.object.sameLayers',label:vs.objectNames[0],enabled:selectedObjects.length>0,action:vectorAction('object','sameLayers')},future(vs.unavailable[0]),null,future(vs.unavailable[1]),future(vs.unavailable[2]),...objectCriteria.slice(1,3).map((criterion,i)=>({id:`vector.object.${criterion}`,label:vs.objectNames[i+1],action:vectorAction('object',criterion)})),null,...objectCriteria.slice(3).map((criterion,i)=>({id:`vector.object.${criterion}`,label:vs.objectNames[i+3],action:vectorAction('object',criterion)}))]},
    future(vs.unavailable[7]),null,
    {id:'vector.save',label:vs.save,enabled:vectorReady&&selectedObjects.length>0,action:()=>props.onSavedSelections('save')},
    {id:'vector.edit',label:vs.edit,enabled:vectorReady&&doc.savedVectorSelections.length>0,action:()=>props.onSavedSelections('edit')},
    {id:'vector.update',label:vs.update,enabled:vectorReady&&selectedObjects.length>0&&doc.savedVectorSelections.length>0,children:doc.savedVectorSelections.map(item=>({id:`savedSelection.update.${savedKey(item.name)}`,label:item.name,action:vectorAction('update',undefined,item.name)}))},
    ...(doc.savedVectorSelections.length?[null,...doc.savedVectorSelections.map(item=>({id:`savedSelection.load.${savedKey(item.name)}`,label:item.name,enabled:vectorReady,action:vectorAction('load',undefined,item.name)}))]:[]),
  ];
  const names=[...t.names.slice(0,6),vs.title,...t.names.slice(6)];
  const menus: Entry[][] = [
    [{ id: 'menu.new', label: t.new, enabled: canFile, shortcut: 'CmdOrCtrl+N', action: onNew }, { id: 'menu.open', label: w.open + '…', enabled: canFile, shortcut: 'CmdOrCtrl+O', action: () => onFile('open') },
      { id: 'menu.closeDocument', label: t.closeDocument, enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+W', action: onCloseDocument },
      {id:'command.place',label: {ja:'配置…',en:'Place…','zh-CN':'置入…'}[locale],enabled:canFile&&hasDocument&&canEdit,shortcut:'CmdOrCtrl+D',action:props.onPlace},
      { id: 'menu.importSvg', label: t.importSvg, enabled: canFile && hasDocument, action: onImportSvg }, null,
      { id: 'menu.save', label: w.save, enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+S', action: () => onFile('save') },
      { id: 'menu.saveAs', label: w.saveAs + '…', enabled: canFile && hasDocument, shortcut: 'CmdOrCtrl+Shift+S', action: () => onFile('saveAs') }, null, {id:'command.import',label: {ja:'読み込み…',en:'Import…', 'zh-CN': '导入…' }[locale], enabled: canFile && hasDocument && canEdit, action: props.onImportImage }, { id: 'menu.export', label: t.export, enabled: canFile && hasDocument, action: () => onFile('export') }],
    [{ id: 'menu.undo', label: w.undo, enabled: canHistory && doc.canUndo, shortcut: 'CmdOrCtrl+Z', action: () => onEdit('undo') },
      { id: 'menu.redo', label: w.redo, enabled: canHistory && doc.canRedo, shortcut: 'CmdOrCtrl+Shift+Z', action: () => onEdit('redo') }, null,
      { id: 'menu.colorSettings', label: t.colorSettings, enabled: hasDocument, action: onColorSettings }, null, { id: 'menu.cut', label: t.cut, enabled: canEdit, shortcut: 'CmdOrCtrl+X', action: () => onEdit('cut') }, { id: 'menu.copy', label: t.copy, enabled: canEdit, shortcut: 'CmdOrCtrl+C', action: () => onEdit('copy') }, { id: 'menu.paste', label: t.paste, enabled: canEdit, shortcut: 'CmdOrCtrl+V', action: () => onEdit('paste') }, null,
      { id: 'menu.clearLayer', label: t.clearLayer, enabled: canEdit && !doc.layers.find(layer => layer.id === doc.layerId)?.locked, action: () => onEdit('clearLayer') }],
    [{ id: 'menu.colorMode', label: t.colorMode, children: documentModes.map(mode => ({ id:`menu.colorMode.${mode}`,label: modeLabels[locale][mode], checked: doc.colorMode === mode, enabled: canEdit, action: () => onColorMode(mode) })) },
      { id: 'menu.bitDepth', label: t.bitDepth, children: bitDepths.map(depth => ({ id:`menu.bitDepth.${depth.value}`,label: depth.label, checked: doc.bitDepth === depth.value, enabled: canEdit, action: () => onBitDepth(depth.value) })) }, null,
      future(t.imageSize), future(t.canvasSize), future(t.rotate)],
    [{ id: 'menu.lock', label: t.lock, enabled: canEdit && !doc.activeSavedPath, children: [
      { id: 'menu.lockSelection', label: t.lockSelection, enabled: doc.selectedVectorObjects.length > 0 || !!doc.selection || (destination?.kind === 'svg' && destination.visible && !destination.locked), shortcut: 'CmdOrCtrl+2', action: () => onEdit('lockSelection') },
      { id: 'menu.lockArtworkAbove', label: t.lockArtworkAbove, enabled: doc.selectedVectorObjects.length > 0, action: () => onEdit('lockArtworkAbove') },
      { id: 'menu.lockOtherLayers', label: t.lockOtherLayers, enabled: doc.layers.length > 1, action: () => onEdit('lockOtherLayers') },
    ]}, { id: 'menu.unlockAllObjects', label: t.unlockAllObjects, enabled: canEdit && !doc.activeSavedPath && doc.hasLockedObjects, shortcut: 'CmdOrCtrl+Alt+2', action: () => onEdit('unlockAllObjects') }, null,
    { id: 'menu.hide', label: t.hide, enabled: canEdit && !doc.activeSavedPath, children: [
      { id: 'menu.hideSelection', label: t.hideSelection, enabled: doc.selectedVectorObjects.length > 0 || !!doc.selection || (destination?.kind === 'svg' && destination.visible && !destination.locked), shortcut: 'CmdOrCtrl+3', action: () => onEdit('hideSelection') },
      { id: 'menu.hideArtworkAbove', label: t.hideArtworkAbove, enabled: doc.selectedVectorObjects.length > 0, action: () => onEdit('hideArtworkAbove') },
      { id: 'menu.hideOtherLayers', label: t.hideOtherLayers, enabled: doc.layers.length > 1, action: () => onEdit('hideOtherLayers') },
    ]}, { id: 'menu.showAllObjects', label: t.showAllObjects, enabled: canEdit && !doc.activeSavedPath && doc.hasHiddenObjects, shortcut: 'CmdOrCtrl+Alt+3', action: () => onEdit('showAllObjects') }, null,
    {id:'command.create-trim-marks',label: {ja:'トリムマークを作成',en:'Create Trim Marks','zh-CN':'创建裁切标记'}[locale],enabled:canArrange&&!doc.activeSavedPath&&selectedObjects.length===1&&selectedObjects[0].object.kind==='rectangle'&&!selectedObjects[0].object.locked,action:()=>onEdit('createTrimMarks')}, null,
    { label: arrangeLabels[0], enabled: canArrange, children: (['front', 'forward', 'backward', 'back', 'moveToLayer'] as const).map((action, index) => ({
      id:`arrange.${action}`,shortcut:({front:'CmdOrCtrl+Shift+]',forward:'CmdOrCtrl+]',backward:'CmdOrCtrl+[',back:'CmdOrCtrl+Shift+[',moveToLayer:''})[action], label: arrangeLabels[index + 1], enabled: action === 'moveToLayer' ? canMoveToLayer : canArrange, action: () => props.onArrange(action),
    })) }, { label: transformLabels[locale].title, enabled: canEdit && selectedObjects.length > 0, children: (['move','rotate','reflect','scale','shear','individual','reset'] as const).map(action => ({id:`transform.${action}`,label:transformLabels[locale][action],action:()=>props.onTransform(action)})) }, { id: 'menu.path', label: t.path, enabled: canEdit, children: [
      { id: 'menu.pathJoin', label: t.pathJoin, enabled: doc.selectedVectorObjects.length >= 1 && doc.selectedVectorObjects.length <= 2, shortcut: 'CmdOrCtrl+J', action: () => onPathEdit('join') },
      { id: 'menu.pathAverage', label: t.pathAverage, enabled: selectedObjects.length > 0, action: () => onPathEdit('average') }, null,
      { id: 'menu.pathOutline', label: t.pathOutline, enabled: selectedObjects.length > 0, action: () => onPathEdit('outline') },
      { id: 'menu.pathOffset', label: t.pathOffset, enabled: selectedObjects.length > 0, action: () => onPathEdit('offset') },
      { id: 'menu.pathReverse', label: t.pathReverse, enabled: selectedObjects.length > 0, action: () => onPathEdit('reverse') }, null,
      { id: 'menu.pathSimplify', label: t.pathSimplify, enabled: selectedObjects.length > 0, action: () => onPathEdit('simplify') },
      { id: 'menu.pathSmooth', label: t.pathSmooth, enabled: selectedObjects.length > 0, action: () => onPathEdit('smooth') },
      { id: 'menu.pathAddAnchors', label: t.pathAddAnchors, enabled: selectedObjects.length > 0, action: () => onPathEdit('addAnchors') },
      { id: 'menu.pathRemoveAnchors', label: t.pathRemoveAnchors, enabled: selectedObjects.length > 0, action: () => onPathEdit('removeAnchors') },
      { id: 'menu.pathDivideBelow', label: t.pathDivideBelow, enabled: selectedObjects.length === 2, action: () => onPathEdit('divideBelow') }, null,
      { id: 'menu.pathSplitGrid', label: t.pathSplitGrid, enabled: selectedObjects.length > 0, action: () => onPathEdit('splitGrid') }, null,
      { id: 'menu.pathCleanUp', label: t.pathCleanUp, enabled: selectedObjects.length > 0, action: () => onPathEdit('cleanUp') },
    ]}, { label: clip[0], enabled: canEdit, children: [
      {id:'clipping.create',shortcut:'CmdOrCtrl+7', label: clip[1], enabled: selectedObjects.length >= 2, action: () => props.onClipping('create') },
      {id:'clipping.release',shortcut:'CmdOrCtrl+Alt+7', label: clip[2], enabled: selectedObjects.some(({ object }) => object.clippingMask), action: () => props.onClipping('release') },
      {id:'clipping.edit', label: clip[3], enabled: selectedObjects.some(({ object }) => object.clippingMask), action: () => props.onClipping('edit') },
    ]}, {id:'command.compound-path',label: {ja:'複合パス',en:'Compound Path', 'zh-CN': '复合路径' }[locale], enabled: canEdit, children: [
      {id:'compound.create',shortcut:'CmdOrCtrl+8', label: clip[1], enabled: selectedObjects.length >= 2, action: () => props.onCompound(false) },
      {id:'compound.release',shortcut:'CmdOrCtrl+Alt+8', label: clip[2], enabled: selectedObjects.length === 1 && selectedObjects[0].object.kind === 'compound', action: () => props.onCompound(true) },
    ]}],
    [future(t.newLayer), future(t.duplicateLayer), future(t.deleteLayer), null,
      {id:'layerGroup.create',label:layerGroupLabels[locale].create,enabled:canEdit,action:()=>props.onLayerGroupEdit({action:'createEmpty',name:layerGroupLabels[locale].name})},
      {id:'layerGroup.group',label:layerGroupLabels[locale].group,enabled:canEdit&&(doc.layerGroups.selected.some(id=>id!=='layer-1')),action:()=>props.onLayerGroupEdit({action:'create',ids:doc.layerGroups.selected.filter(id=>id!=='layer-1'),name:layerGroupLabels[locale].name})},
      {id:'layerGroup.ungroup',label:layerGroupLabels[locale].ungroup,enabled:canEdit&&doc.layerGroups.selected.some(id=>doc.layerGroups.groups.some(g=>g.id===id)),action:()=>{const id=doc.layerGroups.selected.find(id=>doc.layerGroups.groups.some(g=>g.id===id));if(id)props.onLayerGroupEdit({action:'ungroup',id});}},null,
      { id: 'menu.group', label: t.group, enabled: canGroup, shortcut: 'CmdOrCtrl+G', action: () => onGroup('group') },
      { id: 'menu.ungroup', label: t.ungroup, enabled: canUngroup, shortcut: 'CmdOrCtrl+Shift+G', action: () => onGroup('ungroup') },
      { id: 'menu.ungroupAll', label: t.ungroupAll, enabled: canUngroup, action: () => onGroup('ungroupAll') }, null,
      { id: 'menu.showLayer', label: w.showLayer, enabled: canEdit, checked: doc.layerVisible, action: () => onEdit('toggleLayer') }],
    [{id:'command.create-outlines',shortcut:'CmdOrCtrl+Shift+O',label: {ja:'アウトラインを作成',en:'Create Outlines','zh-CN':'创建轮廓'}[locale],enabled:canEdit && selectedObjects.some(({object,layer})=>object.kind==='text' && layer.visible && !layer.locked),action:props.onOutlineText}, null, { label: directionLabels[0], enabled: canEdit && selectedTexts.length > 0 && selectedTexts.every(text => text.editable), children: (['horizontal', 'vertical'] as const).map((mode, index) => ({ id:`text.direction.${mode}`,label: directionLabels[index + 1], checked: selectedTexts.length > 0 && selectedTexts.every(text => (text.text.writingMode ?? 'horizontal') === mode), action: () => props.onWritingMode(mode) })) }, null, future(t.font), future(t.fontSize), future(t.paragraph)],
    [{ id: 'menu.selectAll', label: t.selectAll, enabled: canEdit, shortcut: 'CmdOrCtrl+A', action: () => onEdit('selectAll') },
      { id: 'menu.deselect', label: t.deselect, enabled: canEdit && !!doc.selection, shortcut: 'CmdOrCtrl+Shift+A', action: () => onEdit('deselect') },
      { id: 'menu.invert', label: t.invert, enabled: canEdit && !!doc.selection, shortcut: 'CmdOrCtrl+Shift+I', action: () => onEdit('invertSelection') }],
    [future(t.blur), future(t.sharpen), future(t.adjustments)],
    [{id:'command.preview',label: {ja:'プレビュー表示',en:'Preview','zh-CN':'预览'}[locale], checked: !props.outlineDisplay, enabled: hasDocument, action: () => props.onOutlineDisplay(false) },
      {id:'command.outline',label: {ja:'アウトライン表示',en:'Outline','zh-CN':'轮廓'}[locale], checked: props.outlineDisplay, enabled: hasDocument, action: () => props.onOutlineDisplay(true) }, null,
      {id:'view.zoomIn',shortcut:'CmdOrCtrl+=', label: common.zoomIn, enabled: canEdit && zoom < MAX_ZOOM - 0.0001, action: () => onZoom(stepZoom(zoom, 1)) },
      {id:'view.zoomOut',shortcut:'CmdOrCtrl+-', label: common.zoomOut, enabled: canEdit && zoom > MIN_ZOOM, action: () => onZoom(stepZoom(zoom, -1)) },
      {id:'view.fit',shortcut:'CmdOrCtrl+0', label: common.fit, enabled: canEdit, action: () => onZoom(0) }, null,
      {label:guides[0],enabled:canEdit,children:[
        {id:'guides.visibility',label:guides[doc.guides.visible?1:2],shortcut:'CmdOrCtrl+;',action:()=>props.onGuides('visibility')},
        {id:'guides.lock',label:guides[doc.guides.locked?3:4],shortcut:'CmdOrCtrl+Alt+;',action:()=>props.onGuides('lock')},null,
        {id:'command.select-all-guides',label: {ja:'すべてのガイドを選択',en:'Select All Guides','zh-CN':'选择所有参考线'}[locale],enabled:doc.guides.visible&&!doc.guides.locked&&doc.guides.items.length>0,action:()=>props.onGuides('selectAll')},
        {id:'command.deselect-guides',label: {ja:'ガイドの選択を解除',en:'Deselect Guides','zh-CN':'取消选择参考线'}[locale],enabled:doc.guides.selected.length>0,action:()=>props.onGuides('deselect')},
        {id:'command.invert-guide-selection',label: {ja:'ガイドの選択を反転',en:'Invert Guide Selection','zh-CN':'反选参考线'}[locale],enabled:doc.guides.visible&&!doc.guides.locked&&doc.guides.items.length>0,action:()=>props.onGuides('invertSelection')},null,
        {id:'command.guide-layout',label: {ja:'ガイド配置',en:'Guide Layout','zh-CN':'参考线布局'}[locale],children:[
          {id:'command.thirds',label: {ja:'3等分',en:'Thirds','zh-CN':'三等分'}[locale],action:()=>props.onGuides('thirds')},
          {id:'command.quarters',label: {ja:'4等分',en:'Quarters','zh-CN':'四等分'}[locale],action:()=>props.onGuides('quarters')},
          {id:'command.10-margins',label: {ja:'10%の余白',en:'10% Margins','zh-CN':'10% 边距'}[locale],action:()=>props.onGuides('margins')},null,
          {id:'command.save-layout',label: {ja:'配置を保存…',en:'Save Layout…','zh-CN':'保存布局…'}[locale],enabled:doc.guides.items.length>0,action:()=>props.onGuides('saveLayout')},
          {id:'command.load-layout',label: {ja:'配置を読み込み…',en:'Load Layout…','zh-CN':'加载布局…'}[locale],action:()=>props.onGuides('loadLayout')},
        ]},null,
        {id:'guides.make',label:guides[5],enabled:selectedObjects.some(({object,layer})=>layer.kind==='vector'&&!layer.locked&&object.kind!=='text'),shortcut:'CmdOrCtrl+5',action:()=>props.onGuides('make')},
        {id:'guides.release',label:guides[6],enabled:!doc.guides.locked&&doc.guides.selected.length>0,shortcut:'CmdOrCtrl+Alt+5',action:()=>props.onGuides('release')},
        {id:'guides.clear',label:guides[7],enabled:doc.guides.items.length>0,action:()=>props.onGuides('clear')},null,
        {id:'command.snap-to-guides',label: {ja:'ガイドにスナップ',en:'Snap to Guides','zh-CN':'对齐参考线'}[locale],checked:doc.guides.snap,action:()=>props.onGuides('snap')},
      ]}],
    [future(t.managePlugins), future(t.browsePlugins)],
    [{id:'command.new-window',label: {ja:'新規ウインドウ',en:'New Window', 'zh-CN': '新建窗口' }[locale], shortcut: 'CmdOrCtrl+Shift+N', enabled: isTauri(), action: props.onNewWindow }, null, { id: 'menu.panels', label: w.panels, checked: panels, action: onPanels }, { id: 'menu.resetWorkspace', label: t.resetWorkspace, action: onReset }],
    [{ label: 'LumaPaint 0.1.0' }, null, { id: 'menu.guide', label: t.guide }, { id: 'menu.drawHint', label: t.drawHint }, { id: 'menu.saveHint', label: t.saveHint }, { id: 'menu.recoveryHint', label: t.recoveryHint }],
  ];
  menus.splice(6,0,vectorMenus);
  menus[1].push(null,{id:'shortcuts.edit',label:shortcutLabels[locale].title+'…',shortcut:'CmdOrCtrl+Alt+Shift+K',action:props.onShortcuts});
  const shortcutState=useShortcuts();
  const commands:Command[]=[];
  const collect=(entries:Entry[],category:string,path:string,parentEnabled=true)=>entries.forEach((entry,index)=>{
    if(!entry)return;const id=entry.id??`${path}.${index}`;
    if(entry.children)collect(entry.children,`${category} › ${entry.label}`,id,parentEnabled&&entry.enabled!==false);
    else if(entry.action&&!entry.planned){
      const command={id,label:entry.label,category,defaultKey:entry.shortcut?keyFromAccelerator(entry.shortcut):'',enabled:parentEnabled&&entry.enabled!==false,action:entry.action};
      commands.push(command);entry.shortcut=accelerator(binding(shortcutState.settings,command));
    }
  });
  menus.forEach((entries,index)=>collect(entries,names[index],`menu.${index}`));
  useShortcutCommands('menus',commands);

  const [open, setOpen] = useState<number | null>(null);
  const [focused, setFocused] = useState(0);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const [submenuOpen, setSubmenuOpen] = useState<number | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const triggers = useRef<(HTMLButtonElement | null)[]>([]);
  const nativeBusy = useRef(false);
  const nativeMenu = useRef<NativeMenu | null>(null);
  useEffect(() => () => {
    const menu = nativeMenu.current;
    nativeMenu.current = null;
    void menu?.close().catch(() => {});
  }, []);
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
    let menu: NativeMenu | undefined;
    try {
      const previous = nativeMenu.current;
      nativeMenu.current = null;
      await previous?.close();
      menu = await createNativeMenu({ items: menus[index].map(nativeEntry) });
      nativeMenu.current = menu;
      const rect = triggers.current[index]!.getBoundingClientRect();
      const window = getCurrentWindow();
      const [size, scale] = await Promise.all([window.innerSize(), window.scaleFactor()]);
      // AppKit's content view can include the title-bar safe area excluded from the DOM.
      const topInset = Math.max(0, size.height / scale - globalThis.innerHeight);
      await menu.popup(new LogicalPosition(rect.left, rect.bottom + topInset), window);
    } catch (cause) {
      if (nativeMenu.current === menu) nativeMenu.current = null;
      await menu?.close().catch(() => {});
      onError(String(cause));
    }
    finally {
      nativeBusy.current = false; setOpen(null);
      // popup() resolves when shown. Keep the menu alive for its later action event;
      // release it when replaced or when this component unmounts.
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
  useLayoutEffect(()=>{
    if(native||submenuOpen===null)return;
    const panel=popup.current?.querySelector<HTMLElement>('.workspace-submenu');
    if(!panel)return;
    panel.style.maxHeight=`${innerHeight-16}px`;panel.style.overflowY='auto';
    const parent=panel.parentElement!.getBoundingClientRect();
    const top=Math.max(8,Math.min(parent.top-5,innerHeight-panel.offsetHeight-8));
    panel.style.position='fixed';
    panel.style.top=`${top}px`;
    panel.style.left=`${Math.max(8,parent.right+panel.offsetWidth>innerWidth-8?parent.left-panel.offsetWidth+2:parent.right-2)}px`;
  },[submenuOpen,open,native]);
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
      {names.map((name, index) => <button key={index} ref={node => { triggers.current[index] = node; }} id={`menu-trigger-${index}`} role="menuitem" aria-haspopup="menu" aria-expanded={open === index} aria-controls={!native && open === index ? 'workspace-dropdown' : undefined} tabIndex={focused === index ? 0 : -1}
        onFocus={() => setFocused(index)} onKeyDown={event => triggerKey(event, index)} onClick={() => open === index ? close() : void show(index)} onPointerEnter={() => { if (!native && open !== null && open !== index) void show(index); }}>{name}</button>)}
    </div>
    {!native && open !== null && createPortal(<div ref={popup} id="workspace-dropdown" role="menu" aria-labelledby={`menu-trigger-${open}`} className="workspace-dropdown" style={{...position,maxHeight:'calc(100vh - 16px)',overflowY:'auto'}} onScroll={()=>setSubmenuOpen(null)} onKeyDown={popupKey}>
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
          {entry.planned ? <small>{t.unavailable}</small> : entry.shortcut && <kbd>{displayKey(keyFromAccelerator(entry.shortcut))}</kbd>}
        </button>)}
    </div>, document.body)}
  </>;
}
