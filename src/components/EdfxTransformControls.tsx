import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Locale } from '../i18n';

type Mode = 'move' | 'scale';
const labels = {
  en: { move: 'Move EDFX', scale: 'Scale EDFX', moveHint: 'Drag on the canvas to move the selected page.', scaleHint: 'Drag right or up to enlarge, left or down to reduce. Scaling is anchored at the page origin.' },
  ja: { move: 'EDFXを移動', scale: 'EDFXを拡大・縮小', moveHint: 'キャンバスをドラッグして選択した版面を移動します。', scaleHint: '右か上へドラッグすると拡大し、左か下へドラッグすると縮小します。版面の原点が基準です。' },
  'zh-CN': { move: '移动 EDFX', scale: '缩放 EDFX', moveHint: '在画布上拖动以移动选中的版面。', scaleHint: '向右或向上拖动可放大，向左或向下拖动可缩小。缩放以版面原点为基准。' },
};

export function EdfxTransformControls({ locale, onError }: { locale: Locale; onError: (message: string) => void }) {
  const [mode, setMode] = useState<Mode | null>(null);
  const [busy, setBusy] = useState(false);
  const t = labels[locale];

  useEffect(() => () => { void invoke('edfx_transform_mode', { mode: null }).catch(() => {}); }, []);

  async function toggle(requested: Mode) {
    const next = mode === requested ? null : requested;
    setBusy(true);
    try {
      await invoke('edfx_transform_mode', { mode: next });
      setMode(next);
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy(false);
    }
  }

  return <div className="image-placement-controls edfx-transform-controls" role="group" aria-label="EDFX">
    {(['move', 'scale'] as const).map(option =>
      <button key={option} disabled={busy} aria-pressed={mode === option}
        title={option === 'move' ? t.moveHint : t.scaleHint}
        onClick={() => void toggle(option)}>{t[option]}</button>
    )}
  </div>;
}
