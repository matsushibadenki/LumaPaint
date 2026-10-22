import vector from './assets/feature-types/vector.svg';
import pixel from './assets/feature-types/pixel.svg';
import combined from './assets/feature-types/combined.svg';
import animation from './assets/feature-types/animation.svg';
import type {Locale} from './i18n';
export type FeatureType = 'vector' | 'pixel' | 'combined' | 'animation';
export const featureIcons = {vector, pixel, combined, animation};
export const featureLabels: Record<Locale, Record<FeatureType, string>> = {
  ja: {vector:'ベクター', pixel:'ピクセル', combined:'ベクター・ピクセル', animation:'アニメーション'},
  en: {vector:'Vector', pixel:'Pixel', combined:'Vector and pixel', animation:'Animation'},
  'zh-CN': {vector:'矢量', pixel:'像素', combined:'矢量和像素', animation:'动画'},
};
// Classify by command identity, never by translated labels or current selection.
export function commandFeature(id: string | undefined, fallback?: FeatureType): FeatureType | undefined {
  if (!id) return fallback;
  if (['shortcuts.edit','command.new-window','menu.panels','menu.resetWorkspace'].includes(id)) return undefined;
  if (id.startsWith('pixelSelection.layers.')) return 'combined';
  if (id.startsWith('pixelSelection.')) return 'pixel';
  if (id === 'menu.importSvg' || id === 'command.place') return 'vector';
  if (id === 'command.import') return 'pixel';
  if (id === 'window.timeline') return 'animation';
  if (/^(vector\.|savedSelection\.|align\.|arrange\.|text\.|clipping\.|compound\.)/.test(id)
    || /^(menu\.(align|path|group|ungroup)|command\.(create-outlines|compound-path|create-trim-marks|outline)$)/.test(id)
    || ['guides.make','guides.release'].includes(id)) return 'vector';
  if (/^menu\.(lock|hide|show|unlock)/.test(id) || id.startsWith('layerGroup.')) return 'combined';
  return fallback;
}
