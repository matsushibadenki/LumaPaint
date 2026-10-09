import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useRef, useState } from 'react';
import { editLayerGroups, setLayerEffects, updateLayer, type DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { LayerMaskControls } from './LayerMaskControls';
import './layer-mask-dialog.css';

export const layerMaskDialogLabels = {
  ja: { title: 'レイヤーマスク', close: '閉じる', applied: '変更はすぐに適用されます。', enabled: 'マスクを有効にする', legacy: '既存の濃度マスク', coverage: 'マスク被覆率', missing: '対象のレイヤーがありません。' },
  en: { title: 'Layer mask', close: 'Close', applied: 'Changes apply immediately.', enabled: 'Use layer mask', legacy: 'Existing density mask', coverage: 'Mask coverage', missing: 'The target layer is no longer available.' },
  'zh-CN': { title: '图层蒙版', close: '关闭', applied: '更改会立即生效。', enabled: '启用图层蒙版', legacy: '现有浓度蒙版', coverage: '蒙版覆盖率', missing: '目标图层已不可用。' },
};

export function LayerMaskDialog({ locale, document: snapshot, targetId, onClose, onUpdate, onBusyChange }: {
  locale: Locale; document: DocumentSnapshot; targetId: string; onClose: () => void;
  onUpdate: (snapshot: DocumentSnapshot) => void; onBusyChange?: (busy: boolean) => void | Promise<void>;
}) {
  const t = layerMaskDialogLabels[locale], common = workspaceMessages[locale];
  const dialog = useRef<HTMLDialogElement>(null);
  const running = useRef(false);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const group = snapshot.layerGroups.groups.find(item => item.id === targetId);
  const layer = snapshot.layers.find(item => item.id === targetId);
  const target = group ?? layer;
  const tiled = !!layer?.rasterBlendMode;
  useEffect(() => {
    if (isTauri()) return;
    const element = dialog.current;
    const previous = window.document.activeElement;
    element?.showModal();
    return () => { element?.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, []);
  async function changeBusy(value: boolean) {
    running.current = value; setBusy(value);
    await onBusyChange?.(value);
  }
  function close() { if (!running.current) onClose(); }
  async function changeLegacy(patch: Partial<{ maskEnabled: boolean; maskInverted: boolean; maskDensity: number }>) {
    if (running.current || !target || target.locked) return;
    setError('');
    try {
      await changeBusy(true);
      if (group) {
        onUpdate(await editLayerGroups({ action: 'mask', id: group.id, mask: {
          enabled: patch.maskEnabled ?? group.maskEnabled,
          inverted: patch.maskInverted ?? group.maskInverted,
          density: patch.maskDensity ?? group.maskDensity,
        } }));
      } else if (layer) {
        onUpdate(await updateLayer({ id: layer.id, name: layer.name, opacity: layer.opacity, locked: layer.locked,
          alphaLocked: layer.alphaLocked, maskEnabled: layer.maskEnabled, maskInverted: layer.maskInverted,
          maskDensity: layer.maskDensity, ...patch }));
      }
    } catch (cause) { setError(String(cause)); }
    finally { try { await changeBusy(false); } catch (cause) { setError(String(cause)); } }
  }
  const legacyControls = target && <fieldset className="legacy-mask-settings" disabled={busy || target.locked}>
    <label className="mask-enabled-toggle"><input type="checkbox" checked={target.maskEnabled} onChange={e => void changeLegacy({ maskEnabled: e.currentTarget.checked })}/>{t.enabled}</label>
    <label><span>{tiled ? t.coverage : common.maskDensity} (%)</span><input type="number" min={0} max={100} disabled={!target.maskEnabled} value={Math.round(target.maskDensity * 100)} onChange={e => {
      const value = e.currentTarget.valueAsNumber;
      if (Number.isFinite(value) && value >= 0 && value <= 100) void changeLegacy({ maskDensity: value / 100 });
    }}/></label>
    <label className="mask-enabled-toggle"><input type="checkbox" disabled={!target.maskEnabled} checked={target.maskInverted} onChange={e => void changeLegacy({ maskInverted: e.currentTarget.checked })}/>{common.invertMask}</label>
  </fieldset>;
  return <dialog ref={dialog} open={isTauri()} className="layer-mask-dialog" lang={locale} aria-labelledby="layer-mask-title" aria-describedby="layer-mask-target" onKeyDown={e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();close();}}} onCancel={e => { e.preventDefault(); close(); }}>
    <form onSubmit={e => e.preventDefault()} aria-busy={busy}>
      <header><h2 id="layer-mask-title">{t.title}</h2><p id="layer-mask-target">{target?.name ?? t.missing}</p></header>
      <div className="layer-mask-dialog-body">
        {layer?.effects && <LayerMaskControls key={targetId} locale={locale} layer={layer} enabled={!busy} onBusyChange={changeBusy} onCommit={async (id, effects) => onUpdate(await setLayerEffects(id, effects))} onSnapshot={onUpdate}/>}
        {group ? legacyControls : target && (target.maskEnabled || tiled) ? <details className="legacy-mask-section"><summary>{t.legacy}</summary>{legacyControls}</details> : null}
        {error && <p role="alert">{error}</p>}
      </div>
      <footer><p>{t.applied}</p><button type="button" disabled={busy} onClick={close}>{t.close}</button></footer>
    </form>
  </dialog>;
}
