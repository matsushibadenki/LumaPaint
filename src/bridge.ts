import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

export interface Brush { size: number; hardness: number; color: [number, number, number] }
export interface DocumentSnapshot {
  width: number; height: number; layerId: string; layerVisible: boolean;
  colorMode: ColorMode;
  colorProfile: ColorProfile;
  bitDepth: BitDepth;
  strokeCount: number; layers: LayerSnapshot[]; canUndo: boolean; canRedo: boolean; revision: number; dirty: boolean; fileName: string | null;
}
export interface LayerSnapshot { id: string; name: string; kind: 'paint' | 'svg'; visible: boolean; strokeCount: number }
export interface DocumentTabSnapshot { id: number; fileName: string | null; dirty: boolean }
export interface DocumentWorkspaceSnapshot { activeId: number | null; active: DocumentSnapshot | null; documents: DocumentTabSnapshot[] }
export type ColorMode = 'rgb' | 'cmyk';
export type ColorProfile = 'srgb' | 'displayP3' | 'adobeRgb1998' | 'japanColor2001Coated';
export type BitDepth = 8 | 16 | 32;
export const emptyDocument: DocumentSnapshot = { width: 960, height: 640, layerId: 'layer-1', layerVisible: true, colorMode: 'rgb', colorProfile: 'srgb', bitDepth: 8, strokeCount: 0, layers: [{ id: 'layer-1', name: 'Layer 1', kind: 'paint', visible: true, strokeCount: 0 }], canUndo: false, canRedo: false, revision: 0, dirty: false, fileName: null };

export interface RuntimeInfo {
  version: string;
  platform: string;
  architecture: string;
}

export interface CanvasRequest {
  x: number; y: number; width: number; height: number;
  zoom: number; dark: boolean; visible: boolean;
  brush?: Brush;
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

export function editDocument(action: 'undo' | 'redo' | 'toggleLayer'): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('edit_document', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function toggleLayer(id: string): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('toggle_layer', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function changeColorMode(mode: ColorMode): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_color_mode', { mode }));
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
