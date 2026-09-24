import type { CanvasTool } from './bridge';

export const toolModes = ['paint', 'vector', 'layout', 'animation'] as const;
export type ToolMode = typeof toolModes[number];
export type ModeTool = Exclude<CanvasTool, 'zoomIn' | 'zoomOut' | 'hand'>;

export const modeTools: Record<ToolMode, readonly ModeTool[]> = {
  paint: ['brush', 'eraser', 'rectangle', 'ellipse'],
  vector: ['vectorSelect', 'vectorDirectSelect', 'vectorPen', 'vectorPencil', 'vectorAnchorAdd', 'vectorAnchorDelete', 'vectorAnchorConvert', 'vectorRectangle', 'vectorEllipse'],
  layout: ['vectorSelect', 'vectorDirectSelect', 'vectorRectangle', 'vectorEllipse', 'text'],
  animation: ['brush', 'rectangle', 'ellipse', 'vectorSelect', 'vectorDirectSelect'],
};
export const modeLabels = {
  paint: 'paintTools', vector: 'vectorTools', layout: 'layoutTools', animation: 'animationTools',
} as const;

export const initialTools: Record<ToolMode, ModeTool> = {
  paint: 'brush', vector: 'vectorSelect', layout: 'vectorSelect', animation: 'brush',
};

export function modeForTool(mode: ToolMode, tool: ModeTool): ToolMode {
  // Shared tools retain the current workspace; other shortcuts switch to their home mode.
  if (tool === 'text') return 'layout';
  return modeTools[mode].includes(tool) ? mode : tool.startsWith('vector') ? 'vector' : 'paint';
}

export const penTools = ['vectorPen', 'vectorPencil', 'vectorAnchorAdd', 'vectorAnchorDelete', 'vectorAnchorConvert'] as const;
export type PenTool = typeof penTools[number];
export function isPenTool(tool: CanvasTool): tool is PenTool { return (penTools as readonly string[]).includes(tool); }
