import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

export type CanvasTool = 'brush' | 'rectangle' | 'ellipse' | 'vector';
export type DocumentEditAction = 'undo' | 'redo' | 'toggleLayer' | 'selectAll' | 'deselect' | 'invertSelection';
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
}
export interface LayerSnapshot { id: string; name: string; kind: 'paint' | 'svg' | 'vector'; visible: boolean; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number; deletable: boolean; strokeCount: number }
export interface VectorPath { data: string; fillRule: 'nonZero' | 'evenOdd' }
export interface VectorPaint { color: [number, number, number, number] }
export interface VectorObject { id: string; name: string; path: VectorPath; transform: [number, number, number, number, number, number]; fill: VectorPaint | null; stroke: VectorPaint | null; strokeWidth: number; visible: boolean }
export interface LayerSettings { id: string; name: string; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number }
export interface DocumentTabSnapshot { id: number; fileName: string | null; dirty: boolean }
export interface DocumentWorkspaceSnapshot { activeId: number | null; active: DocumentSnapshot | null; documents: DocumentTabSnapshot[] }
export type ColorMode = 'rgb' | 'cmyk';
export type ColorProfile = 'srgb' | 'displayP3' | 'adobeRgb1998' | 'japanColor2001Coated';
export type BitDepth = 8 | 16 | 32;
export type DocumentUnit = 'pixels' | 'inches' | 'centimeters' | 'millimeters';
export type CanvasColor = 'white' | 'transparent';
export interface DocumentSettings { name: string; width: number; height: number; unit: DocumentUnit; resolution: number; artboards: boolean; canvasColor: CanvasColor; pixelAspectRatio: number }
export const emptyDocument: DocumentSnapshot = { selection: null, name: 'Untitled-1', width: 960, height: 640, unit: 'pixels', resolution: 72, artboards: false, canvasColor: 'white', pixelAspectRatio: 1, layerId: 'layer-1', layerVisible: true, colorMode: 'rgb', colorProfile: 'srgb', bitDepth: 8, strokeCount: 0, layers: [{ id: 'layer-1', name: 'Layer 1', kind: 'paint', visible: true, opacity: 1, locked: false, alphaLocked: false, maskEnabled: false, maskInverted: false, maskDensity: 1, deletable: false, strokeCount: 0 }], canUndo: false, canRedo: false, revision: 0, dirty: false, fileName: null };

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
  if (!isTauri()) return { activeId: 1, active: emptyDocument, documents: [{ id: 1, fileName: null, dirty: false }] };
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
  candidates: { id: string; modifiedMs: number }[];
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
