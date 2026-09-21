type Name = 'rectangle' | 'ellipse' | 'open' | 'save' | 'saveAs' | 'brush' | 'vector' | 'vectorSelect' | 'vectorPen' | 'vectorRectangle' | 'vectorEllipse' | 'importVector' | 'zoom' | 'undo' | 'redo' | 'eye' | 'eyeOff' | 'panels' | 'minus' | 'plus';
export const iconPaths: Record<Name, string> = {
  rectangle: 'M3 3h4 M10 3h4 M17 3h4v4 M21 10v4 M21 17v4h-4 M14 21h-4 M7 21H3v-4 M3 14v-4 M3 7V3',
  ellipse: 'M10 3a9 9 0 0 1 4 0 M18 5a9 9 0 0 1 3 5 M21 14a9 9 0 0 1-3 5 M14 21a9 9 0 0 1-4 0 M6 19a9 9 0 0 1-3-5 M3 10a9 9 0 0 1 3-5',
  open: 'M3 7V4h6l3 3h9v3 M3 10h19l-3 10H3z',
  save: 'M4 3h13l4 4v14H3V3z M7 3v6h10V3 M7 21v-8h10v8',
  saveAs: 'M3 3h12l4 4v4 M7 3v6h8V3 M3 3v18h7 M13 18l6-6 3 3-6 6h-3z',
  brush: 'M14 4l6-2-2 6-7 7-4-4 7-7z M7 13c-5 0-1 7-6 7 8 2 10-4 6-7z',
  vector: 'M5 4h4v4H5z M15 16h4v4h-4z M9 6c6 0 8 3 8 10 M7 8v9h8 M5 15h4v4H5z',
  vectorSelect: 'M5 3l13 9-6 2-3 6z',
  vectorPen: 'M5 19l2-6L17 3l4 4-10 10z M7 13l4 4',
  vectorRectangle: 'M4 5h16v14H4z',
  vectorEllipse: 'M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16',
  importVector: 'M4 3h10l5 5v5 M14 3v5h5 M12 18h9 M16.5 13.5v9',
  zoom: 'M16 16l5 5 M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14',
  undo: 'M8 4L3 9l5 5 M3 9h10a7 7 0 0 1 0 14',
  redo: 'M16 4l5 5-5 5 M21 9H11a7 7 0 0 0 0 14',
  eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
  eyeOff: 'M3 3l18 18 M9 5c7-2 13 7 13 7s-1 2-3 4 M6 7c-3 2-4 5-4 5s4 7 10 7c2 0 3 0 4-1',
  panels: 'M3 4h18v16H3z M15 4v16', minus: 'M5 12h14', plus: 'M5 12h14 M12 5v14',
};
export function Icon({ name }: { name: Name }) {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={iconPaths[name]} /></svg>;
}
