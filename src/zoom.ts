// Document pixels per screen point (100% = 1), independent of Fit.
export const MIN_ZOOM = 0.0313;
export const MAX_ZOOM = 640;
export const ZOOM_PERCENTAGES = [3.13, 4.17, 6.25, 8.33, 12.5, 16.67, 25, 33.33, 50, 66.67, 100, 150, 200, 300, 400, 600, 800, 1200, 1600, 2400, 3200, 4800, 6400, 8500, 12750, 17000, 25500, 34000, 51000, 64000] as const;
export function stepZoom(zoom: number, direction: 1 | -1): number {
  const percentages = direction === 1 ? ZOOM_PERCENTAGES : [...ZOOM_PERCENTAGES].reverse();
  return (percentages.find(percent => direction === 1 ? percent / 100 > zoom + 0.00001 : percent / 100 < zoom - 0.00001) ?? (direction === 1 ? 64000 : 3.13)) / 100;
}
export function zoomLabel(zoom: number): string { return `${Number((zoom * 100).toFixed(2))}%`; }
