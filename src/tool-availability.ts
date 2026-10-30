import type { CanvasTool, DocumentSnapshot } from './bridge';
import type { Locale } from './i18n';

export interface ToolContext {
  document: DocumentSnapshot;
  available: boolean;
  editable: boolean;
  busy: boolean;
}
const reasons = {
  unavailable: { en: 'Open a document to use this tool.', ja: 'ドキュメントを開いてください。', 'zh-CN': '请先打开文档。' },
  readOnly: { en: 'This document is read-only.', ja: 'このドキュメントは読み取り専用です。', 'zh-CN': '此文档为只读。' },
  busy: { en: 'Wait for the current operation to finish.', ja: '実行中の操作が完了するまでお待ちください。', 'zh-CN': '请等待当前操作完成。' },
  noLayer: { en: 'Select a layer to use this tool.', ja: 'レイヤーを選択してください。', 'zh-CN': '请先选择图层。' },
  locked: { en: 'Unlock the selected layer or its group.', ja: '選択レイヤーまたは所属グループのロックを解除してください。', 'zh-CN': '请解锁所选图层或其所在组。' },
  hidden: { en: 'Show the selected layer and its groups.', ja: '選択レイヤーと所属グループを表示してください。', 'zh-CN': '请显示所选图层及其所在组。' },
  paint: { en: 'Select a paint layer to use this tool.', ja: 'ペイントレイヤーを選択してください。', 'zh-CN': '请选择绘画图层。' },
  vector: { en: 'Select a vector layer to use this tool.', ja: 'ベクターレイヤーを選択してください。', 'zh-CN': '请选择矢量图层。' },
  mask: { en: 'This tool cannot edit a mask. Select layer content.', ja: 'このツールはマスクに対応していません。レイヤー本体を選択してください。', 'zh-CN': '此工具无法编辑蒙版。请选择图层内容。' },
} as const;
const viewTools = new Set<CanvasTool>(['hand', 'zoomIn', 'zoomOut', 'eyedropper']);
const selectionTools = new Set<CanvasTool>(['rectangle', 'ellipse', 'lasso', 'polygonLasso', 'magneticLasso', 'selectionBrush', 'objectSelection', 'quickSelection', 'magicWand']);
const maskTools = new Set<CanvasTool>(['brush', 'eraser', ...selectionTools]);

export function toolUnavailableReason(tool: CanvasTool, context: ToolContext, locale: Locale): string | undefined {
  const reason = (key: keyof typeof reasons) => reasons[key][locale];
  if (!context.available) return reason('unavailable');
  if (viewTools.has(tool)) return undefined;
  if (context.busy) return reason('busy');
  if (!context.editable) return reason('readOnly');
  const doc = context.document;
  // Crop edits the document; pixel selections are also document-wide.
  if (tool === 'crop' || selectionTools.has(tool)) return undefined;
  const layer = doc.layers.find(item => item.id === doc.layerId);
  if (!layer || doc.layerEditTarget === 'none') return reason('noLayer');
  const member = doc.layerGroups.members.find(item => item.id === layer.id);
  const ancestors = new Set<string>([layer.id]);
  for (let changed = true; changed;) {
    changed = false;
    for (const group of doc.layerGroups.groups) {
      if (!ancestors.has(group.id) && group.children.some(id => ancestors.has(id))) {
        ancestors.add(group.id); changed = true;
      }
    }
  }
  const groups = doc.layerGroups.groups.filter(group => ancestors.has(group.id));
  if (layer.locked || member?.locked || groups.some(group => group.locked)) return reason('locked');
  if (!layer.visible || member?.visible === false || groups.some(group => !group.visible)) return reason('hidden');
  if (doc.layerEditTarget === 'mask') return maskTools.has(tool) ? undefined : reason('mask');
  const vectorTool = tool.startsWith('vector') || tool.startsWith('text') || tool.startsWith('imageFrame');
  if (vectorTool) return layer.kind === 'vector' ? undefined : reason('vector');
  if (tool === 'gradient') return layer.kind === 'paint' || layer.kind === 'vector' ? undefined : reason('paint');
  return layer.kind === 'paint' ? undefined : reason('paint');
}

// Each toolbar menu contains tools with the same layer requirements.
export function toolbarTool(item: string): CanvasTool {
  return ({ selection: 'rectangle', vectorSelection: 'vectorSelect', transform: 'vectorScale', zoom: 'zoomIn' } as Record<string, CanvasTool>)[item] ?? item as CanvasTool;
}
