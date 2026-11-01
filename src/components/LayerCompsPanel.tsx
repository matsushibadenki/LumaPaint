import { useEffect, useRef, useState } from 'react';
import { editLayerGroups, type DocumentSnapshot, type LayerCompSummary, type LayerGroupEdit } from '../bridge';
import type { Locale } from '../i18n';
import { PanelBlock } from './PanelBlock';
import { Icon } from './Icon';

export const layerCompLabels = {
  ja: { title:'レイヤーカンプ', hint:'現在のページの表示状態とレイヤーの不透明度を保存します。グループのマスクと位置は保存しません。追加したレイヤーは変更しません。以前のカンプは表示状態のみ適用し、更新すると不透明度も保存します。Δは変更レイヤー数。ホバーで現在→カンプの差分を表示します。', name:'カンプ名', base:'カンプ', create:'表示・不透明度を保存', update:'現在の表示・不透明度で更新', rename:'カンプ名を変更', remove:'カンプを削除', apply:'表示・不透明度を適用', matches:'保存した設定と一致', empty:'カンプはありません', error:'カンプを変更できませんでした', limit:'カンプは32件まで保存できます', difference:'現在 → カンプ', changed:'変更レイヤー', visibility:'表示', opacity:'不透明度', uncaptured:'未記録（適用時も維持）', visible:'表示', hidden:'非表示', remaining:'ほかの変更レイヤー' },
  en: { title:'Layer comps', hint:'Save visibility and layer opacity for the current page. Group masks and position are not captured. Newly added layers stay unchanged. Older comps apply visibility only; update them to capture opacity too. Δ counts changed layers; hover for differences from current state to the comp.', name:'Comp name', base:'Comp', create:'Save visibility and opacity', update:'Update visibility and opacity', rename:'Rename comp', remove:'Delete comp', apply:'Apply visibility and opacity', matches:'Matches saved settings', empty:'No layer comps', error:'Could not change the layer comp', limit:'Up to 32 comps can be saved', difference:'Current → comp', changed:'Changed layers', visibility:'Visibility', opacity:'Opacity', uncaptured:'Not captured (kept on apply)', visible:'Visible', hidden:'Hidden', remaining:'Other changed layers' },
  'zh-CN': { title:'图层复合', hint:'保存当前页面的可见性和图层不透明度。不保存组蒙版或位置。新增图层保持不变。旧复合仅应用可见性；更新后也会保存不透明度。Δ表示更改的图层数；悬停可查看从当前状态到复合的差异。', name:'复合名称', base:'复合', create:'保存可见性和不透明度', update:'更新可见性和不透明度', rename:'重命名复合', remove:'删除复合', apply:'应用可见性和不透明度', matches:'与保存的设置一致', empty:'没有图层复合', error:'无法更改图层复合', limit:'最多可保存32个复合', difference:'当前 → 复合', changed:'已更改图层', visibility:'可见性', opacity:'不透明度', uncaptured:'未记录（应用时保留）', visible:'可见', hidden:'隐藏', remaining:'其他更改的图层' },
};

const layerCompPercentFormats = {
  ja: new Intl.NumberFormat('ja', { maximumFractionDigits: 3, style: 'percent' }),
  en: new Intl.NumberFormat('en', { maximumFractionDigits: 3, style: 'percent' }),
  'zh-CN': new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3, style: 'percent' }),
};

export function layerCompDifferenceText(comp: LayerCompSummary, locale: Locale): string {
  const t = layerCompLabels[locale];
  const diff = comp.difference;
  if (!diff) return comp.matches ? t.matches : t.difference;
  const percent = layerCompPercentFormats[locale];
  const visible = (value: boolean | null) => value === null ? '—' : value ? t.visible : t.hidden;
  const alpha = (value: number | null) => value === null ? '—' : percent.format(value);
  const lines = [t.difference, `${t.changed}: ${diff.layerCount} · ${t.visibility}: ${diff.visibilityCount} · ${t.opacity}: ${diff.opacityCount}`];
  for (const detail of diff.details) {
    const changes: string[] = [];
    if (detail.visibility) changes.push(`${t.visibility}: ${visible(detail.visibility[0])} → ${visible(detail.visibility[1])}`);
    if (detail.opacity) changes.push(`${t.opacity}: ${alpha(detail.opacity[0])} → ${alpha(detail.opacity[1])}`);
    lines.push(`${detail.name}: ${changes.join(' · ')}`);
  }
  const remaining = diff.layerCount - diff.details.length;
  if (remaining > 0) lines.push(`${t.remaining}: ${remaining}`);
  if (diff.uncapturedCount > 0) lines.push(`${t.uncaptured}: ${diff.uncapturedCount}`);
  if (comp.matches) lines.push(t.matches);
  return lines.join('\n');
}

export function LayerCompsPanel({ comps, locale, enabled, onUpdate }: { comps: LayerCompSummary[]; locale: Locale; enabled: boolean; onUpdate: (snapshot: DocumentSnapshot) => void }) {
  const t = layerCompLabels[locale];
  const [selected, setSelected] = useState('');
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const live = useRef(true), pending = useRef(false);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  const target = comps.find(comp => comp.id === selected);
  const disabled = !enabled || busy;
  async function run(action: LayerGroupEdit['action'], id = selected) {
    if (disabled || pending.current) return;
    pending.current = true; setBusy(true); setError('');
    try {
      const next = await editLayerGroups({ action, id, name: name.trim() || `${t.base} ${comps.length + 1}` });
      if (!live.current) return;
      onUpdate(next);
      if (action === 'compCreate') {
        const created = next.layerGroups.comps?.at(-1);
        if (created) { setSelected(created.id); setName(created.name); }
      } else if (action === 'compDelete') { setSelected(''); setName(''); }
    } catch (cause) { if (live.current) setError(String(cause)); }
    finally { pending.current = false; if (live.current) setBusy(false); }
  }
  return <PanelBlock id="layers.comps" title={<>{t.title}<span className="panel-info" title={t.hint} aria-label={t.hint}>ⓘ</span></>}>
    <div className="layer-comps" aria-busy={busy}>
      <div className="layer-comp-list" role="group" aria-label={t.title}>
        {!comps.length && <span className="layer-comp-empty">{t.empty}</span>}
        {comps.map(comp => {
          const comparison = layerCompDifferenceText(comp, locale);
          return <button key={comp.id} type="button" disabled={disabled} className={selected === comp.id ? 'active' : ''} aria-pressed={selected === comp.id} title={`${t.apply}: ${comp.name}\n${comparison}`} onClick={() => { setSelected(comp.id); setName(comp.name); void run('compApply', comp.id); }}>
            <Icon name="layers"/><span>{comp.name}</span><span className={`layer-comp-match${comp.matches ? '' : ' layer-comp-difference'}`} role="img" title={comparison} aria-label={comparison}>{comp.matches ? '✓' : comp.difference ? `Δ ${comp.difference.layerCount}` : 'Δ'}</span>
          </button>;
        })}
      </div>
      <div className="layer-comp-edit">
        <input aria-label={t.name} placeholder={t.name} value={name} maxLength={120} disabled={disabled} onChange={event => setName(event.target.value)} onKeyDown={event => { event.stopPropagation(); if (event.key === 'Enter' && !event.nativeEvent.isComposing && target && name.trim()) { event.preventDefault(); void run('compRename'); } }}/>
        <button type="button" title={comps.length >= 32 ? t.limit : t.create} aria-label={t.create} disabled={disabled || comps.length >= 32} onClick={() => void run('compCreate')}><Icon name="plus"/></button>
      </div>
      <div className="layer-comp-actions">
        <button type="button" title={t.update} aria-label={t.update} disabled={disabled || !target} onClick={() => void run('compUpdate')}>↻</button>
        <button type="button" title={t.rename} aria-label={t.rename} disabled={disabled || !target || !name.trim() || name.trim() === target.name} onClick={() => void run('compRename')}>✎</button>
        <button type="button" title={t.remove} aria-label={t.remove} disabled={disabled || !target} onClick={() => void run('compDelete')}>⌫</button>
      </div>
      {error && <p role="alert" title={error}>{t.error}</p>}
    </div>
  </PanelBlock>;
}
