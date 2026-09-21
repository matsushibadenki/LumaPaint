import type { CanvasTool } from './bridge';

export const toolModes = ['paint', 'vector', 'layout', 'animation'] as const;
export type ToolMode = typeof toolModes[number];

export const modeTools: Record<ToolMode, readonly CanvasTool[]> = {
  paint: ['brush', 'rectangle', 'ellipse'],
  vector: ['vectorSelect', 'vectorPen', 'vectorRectangle', 'vectorEllipse'],
  layout: ['vectorSelect', 'vectorRectangle', 'vectorEllipse'],
  animation: ['brush', 'rectangle', 'ellipse', 'vectorSelect'],
};
export const modeLabels = {
  paint: 'paintTools', vector: 'vectorTools', layout: 'layoutTools', animation: 'animationTools',
} as const;

export const initialTools: Record<ToolMode, CanvasTool> = {
  paint: 'brush', vector: 'vectorSelect', layout: 'vectorSelect', animation: 'brush',
};

export function modeForTool(mode: ToolMode, tool: CanvasTool): ToolMode {
  // Shared tools retain the current workspace; other shortcuts switch to their home mode.
  return modeTools[mode].includes(tool) ? mode : tool.startsWith('vector') ? 'vector' : 'paint';
}
