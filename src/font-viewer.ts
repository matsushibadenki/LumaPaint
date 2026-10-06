import { invoke, isTauri } from '@tauri-apps/api/core';
import { readPreference, savePreference } from './i18n';
export interface FontFace { postscript: string; family: string; style: string; weight: number; italic: boolean; japanese: boolean; latin: boolean; adobe: boolean }
let catalog: Promise<FontFace[]> | undefined;
export function fontCatalog() {
  return catalog ??= (isTauri() ? invoke<FontFace[]>('font_catalog') : Promise.resolve([])).catch(error => { catalog = undefined; throw error; });
}
// Limit preview traffic to visible rows, two worker jobs and a bounded Blob cache.
const cache = new Map<string, Promise<Blob>>();
let active = 0;
const waiting: (() => void)[] = [];
async function limited<T>(task: () => Promise<T>) {
  if (active >= 2) await new Promise<void>(resolve => waiting.push(resolve));
  else active++;
  try { return await task(); } finally { const next = waiting.shift(); if (next) next(); else active--; }
}
export function fontPreview(face: FontFace, sample: string, size: number, width: number, height: number): Promise<Blob> {
  const key = JSON.stringify([face.postscript, sample, size, width, height]);
  const old = cache.get(key);
  if (old) { cache.delete(key); cache.set(key, old); return old; }
  const pending = limited(async () => {
    if (!isTauri()) throw new Error('Font previews require the desktop app.');
    const bytes = await invoke<ArrayBuffer>('font_preview', { postscript: face.postscript, sample, size, width, height });
    return new Blob([bytes], { type: 'image/svg+xml' });
  });
  cache.set(key, pending);
  while (cache.size > 24) cache.delete(cache.keys().next().value!);
  pending.catch(() => { if (cache.get(key) === pending) cache.delete(key); });
  return pending;
}
export interface FontPreferences { favorites: string[]; recent: string[]; groups: { id: string; name: string; fonts: string[] }[] }
export function readFontPreferences(): FontPreferences {
  try {
    const value = JSON.parse(readPreference('font-viewer-v1') ?? '{}');
    const names = (v: unknown) => Array.isArray(v) ? [...new Set(v.filter((s): s is string => typeof s === 'string' && s.length <= 200))].slice(0, 1000) : [];
    return { favorites: names(value.favorites), recent: names(value.recent).slice(0, 30), groups: Array.isArray(value.groups) ? value.groups.filter((g: Record<string, unknown>) => g && typeof g.id === 'string' && typeof g.name === 'string' && g.name.length <= 80).slice(0, 30).map((g: { id: string; name: string; fonts: unknown }) => ({ id: g.id, name: g.name, fonts: names(g.fonts) })) : [] };
  } catch { return { favorites: [], recent: [], groups: [] }; }
}
export function saveFontPreferences(value: FontPreferences) { savePreference('font-viewer-v1', JSON.stringify(value)); window.dispatchEvent(new Event('font-viewer-preferences')); }
export const fontViewerMessages = {
  ja: { title: 'フォントビューア', menu: '文字パネルのメニュー', character: '文字設定', search: 'フォント名で検索…', sample: 'プレビュー文字', previewSize: 'プレビューサイズ', listSize: 'リスト文字サイズ', nameMode: 'フォント名', previewMode: 'サンプル文字', all: 'すべて', japanese: '和文', latin: '欧文', favorites: 'お気に入り', recent: '最近使用', used: '文書で使用中', apply: '選択文字に適用', favorite: 'お気に入りを切り替え', loading: 'フォントを読み込み中…', empty: '該当するフォントはありません', unavailable: '書体プレビューを表示できません', fallback: '未収録の文字は代替フォントで表示します', group: 'お気に入りグループ', addGroup: 'グループを追加', removeGroup: 'グループを削除', groupName: 'グループ名', addToGroup: '選択書体を追加', removeFromGroup: '選択書体を除外', styles: '書体を開閉', defaultSample: '山路を登りながら、こう考えた。\nThe quick brown fox.', count: '書体', reset: 'サンプルをリセット' },
  en: { title: 'Font Viewer', menu: 'Character panel menu', character: 'Character settings', search: 'Search font names…', sample: 'Preview text', previewSize: 'Preview size', listSize: 'List text size', nameMode: 'Font names', previewMode: 'Sample text', all: 'All', japanese: 'Japanese', latin: 'Latin', favorites: 'Favorites', recent: 'Recent', used: 'Used in document', apply: 'Apply to selected text', favorite: 'Toggle favorite', loading: 'Loading fonts…', empty: 'No matching fonts', unavailable: 'Font preview unavailable', fallback: 'Missing characters use a fallback font', group: 'Favorite collection', addGroup: 'Add collection', removeGroup: 'Delete collection', groupName: 'Collection name', addToGroup: 'Add selected face', removeFromGroup: 'Remove selected face', styles: 'Expand font styles', defaultSample: 'The quick brown fox.\n山路を登りながら、こう考えた。', count: 'faces', reset: 'Reset sample' },
  'zh-CN': { title: '字体查看器', menu: '字符面板菜单', character: '字符设置', search: '搜索字体名称…', sample: '预览文字', previewSize: '预览字号', listSize: '列表字号', nameMode: '字体名称', previewMode: '示例文字', all: '全部', japanese: '日文', latin: '西文', favorites: '收藏', recent: '最近使用', used: '文档使用', apply: '应用到所选文字', favorite: '切换收藏', loading: '正在加载字体…', empty: '没有匹配的字体', unavailable: '无法显示字体预览', fallback: '缺失字符使用替代字体显示', group: '收藏分组', addGroup: '添加分组', removeGroup: '删除分组', groupName: '分组名称', addToGroup: '添加所选字体', removeFromGroup: '移除所选字体', styles: '展开字体样式', defaultSample: '山路を登りながら、こう考えた。\nThe quick brown fox.', count: '字体', reset: '重置示例' },
};
