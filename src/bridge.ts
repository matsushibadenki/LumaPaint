import { applyNotification, type NotificationState, type WorkspaceNotification } from './workspace-notifications';
import type { Screentone } from './screentone';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow';

// Native session events must only reach this editor, including text and zoom.
const listen: ReturnType<typeof getCurrentWebviewWindow>['listen'] = (event, handler) => getCurrentWebviewWindow().listen(event, handler);

export async function newEditorWindow(): Promise<string> {
  const label = await invoke<string>('new_editor_window');
  const editor = await WebviewWindow.getByLabel(label);
  if (editor) await editor.setFocus();
  return label;
}


export type CanvasTool = 'blur' | 'sharpen' | 'smudge' | 'lasso' | 'polygonLasso' | 'magneticLasso' | 'selectionBrush' | 'paintBucket' | 'cloneStamp' | 'brush' | 'eraser' | 'crop' | 'gradient' | 'eyedropper' | 'rectangle' | 'ellipse' | 'vectorSelect' | 'vectorDirectSelect' | 'vectorScale' | 'vectorRotate' | 'vectorPen' | 'vectorPencil' | 'vectorAnchorAdd' | 'vectorAnchorDelete' | 'vectorAnchorConvert' | 'vectorRectangle' | 'vectorEllipse' | 'imageFrameRectangle' | 'imageFrameEllipse' | 'text' | 'textVertical' | 'textFrame' | 'textFrameVertical' | 'zoomIn' | 'zoomOut' | 'hand';
export type DocumentEditAction = 'createTrimMarks' | 'registrationFill' | 'registrationStroke' | 'undo' | 'redo' | 'toggleLayer' | 'selectAll' | 'deselect' | 'invertSelection' | 'deleteSelectedObjects' | 'clearLayer' | 'lockSelection' | 'lockArtworkAbove' | 'lockOtherLayers' | 'unlockAllObjects' | 'hideSelection' | 'hideArtworkAbove' | 'hideOtherLayers' | 'showAllObjects' | 'copy' | 'cut' | 'paste';
export interface Selection { regions: { shape: 'rectangle' | 'ellipse' | 'polygon' | 'stroke'; points?:{x:number;y:number}[]; radius?:number; bounds: [number, number, number, number]; operation: 'replace' | 'add' | 'subtract' | 'intersect' | 'invert' }[] }
export interface BrushEnvelope { enabled: boolean; attack: number; decay: number; sustain: number; hold: number; release: number; dryness: number }
export interface Brush { opacity?: number; flow?: number; smoothing?: number; alpha?: number; blendMode?: import('./brush-settings').BrushBlendMode; noColor?: boolean; size: number; hardness: number; color: [number, number, number]; simulation?: 'round' | 'ink' | 'pencil' | 'dryBrush'; envelope?: BrushEnvelope }
export interface TransformPanelInfo { corners: [number, number][]; width: number; height: number; rotation: number; shear: number; rectangle: boolean; radii: [number, number, number, number] }
export interface TransformPanelEdit { field: 'x'|'y'|'width'|'height'|'rotation'|'shear'|'corners'; values: [number, number, number, number]; reference: [number, number]; proportional: boolean; scaleCorners: boolean; scaleStrokes: boolean; revision: number; ids: string[] }
export function editTransformPanel(edit: TransformPanelEdit): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('edit_transform_panel', { edit }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export interface PagesSnapshot { facing:boolean; binding:'leftToRight'|'rightToLeft'; active:number; pages:{id:string;number:number;width:number;height:number;spread:number;side:'left'|'right'|'single'}[] }
export interface PageEdit { action:'select'|'add'|'duplicate'|'delete'|'moveBefore'|'moveAfter'|'layout'; index?:number; facing?:boolean; binding?:PagesSnapshot['binding'] }
export function editPages(edit:PageEdit):Promise<DocumentSnapshot>{return invoke('edit_pages',{edit});}
export interface GuidesState {nudge:[number,number];origin:[number,number];snap:boolean;visible:boolean;locked:boolean;nextId:number;selected:string[];items:{id:string;axis:'horizontal'|'vertical'|null;position:number;layerId:string|null}[]}
export interface GuideEdit {action:'visibility'|'lock'|'make'|'release'|'clear'|'delete'|'select'|'move'|'snap'|'origin'|'position'|'moveSelected'|'duplicate'|'selectAll'|'invertSelection'|'deselect'|'increments'|'thirds'|'quarters'|'margins'|'saveLayout'|'loadLayout';axis?:'horizontal'|'vertical';id?:string;position?:number;delta?:[number,number]}
export function editGuides(edit:GuideEdit):Promise<DocumentSnapshot>{return invoke('edit_guides',{edit});}
export function rulerGuide(axis:'horizontal'|'vertical',position:number,phase:number):Promise<void>{return invoke('ruler_guide',{axis,position,phase});}
export interface LayerGroup {id:string;name:string;visible:boolean;locked:boolean;collapsed:boolean;maskEnabled:boolean;maskInverted:boolean;maskDensity:number;children:string[]}
export interface LayerGroupsState {groups:LayerGroup[];roots:string[];members:{id:string;visible:boolean;locked:boolean}[];selected:string[]}
export interface LayerGroupEdit {action:'create'|'createEmpty'|'select'|'move'|'reorder'|'collapse'|'visibility'|'lock'|'rename'|'ungroup'|'delete'|'mask';id?:string;target?:string;name?:string;ids?:string[];additive?:boolean;mask?:{enabled:boolean;inverted:boolean;density:number}}
export function editLayerGroups(edit:LayerGroupEdit):Promise<DocumentSnapshot>{return invoke('edit_layer_groups',{edit});}
export interface CompoundShapeSnapshot { id: string; operation: PathfinderOperation; operands: {id:string;name:string;transform:[number,number,number,number,number,number]}[] }
export type LayerEditTarget = 'content' | 'mask' | 'none';
export interface DocumentSnapshot {
  layerEditTarget: LayerEditTarget;
  editingChannel: DisplayChannel;
  compoundShapes?: CompoundShapeSnapshot[];
  layerGroups:LayerGroupsState;
  guides:GuidesState;
  pages: PagesSnapshot;
  hasLockedObjects: boolean;
  hasHiddenObjects: boolean;
  transformPanel?: TransformPanelInfo | null;
  selectedBounds?: [number, number, number, number] | null;
  savedVectorSelections:{name:string;count:number}[];
  canReselectVectors:boolean;
  activeSavedPath: string | null;
  savedPaths: { guideColor?: [number, number, number, number]; id: string; name: string; components: number; clipping: boolean }[];
  selection: Selection | null;
  name: string;
  width: number; height: number; layerId: string; layerVisible: boolean;
  rasterResolution?: { xPpi: number; yPpi: number } | null;
  unit: DocumentUnit; resolution: number; artboards: boolean; canvasColor: CanvasColor; pixelAspectRatio: number;
  colorMode: ColorMode;
  colorProfile: ColorProfile;
  bitDepth: BitDepth;
  strokeCount: number; layers: LayerSnapshot[]; canUndo: boolean; canRedo: boolean; revision: number; dirty: boolean; fileName: string | null;
  selectedVectorObjects: string[];
  textObjects: TextObjectSnapshot[];
}
export interface WidthStop { position: number; width: number; slope: number }
export interface StrokeStyle {
  cap: 'butt' | 'round' | 'square'; join: 'miter' | 'round' | 'bevel'; miterLimit: number;
  alignment: 'center' | 'inside' | 'outside'; dashArray: number[]; dashOffset: number;
  startArrow: 'none' | 'triangle' | 'open' | 'circle' | 'diamond' | 'square' | 'bar' | 'stealth'; endArrow: 'none' | 'triangle' | 'open' | 'circle' | 'diamond' | 'square' | 'bar' | 'stealth';
  arrowScale: number; profile: 'uniform' | 'taperBoth' | 'taperStart' | 'taperEnd' | 'bulge' | 'custom';
  widthCurve: WidthStop[]; startArrowScale: number | null; endArrowScale: number | null; contourAlignments: ('center' | 'inside' | 'outside')[];
}
export const defaultStrokeStyle: StrokeStyle = { cap: 'butt', join: 'miter', miterLimit: 4, alignment: 'center', dashArray: [], dashOffset: 0, startArrow: 'none', endArrow: 'none', arrowScale: 1, profile: 'uniform', widthCurve: [{ position: 0, width: 1, slope: 0 }, { position: 1, width: 1, slope: 0 }], startArrowScale: null, endArrowScale: null, contourAlignments: [] };
export interface GradientStop { position: number; color: [number,number,number,number]; midpoint: number }
export interface Gradient { pixelStyle?: 'angular'|'reflected'|'diamond'; geometry?: [number,number,number,number,number,number]; kind: "linear" | "radial"; angle: number; aspect: number; dither?: boolean; method: "classic" | "linear" | "perceptual"; stops: GradientStop[] }
export type FrameFit="contain"|"cover"|"stretch";
export interface ImageFrameSummary {fitting:FrameFit;sourcePath:string|null;name:string|null;size:[number,number]|null;contentTransform:[number,number,number,number,number,number]|null}
// Native snapshots omit strokeStyle when it equals defaultStrokeStyle.
// Missing gradients/imageFrame mean no gradient/frame; custom settings are complete.
export interface LayerObjectSnapshot { imageFrame?:ImageFrameSummary|null; fillGradient?: Gradient | null; strokeGradient?: Gradient | null; locked: boolean; strokeContours: boolean[]; opacity: number; blendMode: string; fillColor: [number,number,number,number] | null; strokeColor: [number,number,number,number] | null; strokeWidth: number; strokeStyle?: StrokeStyle; id: string; name: string; groupPath: string[]; clippingMask: boolean; kind: 'path' | 'bezier' | 'compound' | 'rectangle' | 'ellipse' | 'text'; visible: boolean }
export type RasterBlendMode = 'normal' | 'multiply' | 'screen' | 'darken' | 'lighten' | 'difference' | 'exclusion';
export function setRasterBlendMode(id: string, mode: RasterBlendMode): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_raster_blend_mode', { id, mode }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export type MaskKind = 'pixel' | 'vector';
export interface LayerMask {kind:MaskKind;enabled:boolean;inverted:boolean;density:number;linked:boolean;transform:[number,number,number,number,number,number]}
export function createLayerMask(id:string,kind:MaskKind):Promise<DocumentSnapshot> {
  const result=canvasQueue.then(()=>invoke<DocumentSnapshot>('create_layer_mask',{id,kind}));
  canvasQueue=result.then(()=>undefined,()=>undefined);return result;
}
export function transformLayerMask(id:string,matrix:LayerMask['transform']):Promise<DocumentSnapshot> {
  const result=canvasQueue.then(()=>invoke<DocumentSnapshot>('transform_layer_mask',{id,matrix}));
  canvasQueue=result.then(()=>undefined,()=>undefined);return result;
}
export interface LayerEffects { mask?:LayerMask|null; screentone?: Screentone | null; enabled: boolean; values: number[]; curves: [number, number][][]; curveSmooth: boolean[]; mixer: number[][]; grading: number[][]; gradingBlend: number; gradingBalance: number }
export const defaultLayerEffects = (): LayerEffects => ({ enabled: false, curveSmooth: Array(4).fill(true), gradingBlend: 50, gradingBalance: 0, mixer: Array.from({length:8},()=>[0,0,0]), grading: Array.from({length:3},()=>[0,0,0]), values: Array(10).fill(0), curves: Array.from({length: 4}, () => [[0,0],[1,1]]) });
export function setLayerEffects(id: string, effects: LayerEffects): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_layer_effects', { id, effects }));
  canvasQueue = result.then(() => undefined, () => undefined); return result;
}
export interface LayerSnapshot {
  effects?: LayerEffects;
  rasterBlendMode?: RasterBlendMode;
  guideColor?: [number, number, number, number]; objects: LayerObjectSnapshot[]; id: string; name: string; kind: 'paint' | 'svg' | 'vector'; visible: boolean; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number; deletable: boolean; strokeCount: number }
export interface TextStyle { noColor?: boolean; fontFamily: string; fontSize: number; scaleX: number; scaleY: number; rotation: number; bold: boolean; italic: boolean; rotateLatin?: boolean; tateChuYoko?: boolean; kerning: 'metrics' | 'optical' | 'japaneseMonospaced'; tracking: number; baselineShift: number; underline: boolean; strikethrough: boolean; color: [number, number, number] }
export interface TextRun { start: number; end: number; style: TextStyle }
export interface TextGlyphCluster { start: number; end: number; x: number }
export interface TextSelection { start: number; length: number; characters: number; style: TextStyle; mixed: (keyof TextStyle)[] }
export interface VectorText { noColor?: boolean;
  pointText?: boolean;
  changeGeneration: number;
  updatedAtMs: number;
  writingMode?: 'horizontal' | 'vertical';
  runs?: TextRun[]; softBreaks?: number[]; lineBaselines?: number[]; lineWidths?: number[]; lineOrigins?: number[]; styleSegmentOrigins?: number[][]; characterOrigins?: number[][]; glyphClusters?: TextGlyphCluster[][]; layoutBounds?: [number, number, number, number];
  content: string; fontFamily: string; fontSize: number; lineHeight: number; bold: boolean;
  italic: boolean; rotateLatin?: boolean; tateChuYoko?: boolean; kerning: 'metrics' | 'optical' | 'japaneseMonospaced'; tracking: number; scaleX: number; scaleY: number; baselineShift: number;
  rotation: number; underline: boolean; strikethrough: boolean; alignment: 'left' | 'center' | 'right' | 'justify';
  boxWidth: number; boxHeight?: number | null; indentLeft: number; indentRight: number; indentFirst: number; spaceBefore: number; spaceAfter: number;
  listStyle: 'none' | 'bullets' | 'numbers'; kinsoku: 'none' | 'standard' | 'strict'; mojikumi: 'none' | 'japanese'; hyphenation: boolean;
}
export const defaultVectorText: VectorText = {
  changeGeneration: 0, updatedAtMs: 0,
  content: 'Text', runs: [], softBreaks: [], fontFamily: 'sans-serif', fontSize: 48, lineHeight: 1.4, bold: false,
  italic: false, rotateLatin: true, tateChuYoko: false, kerning: 'metrics', tracking: 0, scaleX: 1, scaleY: 1, baselineShift: 0, rotation: 0,
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
export interface VectorObject { rectangleRadii?: [number, number, number, number]; id: string; name: string; path: VectorPath; transform: [number, number, number, number, number, number]; fill: VectorPaint | null; stroke: VectorPaint | null; strokeWidth: number; strokeStyle?: StrokeStyle; visible: boolean; kind: 'path' | 'bezier' | 'compound' | 'rectangle' | 'ellipse' | 'text'; text?: VectorText; controlPoints: [number, number][] }
export interface LayerSettings { id: string; name: string; opacity: number; locked: boolean; alphaLocked: boolean; maskEnabled: boolean; maskInverted: boolean; maskDensity: number }
export interface DocumentTabSnapshot { id: number; fileName: string | null; dirty: boolean; format: 'legacy' | 'tiled' }
export interface DocumentWorkspaceSnapshot { activeId: number | null; active: DocumentSnapshot | null; documents: DocumentTabSnapshot[] }
export type ColorMode = 'rgb' | 'cmyk' | 'grayscale' | 'lab';
export type ColorProfile = 'srgb' | 'displayP3' | 'adobeRgb1998' | 'japanColor2001Coated' | 'grayD65' | 'labD50';
export type BitDepth = 8 | 16 | 32;
export type DocumentUnit = 'pixels' | 'inches' | 'centimeters' | 'millimeters' | 'points';
export type CanvasColor = 'white' | 'transparent';
export interface DocumentSettings { name: string; width: number; height: number; unit: DocumentUnit; resolution: number; artboards: boolean; canvasColor: CanvasColor; pixelAspectRatio: number }
export type NewDocumentGuideLayout = {kind: 'print'; bleedMm: number} | {kind: 'manga'; trimWidthMm: number; trimHeightMm: number};
export interface NewDocumentSettings { guideLayout?: NewDocumentGuideLayout; pages?:{count:number;facing:boolean;binding:PagesSnapshot['binding']}; document: DocumentSettings; colorMode: ColorMode; colorProfile: ColorProfile; bitDepth: BitDepth }
export const emptyDocument: DocumentSnapshot = {
  savedVectorSelections:[],canReselectVectors:false,layerGroups:{groups:[],roots:[],members:[],selected:[]}, guides:{nudge:[1,10],origin:[0,0],snap:true,visible:true,locked:true,nextId:1,selected:[],items:[]}, pages:{facing:false,binding:"leftToRight",active:0,pages:[{id:"page-1",number:1,width:960,height:640,spread:0,side:"single"}]}, hasHiddenObjects: false, hasLockedObjects: false, activeSavedPath: null, savedPaths: [], selection: null, name: 'Untitled-1', width: 960, height: 640, unit: 'pixels', resolution: 72, artboards: false, canvasColor: 'white', pixelAspectRatio: 1, layerId: 'layer-1', layerEditTarget: 'content', editingChannel: 0, layerVisible: true, colorMode: 'rgb', colorProfile: 'srgb', bitDepth: 8, strokeCount: 0, layers: [{ objects: [], id: 'layer-1', name: 'Layer 1', kind: 'paint', visible: true, opacity: 1, locked: false, alphaLocked: false, maskEnabled: false, maskInverted: false, maskDensity: 1, deletable: false, strokeCount: 0 }], selectedVectorObjects: [], textObjects: [], canUndo: false, canRedo: false, revision: 0, dirty: false, fileName: null };

export interface RuntimeInfo {
  version: string;
  platform: string;
  architecture: string;
}

export type DisplayChannel = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
export interface CanvasRequest {
  overlays?: [number,number,number,number][];
  overlay?: [number, number, number, number] | null;
  channel?: DisplayChannel;
  x: number; y: number; width: number; height: number;
  zoom: number; absoluteZoom?: boolean; pasteboardColor?: Brush['color'] | null; dark: boolean; visible: boolean;
  zoomRevision?: number;
  brush?: Brush;
  tool?: CanvasTool;
}

export interface RulerViewport { rulerOrigin?:[number,number]; width: number; height: number; originX: number; originY: number; zoom: number }

export interface CanvasInfo {
  status: 'ready' | 'hidden' | 'unsupported';
  backend: string;
  adapterName: string;
  physicalWidth: number;
  physicalHeight: number;
  scaleFactor: number;
  zoom?: number | null;
  rulerViewport?: RulerViewport | null;
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
export type ArrangeAction = 'front' | 'forward' | 'backward' | 'back' | 'moveToLayer';
export function arrangeSelectedVectors(action: ArrangeAction): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('arrange_selected_vectors', { action }));
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

export async function subscribeCanvasError(onError: (error: string) => void) {
  if (!isTauri()) return () => {};
  return listen<string>('canvas-error', event => onError(event.payload));
}

export async function subscribeDocuments(onDocuments: (value: DocumentWorkspaceSnapshot) => void, onError: (error: string) => void = console.error) {
  if (!isTauri()) return () => {};
  let state: NotificationState | undefined;
  let live = true;
  let pending = true;
  const resync = () => {
    pending = true;
    return invoke('resync_document_notifications').catch(cause => {
      pending = false;
      if (live) onError(String(cause));
    }).finally(() => { pending = false; });
  };
  const stop = await listen<WorkspaceNotification>('documents-changed', event => {
    if (!live) return;
    const next = applyNotification(state, event.payload);
    if (!next) {
      if (!pending) void resync();
      return;
    }
    if (next === state) return;
    state = next;
    pending = false;
    onDocuments(next.workspace);
  });
  // Register before requesting a full packet; no command snapshot can overwrite
  // a newer event while the initial subscription is being established.
  await resync();
  return () => { live = false; stop(); };
}

export async function getDocumentWorkspace(): Promise<DocumentWorkspaceSnapshot> {
  if (!isTauri()) return { activeId: null, active: null, documents: [] };
  return invoke<DocumentWorkspaceSnapshot>('document_workspace');
}

function documentCommand(command: 'new_document' | 'switch_document' | 'close_document', id?: number): Promise<DocumentWorkspaceSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentWorkspaceSnapshot>(command, id === undefined ? undefined : { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function createDocument(settings: NewDocumentSettings): Promise<DocumentWorkspaceSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentWorkspaceSnapshot>('new_document', { settings }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export async function subscribeNewDocument(onNew: () => void) {
  if (!isTauri()) return () => {};
  return listen('new-document-requested', onNew);
}
export const switchDocument = (id: number) => documentCommand('switch_document', id);
export const closeDocument = (id: number) => documentCommand('close_document', id);

// Serialize view updates and teardown, including React StrictMode's setup/cleanup replay.
let canvasQueue: Promise<void> = Promise.resolve();
export function editorWindowTargets():Promise<[string,string][]> { return invoke('editor_window_targets'); }
export function openDocumentView(id:number,target:string):Promise<DocumentWorkspaceSnapshot> {
  const result=canvasQueue.then(()=>invoke<DocumentWorkspaceSnapshot>('open_document_view',{id,target}));
  canvasQueue=result.then(()=>undefined,()=>undefined);return result;
}
export function moveDocumentToWindow(id:number,target:string):Promise<DocumentWorkspaceSnapshot> {
  const result=canvasQueue.then(()=>invoke<DocumentWorkspaceSnapshot>('move_document_to_window',{id,target}));
  canvasQueue=result.then(()=>undefined,()=>undefined);return result;
}

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

export function dropFiles(paths: string[], targetId: number | null): Promise<{workspace: DocumentWorkspaceSnapshot; errors: string[]}> {
  const result = canvasQueue.then(() => invoke<{workspace: DocumentWorkspaceSnapshot; errors: string[]}>('drop_files', { paths, targetId }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function projectAction(action: 'open' | 'save' | 'saveAs' | 'export'): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('project_action', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function finishRasterImport(commit: boolean): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('finish_raster_import', { commit }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export async function subscribeRasterPlacement(onChange: (active: boolean) => void) {
  return listen<boolean>('raster-placement', event => onChange(event.payload));
}
export function importRasterLayer(format: 'all' | 'jpeg' | 'png'): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('import_raster_layer', { format }));
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

export async function subscribeCanvasSampledColor(onColor: (color: Brush['color']) => void) {
  if (!isTauri()) return () => {};
  return listen<Brush['color']>('canvas-sampled-color', event => onColor(event.payload));
}

export async function subscribeCanvasColorSwap(onSwap: () => void) {
  if (!isTauri()) return () => {};
  return listen('canvas-swap-colors', onSwap);
}

export async function subscribeCanvasText(onEdit: () => void) {
  if (!isTauri()) return () => {};
  return listen('canvas-text-edit', onEdit);
}

export function selectChannel(channel: DisplayChannel): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('select_channel', { channel }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function selectLayerTarget(id: string, target: LayerEditTarget): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('select_layer_target', { id, target }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function selectLayer(id: string, preserveObjects = false): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>(preserveObjects ? 'select_arrange_layer' : 'select_layer', { id }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function strokePreview(width: number, style: StrokeStyle): Promise<string> {
  return invoke<string>('stroke_preview', { width, style });
}
export function setVectorStrokeStyle(patch: Partial<StrokeStyle>): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_stroke_style', { patch }));
  canvasQueue = result.then(() => {}, () => {});
  return result;
}
export function setVectorStrokeWidth(width: number, color: Brush['color']): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_stroke_width', { width, color }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function clippingPath(action: 'create' | 'release' | 'edit'): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('clipping_path', { action }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function compoundPath(release: boolean): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('compound_path', { release }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export type TransformAction = 'move' | 'rotate' | 'reflect' | 'scale' | 'shear' | 'individual' | 'reset';
export function transformObjects(action: TransformAction, values: number[]): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('transform_objects', { action, values }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function setVectorPaint(ids: string[], target: 'fill'|'stroke'|'swap', color: Brush['color'] | null): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_paint', { ids, target, color }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function outlineText(): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('outline_text'));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function outlineView(value?: boolean): Promise<boolean> {
  const result = canvasQueue.then(() => invoke<boolean>('outline_view', { value: value ?? null }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function setTextWritingMode(mode: 'horizontal' | 'vertical'): Promise<DocumentSnapshot> {
  return textCommand<DocumentSnapshot>('text_writing_mode', { mode });
}

export function savedPathAction(action: string, id: string | null, name: string): Promise<DocumentSnapshot> {
  return textCommand<DocumentSnapshot>('saved_path_action', { action, id, name });
}

export function setVectorAppearance(ids: string[], opacity: number | null, blendMode: string | null): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('set_vector_appearance', { ids, opacity, blendMode }));
  canvasQueue = result.then(() => {}, () => {});
  return result;
}

export interface DirectControlInfo { id:string; index:number; x:number; y:number; anchor:boolean; radius:number|null }
export function directControlInfo():Promise<DirectControlInfo[]> { return isTauri()?textCommand('direct_control_info',{}):Promise.resolve([]); }
export function editDirectControls(mode:'position'|'move'|'corner',values:number[],preview:boolean,points:DirectControlInfo[],revision:number):Promise<{snapshot:DocumentSnapshot;preview:string}> {
  return textCommand('edit_direct_controls',{mode,values,preview,expected:points.map(p=>[p.id,p.index]),revision});
}

export type PathfinderOperation = 'unite' | 'minusFront' | 'intersect' | 'exclude' | 'divide' | 'trim' | 'merge' | 'crop' | 'outline' | 'minusBack';
export function pathfinderVectors(operation: PathfinderOperation): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('pathfinder_vectors', { operation }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function makeCompoundShape(operation: PathfinderOperation): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('make_compound_shape', { operation }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}
export interface CompoundShapeEdit { action:'update'|'expand'|'release'; operation?:PathfinderOperation; operand?:string; translation?:[number,number] }
export function editCompoundShape(edit:CompoundShapeEdit):Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('edit_compound_shape', { edit }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}

export function applyGradient(ids: string[], target: 'fill' | 'stroke' | 'pixels' | 'gradientLayer', gradient: Gradient): Promise<DocumentSnapshot> {
  const result = canvasQueue.then(() => invoke<DocumentSnapshot>('apply_gradient', { ids, target, gradient }));
  canvasQueue = result.then(() => undefined, () => undefined);
  return result;
}



export type SwatchPaint = {kind:'none'} | {kind:'color';color:Brush['color'];registration?:boolean} | {kind:'gradient';gradient:Gradient};
export type SwatchDraft = {name:string;paint:SwatchPaint};
export type Swatch = SwatchDraft & {id:number};
export async function swatchLibrary(action:'get'|'add'|'addMany'|'update'|'remove'|'import'|'export', draft?:SwatchDraft, id?:number, seeds?:SwatchDraft[], batch?:SwatchDraft[]):Promise<Swatch[]> {
  if (!isTauri()) { if(action==='get')return (seeds??[]).map((s,i)=>({...s,id:i+1}));throw new Error('LumaPaint app required / LumaPaintアプリで使用してください / 请在LumaPaint应用中使用'); }
  return invoke<Swatch[]>('swatch_library',{action,draft:draft??null,id:id??null,seeds:seeds??null,batch:batch??null});
}

export async function watchSwatches(callback:()=>void):Promise<()=>void> {
  return isTauri()?listen('swatches-changed',callback):()=>{};
}


// Adapter support and editor menu integration are deliberately separate.
export interface FileFormatCapability {
  format: 'native' | 'svg' | 'pdf' | 'psd' | 'ora' | 'exr' | 'kra' | 'png' | 'jpeg' | 'webp' | 'gif' | 'bmp' | 'tiff' | 'raw' | 'heif' | 'ico' | 'avif' | 'illustrator';
  extensions: string[];
  family: 'native' | 'vector' | 'raster' | 'layered' | 'hdr' | 'cameraRaw';
  plannedOpen: boolean;
  plannedImport: boolean;
  plannedExport: boolean;
  extension: string;
  mediaType: string;
  openAdapter: boolean;
  importAdapter: boolean;
  exportAdapter: boolean;
  editorOpen: boolean;
  editorImport: boolean;
  editorExport: boolean;
  partial: boolean;
}
export async function fileFormatCapabilities(): Promise<FileFormatCapability[]> {
  return isTauri() ? invoke<FileFormatCapability[]>('file_format_capabilities') : [];
}

export function gradientToolOptions(): Promise<{gradient:Gradient;gradientTarget:"fill"|"stroke"}> { return textCommand("tool_options",{}); }

export function setGradientTool(gradient: Gradient,target:"fill"|"stroke"):Promise<void> { return textCommand("set_gradient_tool",{gradient,target}); }

export function placeImage(id?:string):Promise<DocumentSnapshot>{return textCommand('place_image',{id:id??null});}
export interface ImageLink {id:string;name:string;path:string|null;status:'empty'|'normal'|'modified'|'missing'|'embedded';format:string;bytes:number;modifiedAt:number|null}
export function imageLinks():Promise<ImageLink[]>{return textCommand('image_links',{});}
export function imageFrameAction(id:string,action:'update'|'relink'|'embed'|'go'|FrameFit|'offset',values?:number[]):Promise<DocumentSnapshot>{return textCommand('image_frame_action',{id,action,values:values??null});}
export function subscribePlaceImage(handler:()=>void):Promise<()=>void>{return listen('place-image-requested',handler);}

export function manageImageLinks(ids:string[],action:"update"|"relink"|"embed"):Promise<DocumentSnapshot>{return textCommand("manage_image_links",{ids,action});}

export function rulerOrigin(point:[number,number],phase:number):Promise<void>{return invoke('ruler_origin',{point,phase});}

export type VectorSelectionRequest={action:string;criterion?:string;name?:string;newName?:string};
export const vectorSelectionAction=(request:VectorSelectionRequest)=>invoke<DocumentSnapshot>('vector_selection_action',{request});

export function createScreentoneLayer(tone: Screentone): Promise<DocumentSnapshot> { return textCommand("create_screentone_layer", {tone}); }
