import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Brush, ColorMode, ColorProfile } from './bridge';

export interface UsedColor { color: Brush['color']; alpha: number; colorMode: ColorMode; colorProfile: ColorProfile }
export const colorHistoryLabels = {
  ja: { history: '使用した色', apply: '描画色に設定', variants: '派生色', lighter: '明るく', darker: '暗く', muted: '彩度を下げる', vivid: '彩度を上げる', save: 'スウォッチに追加' },
  en: { history: 'Used colors', apply: 'Set foreground color', variants: 'Color variations', lighter: 'Lighter', darker: 'Darker', muted: 'Less saturated', vivid: 'More saturated', save: 'Add to swatches' },
  'zh-CN': { history: '使用过的颜色', apply: '设为前景色', variants: '派生颜色', lighter: '提亮', darker: '变暗', muted: '降低饱和度', vivid: '提高饱和度', save: '添加到色板' },
};
interface Usage { colorMode: ColorMode; colors: UsedColor[]; commit: (color: Brush['color'], alpha?: number) => void; resize: (capacity: number) => void }
const ColorUsage = createContext<Usage>({ colorMode: 'rgb', colors: [], commit: () => {}, resize: () => {} });
export const useColorUsage = () => useContext(ColorUsage);
/** This provider contains a projection only. The session history is owned by Rust. */
export function ColorUsageProvider({ children, colorMode, colorProfile, alpha = 1, onError }: { children: ReactNode; colorMode: ColorMode; colorProfile: ColorProfile; alpha?: number; onError: (error: string) => void }) {
  const [colors, setColors] = useState<UsedColor[]>([]);
  const sequence = useRef(0);
  const errorRef = useRef(onError); errorRef.current = onError;
  async function request(color?: UsedColor, capacity?: number) {
    if (!isTauri()) return;
    const id = ++sequence.current;
    try {
      const next = await invoke<UsedColor[]>('color_history', { color: color ?? null, capacity: capacity ?? null });
      if (id === sequence.current) setColors(next);
    } catch (error) { errorRef.current(String(error)); }
  }
  useEffect(() => {
    let live = true; let stop: (() => void) | undefined;
    if (!isTauri()) return;
    void listen('color-history-changed', () => { if (live) void request(); }).then(fn => { if (live) { stop = fn; void request(); } else fn(); }).catch(error => errorRef.current(String(error)));
    return () => { live = false; ++sequence.current; stop?.(); };
  }, []);
  return <ColorUsage.Provider value={{ colorMode, colors, commit: (color, nextAlpha = alpha) => { void request({ color, alpha: nextAlpha, colorMode, colorProfile }); }, resize: capacity => { void request(undefined, capacity); } }}>{children}</ColorUsage.Provider>;
}
