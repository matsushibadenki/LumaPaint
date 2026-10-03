import { pixelsPerMeasurement } from './measurement-math';
import type { DocumentUnit, NewDocumentSettings } from './bridge';

export type PresetCategory = 'photo' | 'print' | 'art' | 'web' | 'mobile' | 'video';
export interface PaperPreset { id: string; category: PresetCategory; name: string; width: number; height: number; unit: DocumentUnit; resolution: number }
function presets(category: PresetCategory, unit: DocumentUnit, resolution: number, sizes: [string, number, number][]): PaperPreset[] {
  return sizes.map(([name, width, height]) => ({ id: `${category}-${name}`, category, name, width, height, unit, resolution }));
}
export const paperPresets: PaperPreset[] = [
  ...presets('print', 'millimeters', 300, [['A4', 210, 297], ['A6', 105, 148], ['A5', 148, 210], ['A3', 297, 420], ['B5 (ISO)', 176, 250], ['B5 (JIS)', 182, 257], ['B4 (ISO)', 250, 353], ['B4 (JIS)', 257, 364], ['B3 (ISO)', 353, 500], ['B3 (JIS)', 364, 515], ['C4', 229, 324], ['C5', 162, 229], ['C6', 114, 162], ['DL', 110, 220], ['Postcard', 100, 148]]),
  ...presets('print', 'inches', 300, [['Letter', 8.5, 11], ['Legal', 8.5, 14], ['Tabloid', 11, 17]]),
  ...presets('photo', 'millimeters', 300, [['L', 89, 127], ['2L', 127, 178], ['6-cut', 203, 254], ['4-cut', 254, 305]]),
  ...presets('photo', 'inches', 300, [['3 × 2', 3, 2], ['6 × 4', 6, 4], ['7 × 5', 7, 5], ['10 × 8', 10, 8], ['2 × 3', 2, 3], ['4 × 6', 4, 6], ['5 × 7', 5, 7], ['8 × 10', 8, 10]]),
  ...presets('art', 'pixels', 300, [['1000 × 1000', 1000, 1000], ['2000 × 2000', 2000, 2000], ['1080p', 1920, 1080], ['720p', 1280, 720]]),
  ...presets('art', 'inches', 300, [['Poster', 18, 24], ['Postcard', 4, 6]]),
  ...presets('web', 'pixels', 72, [['1366 × 768', 1366, 768], ['1920 × 1080', 1920, 1080], ['1440 × 900', 1440, 900], ['1024 × 768', 1024, 768], ['1280 × 800', 1280, 800], ['2560 × 1600', 2560, 1600], ['2880 × 1800', 2880, 1800]]),
  ...presets('mobile', 'pixels', 72, [['1125 × 2436', 1125, 2436], ['1080 × 1920', 1080, 1920], ['750 × 1334', 750, 1334], ['2048 × 2732', 2048, 2732], ['1536 × 2048', 1536, 2048], ['Icon 1024', 1024, 1024], ['Icon 512', 512, 512], ['Icon 256', 256, 256], ['Icon 128', 128, 128], ['Icon 64', 64, 64]]),
  ...presets('video', 'pixels', 72, [['HDTV 1080p', 1920, 1080], ['HDTV 720p', 1280, 720], ['DCI 2K', 2048, 1080], ['UHD 4K', 3840, 2160], ['DCI 4K', 4096, 2160], ['UHD 8K', 7680, 4320], ['DCI 8K', 8192, 4320], ['NTSC', 720, 480], ['PAL', 720, 576], ['Film 2K', 2048, 1556], ['Film 4K', 4096, 3112], ['Film 8K', 8192, 6224]]),
];
export function unitFactor(unit: DocumentUnit, resolution: number) {
  return pixelsPerMeasurement(unit,resolution);
}
export function presetSettings(preset: PaperPreset, name: string): NewDocumentSettings {
  const factor = unitFactor(preset.unit, preset.resolution);
  return { document: { name, width: Math.round(preset.width * factor), height: Math.round(preset.height * factor), unit: preset.unit, resolution: preset.resolution, artboards: preset.category === 'web' || preset.category === 'mobile', canvasColor: 'white', pixelAspectRatio: 1 }, colorMode: 'rgb', colorProfile: 'srgb', bitDepth: 8 };
}
