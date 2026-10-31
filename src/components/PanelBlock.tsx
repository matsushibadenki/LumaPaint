import { useState, type ReactNode } from 'react';
import type { Locale } from '../i18n';

const memory = new Map<string, boolean>();
const prefix = 'lumapaint.panel-block.v1.';
function restored(id: string, fallback: boolean) {
  if (memory.has(id)) return memory.get(id)!;
  try { const saved = localStorage.getItem(prefix + id); if (saved === 'open' || saved === 'closed') return saved === 'open'; } catch { /* View preferences remain usable without storage. */ }
  return fallback;
}
/** Native disclosure semantics keep controls mounted and hide them from keyboard navigation. */
export function PanelBlock({ id, title, children, defaultOpen = true, className = '' }: { id: string; title: ReactNode; children: ReactNode; defaultOpen?: boolean; className?: string }) {
  const [open, setOpen] = useState(() => restored(id, defaultOpen));
  const changeOpen = (next: boolean) => {
    memory.set(id, next); setOpen(next);
    try { localStorage.setItem(prefix + id, next ? 'open' : 'closed'); } catch { /* Session fallback above retains the choice. */ }
  };
  return <details className={`panel-block ${className}`} data-panel-block={id} open={open} onToggle={event => {
    const next = event.currentTarget.open;
    if (next === open) return;
    changeOpen(next);
  }}>
    <summary onClick={event => { event.preventDefault(); changeOpen(!open); }}>{title}</summary>
    <div className="panel-block-content">{children}</div>
  </details>;
}
export const panelBlockLabels: Record<Locale, Record<'basic'|'appearance'|'channels'|'wheel'|'palette'|'presets'|'settings'|'list'|'actions'|'geometry'|'options'|'timing'|'search'|'size'|'character'|'arrows', string>> = {
  ja: { basic:'基本設定', appearance:'表示と合成', channels:'カラー値', wheel:'カラーホイール', palette:'配色', presets:'プリセット', settings:'設定', list:'一覧', actions:'操作', geometry:'位置とサイズ', options:'オプション', timing:'タイミングと動き', search:'検索と表示', size:'サイズと解像度', character:'文字', arrows:'矢印と可変幅' },
  en: { basic:'Basics', appearance:'Appearance', channels:'Color values', wheel:'Color wheel', palette:'Palette', presets:'Presets', settings:'Settings', list:'Items', actions:'Actions', geometry:'Position and size', options:'Options', timing:'Timing and motion', search:'Search and display', size:'Size and resolution', character:'Character', arrows:'Arrows and width profile' },
  'zh-CN': { basic:'基本设置', appearance:'外观与混合', channels:'颜色数值', wheel:'色轮', palette:'配色', presets:'预设', settings:'设置', list:'列表', actions:'操作', geometry:'位置与尺寸', options:'选项', timing:'时间与运动', search:'搜索与显示', size:'尺寸与分辨率', character:'字符', arrows:'箭头与可变宽度' },
};
