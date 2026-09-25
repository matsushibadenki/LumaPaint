import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

export type CanvasTool = 'brush' | 'eraser' | 'rectangle' | 'ellipse' | 'vectorSelect' | 'vectorDirectSelect' | 'vectorPen' | 'vectorPencil' | 'vectorAnchorAdd' | 'vectorAnchorDelete' | 'vectorAnchorConvert' | 'vectorRectangle' | 'vectorEllipse' | 'text' | 'textFrame' | 'zoomIn' | 'zoomOut' | 'hand';
export type DocumentEditAction = 'undo' | 'redo' | 'toggleLayer' | 'selectAll' | 'deselect' | 'invertSelection' | 'deleteSelectedObjects' | 'clearLayer' | 'copy' | 'cut' | 'paste';
export interface Selection { regions: { shape: 'rectangle' | 'ellipse'; bounds: [number, number, number, number]; operation: 'replace' | 'add' | 'subtract' | 'invert' }[] }
export interface Brush { size: number; hardness: number; color: [number, number, number] }
export interface DocumentSnapshot {
  selection: Selection | null;
  name: string;
  width: number; height: number; layerId: string; layerVisible: boolean;
  unit: DocumentUnit; resolution: number; artboards: boolean; canvasColor: CanvasColor; pixelAspectRatio: number;
  colorMode: ColorMode;
  colorProfile: ColorProfile;
  bitDepth: BitDepth;
  strokeCount: number; layers: LayerSnapshot[]; canUndo: boolean; canRedo: boolean; revision: number; dirty: boolean; fileName: string | null;
  selectedVectorObjects: string[];
  textObjects: TextObjectSnapshot[];
}
export interface LayerObjectSnapshot { strokeWidth: number; id: string; name: string; groupPath: string[]; kind: 'path' | 'bezier' | 'compound' | 'rectangle' | 'ellipse' | 'text'; visible: boolean }
export interface LayerSnapshot { objects: LayerObjectSnapshot[]; id: string; name: string; kind: 'paint' | 'svg' | 'vector'; visible: boolean; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number; deletable: boolean; strokeCount: number }
export interface TextStyle { fontFamily: string; fontSize: number; bold: boolean; italic: boolean; tracking: number; baselineShift: number; underline: boolean; strikethrough: boolean; color: [number, number, number] }
export interface TextRun { start: number; end: number; style: TextStyle }
export interface TextGlyphCluster { start: number; end: number; x: number }
export interface TextSelection { start: number; length: number; characters: number; style: TextStyle; mixed: (keyof TextStyle)[] }
export interface VectorText {
  runs?: TextRun[]; softBreaks?: number[]; lineBaselines?: number[]; lineWidths?: number[]; lineOrigins?: number[]; styleSegmentOrigins?: number[][]; characterOrigins?: number[][]; glyphClusters?: TextGlyphCluster[][]; layoutBounds?: [number, number, number, number];
  content: string; fontFamily: string; fontSize: number; lineHeight: number; bold: boolean;
  italic: boolean; tracking: number; scaleX: number; scaleY: number; baselineShift: number;
  rotation: number; underline: boolean; strikethrough: boolean; alignment: 'left' | 'center' | 'right' | 'justify';
  boxWidth: number; boxHeight?: number | null; indentLeft: number; indentRight: number; indentFirst: number; spaceBefore: number; spaceAfter: number;
  listStyle: 'none' | 'bullets' | 'numbers'; kinsoku: 'none' | 'standard' | 'strict'; mojikumi: 'none' | 'japanese'; hyphenation: boolean;
}
export const defaultVectorText: VectorText = {
  content: 'Text', runs: [], softBreaks: [], fontFamily: 'sans-serif', fontSize: 48, lineHeight: 1.4, bold: false,
  italic: false, tracking: 0, scaleX: 1, scaleY: 1, baselineShift: 0, rotation: 0,
  underline: false, strikethrough: false, alignment: 'left', boxWidth: 480,
  indentLeft: 0, indentRight: 0, indentFirst: 0, spaceBefore: 0, spaceAfter: 0,
  listStyle: 'none', kinsoku: 'none', mojikumi: 'none', hyphenation: false,
};
export function textFonts(): Promise<string[]> { return isTauri() ? invoke('text_fonts') : Promise.resolve([]); }
function textCommand<T>(command: string, args: Record<string, unknown>): Promise<T> {
  const result = canvasQueue.then(() => invoke<T>(command, args));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
function textPayload(settings: TextSettings) { return { id: settings.id, text: settings.text, position: settings.position, color: settings.color }; }
export function beginTextEdit(settings: TextSettings) { return textCommand<void>('begin_text_edit', { settings: textPayload(settings) }); }
export function updateTextEdit(settings: TextSettings) { return textCommand<DocumentSnapshot>('update_text_edit', { settings: textPayload(settings), patch: settings.stylePatch ?? null }); }
export function setTextEditColor(id: string | null, color: Brush['color']) { return textCommand<void>('set_text_edit_color', { id, color }); }
export function finishTextEdit(commit: boolean) { return textCommand<DocumentSnapshot>('finish_text_edit', { commit }); }
export async function subscribeTextSession(onChange: (settings: TextSettings | null) => void) {
  if (!isTauri()) return () => {};
  return listen<TextSettings | null>('canvas-text-session', event => onChange(event.payload));
}

export interface TextSettings { id: string | null; text: VectorText; position: [number, number]; color: [number, number, number]; selection?: TextSelection; stylePatch?: Partial<TextStyle> }
export interface TextObjectSnapshot extends Omit<TextSettings, 'id'> { id: string; layerId: string; editable: boolean }
export function setTextObject(settings: TextSettings): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_text_object', { settings: { id: settings.id, text: settings.text, position: settings.position, color: settings.color } }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export interface VectorPath { data: string; fillRule: 'nonZero' | 'evenOdd' }
export type PathOperation = 'union' | 'difference' | 'intersection' | 'xor';
export type PathEditAction = 'join' | 'average' | 'outline' | 'offset' | 'reverse' | 'simplify' | 'smooth' | 'addAnchors' | 'removeAnchors' | 'divideBelow' | 'splitGrid' | 'cleanUp';
export interface VectorPaint { color: [number, number, number, number] }
export interface VectorObject { id: string; name: string; path: VectorPath; transform: [number, number, number, number, number, number]; fill: VectorPaint | null; stroke: VectorPaint | null; strokeWidth: number; visible: boolean; kind: 'path' | 'bezier' | 'compound' | 'rectangle' | 'ellipse' | 'text'; text?: VectorText; controlPoints: [number, number][] }
export interface LayerSettings { id: string; name: string; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number }
export interface DocumentTabSnapshot { id: number; fileName: string | null; dirty: boolean; format: 'legacy' | 'tiled' }
export interface DocumentWorkspaceSnapshot { activeId: number | null; active: DocumentSnapshot | null; documents: DocumentTabSnapshot[] }
export type ColorMode = 'rgb' | 'cmyk';
export type ColorProfile = 'srgb' | 'displayP3' | 'adobeRgb1998' | 'japanColor2001Coated';
export type BitDepth = 8 | 16 | 32;
export type DocumentUnit = 'pixels' | 'inches' | 'centimeters' | 'millimeters';
export type CanvasColor = 'white' | 'transparent';
export interface DocumentSettings { name: string; width: number; height: number; unit: DocumentUnit; resolution: number; artboards: boolean; canvasColor: CanvasColor; pixelAspectRatio: number }
export const emptyDocument: DocumentSnapshot = { selection: null, name: 'Untitled-1', width: 960, height: 640, unit: 'pixels', resolution: 72, artboards: false, canvasColor: 'white', pixelAspectRatio: 1, layerId: 'layer-1', layerVisible: true, colorMode: 'rgb', colorProfile: 'srgb', bitDepth: 8, strokeCount: 0, layers: [{ objects: [], id: 'layer-1', name: 'Layer 1', kind: 'paint', visible: true, opacity: 1, locked: false, alphaLocked: false, maskEnabled: false, maskInverted: false, maskDensity: 1, deletable: false, strokeCount: 0 }], selectedVectorObjects: [], textObjects: [], canUndo: false, canRedo: false, revision: 0, dirty: false, fileName: null };

export interface RuntimeInfo {
  version: string;
  platform: string;
  architecture: string;
}

export interface CanvasRequest {
  x: number; y: number; width: number; height: number;
  zoom: number; dark: boolean; visible: boolean;
  brush?: Brush;
  tool?: CanvasTool;
}

export interface CanvasInfo {
  status: 'ready' | 'hidden' | 'unsupported';
  backend: string;
  adapterName: string;
  physicalWidth: number;
  physicalHeight: number;
  scaleFactor: number;
  document: DocumentSnapshot | null;
}

export function editDocument(action: DocumentEditAction): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('edit_document', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function toggleLayer(id: string): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('toggle_layer', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function updateLayer(settings: LayerSettings): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_layer_settings', { settings }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function deleteLayer(id: string): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('delete_layer', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function addPaintLayer(): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('add_paint_layer'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function addVectorLayer(): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('add_vector_layer'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function upsertVectorObject(layerId: string, object: VectorObject): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('upsert_vector_object', { layerId, object }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function selectVectorObjects(ids: string[]): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('select_vector_objects', { ids }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function setVectorObjectVisibility(layerId: string, objectId: string, visible: boolean): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_object_visibility', { layerId, objectId, visible }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function reorderVectorObjects(layerId: string, ids: string[]): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('reorder_vector_objects', { layerId, ids }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function combineSelectedVectors(operation: PathOperation): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('combine_selected_vectors', { operation }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function groupSelectedVectors(): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('group_selected_vectors'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function ungroupSelectedVectors(all = false): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('ungroup_selected_vectors', { all }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function editSelectedPaths(action: PathEditAction): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('edit_selected_paths', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export function reorderLayers(ids: string[]): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('reorder_layers', { ids }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function changeColorMode(mode: ColorMode): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_color_mode', { mode }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function changeDocumentSettings(settings: DocumentSettings): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_document_settings', { settings }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function changeBitDepth(depth: BitDepth): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_bit_depth', { depth }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function changeColorProfile(profile: ColorProfile): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_color_profile', { profile }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export async function subscribeDocument(onDocument: (value: DocumentSnapshot) => void, onError: (error: string) => void) {
  if (!isTauri()) return () => {};
  const stopDocument = await listen<DocumentSnapshot>('document-changed', event => onDocument(event.payload));
  try {
    const stopError = await listen<string>('canvas-error', event => onError(event.payload));
    return () => { stopDocument(); stopError(); };
  } catch (error) { stopDocument(); throw error; }
}

export async function subscribeDocuments(onDocuments: (value: DocumentWorkspaceSnapshot) => void) {
  if (!isTauri()) return () => {};
  return listen<DocumentWorkspaceSnapshot>('documents-changed', event => onDocuments(event.payload));
}

export async function getDocumentWorkspace(): Promise<DocumentWorkspaceSnapshot> {
  if (!isTauri()) return { activeId: 1, active: emptyDocument, documents: [{ id: 1, fileName: null, dirty: false, format: 'legacy' }] };
  return invoke<DocumentWorkspaceSnapshot>('document_workspace');
}

function documentCommand(command: 'new_document' | 'switch_document' | 'close_document', id?: number): Promise<DocumentWorkspaceSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentWorkspaceSnapshot>(command, id === undefined ? undefined : { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export const createDocument = () => documentCommand('new_document');
export const switchDocument = (id: number) => documentCommand('switch_document', id);
export const closeDocument = (id: number) => documentCommand('close_document', id);

// Serialize view updates and teardown, including React StrictMode's setup/cleanup replay.
let canvasQueue: Promise<void> = Promise.resolve();
export function syncCanvas(request: CanvasRequest): Promise<CanvasInfo | null> {
  if (!isTauri()) return Promise.resolve(null);
  const result = canvasQueue.then(() => invoke<CanvasInfo>('sync_canvas', { request }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function finishCanvasPath(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  const result = canvasQueue.then(() => invoke<void>('finish_canvas_path'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function resetCanvasPan(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  const result = canvasQueue.then(() => invoke<void>('reset_canvas_pan'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export async function onNativeScaleChange(callback: () => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return getCurrentWindow().onScaleChanged(callback);
}

export async function getRuntimeInfo(): Promise<RuntimeInfo | null> {
  if (!isTauri()) return null;
  return invoke<RuntimeInfo>('runtime_info');
}

export function projectAction(action: 'open' | 'save' | 'saveAs'): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('project_action', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function importSvgLayer(): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('import_svg_layer'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export interface RecoveryInfo {
  status: { savedRevision: number | null; pending: boolean; error: string | null };
  candidates: { id: string; modifiedMs: number; format: 'legacy' | 'tiled' }[];
}
export async function getRecoveryInfo(): Promise<RecoveryInfo | null> {
  return isTauri() ? invoke<RecoveryInfo | null>('recovery_info') : null;
}
export function restoreRecovery(id: string): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('restore_recovery', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export async function retryRecovery(): Promise<void> { await invoke('retry_recovery'); }
export async function deleteRecovery(id: string): Promise<RecoveryInfo> { return invoke<RecoveryInfo>('delete_recovery', { id }); }
export async function deleteAllRecoveries(): Promise<RecoveryInfo> { return invoke<RecoveryInfo>('delete_all_recoveries'); }

export async function subscribeCanvasTool(onTool: (tool: CanvasTool) => void) {
  if (!isTauri()) return () => {};
  return listen<CanvasTool>('canvas-tool-changed', event => onTool(event.payload));
}

export async function subscribeCanvasZoom(onZoom: (zoom: number) => void) {
  if (!isTauri()) return () => {};
  return listen<number>('canvas-zoom-changed', event => onZoom(event.payload));
}

export async function subscribeCanvasColorSwap(onSwap: () => void) {
  if (!isTauri()) return () => {};
  return listen('canvas-swap-colors', onSwap);
}

export async function subscribeCanvasText(onEdit: () => void) {
  if (!isTauri()) return () => {};
  return listen('canvas-text-edit', onEdit);
}

export function selectLayer(id: string): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('select_layer', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function setVectorStrokeWidth(width: number, color: Brush['color']): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_stroke_width', { width, color }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
