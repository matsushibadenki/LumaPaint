import { useState } from 'react';
import { pathfinderVectors, type DocumentSnapshot, type PathfinderOperation } from '../bridge';
import type { Locale } from '../i18n';
export const pathfinderLabels = {
  ja: { title: 'パスファインダー', modes: '形状モード：', paths: 'パスファインダー：', expand: '拡張', expandHint: '演算結果は編集可能なパスとして確定済みです', actions: ['合体', '前面オブジェクトで型抜き', '交差', '中マド', '分割', '刈り込み', '合流', '切り抜き', 'アウトライン', '背面オブジェクトで型抜き'] },
  en: { title: 'Pathfinder', modes: 'Shape Modes:', paths: 'Pathfinders:', expand: 'Expand', expandHint: 'Results are already editable paths', actions: ['Unite', 'Minus Front', 'Intersect', 'Exclude', 'Divide', 'Trim', 'Merge', 'Crop', 'Outline', 'Minus Back'] },
  'zh-CN': { title: '路径查找器', modes: '形状模式：', paths: '路径查找器：', expand: '扩展', expandHint: '结果已转换为可编辑路径', actions: ['联集', '减去顶层', '交集', '排除', '分割', '修边', '合并', '裁剪', '轮廓', '减去后层'] },
};
const operations: PathfinderOperation[] = ['unite', 'minusFront', 'intersect', 'exclude', 'divide', 'trim', 'merge', 'crop', 'outline', 'minusBack'];
function OperationIcon({ index }: { index: number }) {
  const masks = ['M4 4h11v5h5v11H9v-5H4Z','M4 4h11v5H9v6H4Z','M9 9h6v6H9Z','M4 4h11v5H9v6H4ZM15 9h5v11H9v-5h6Z'];
  return <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
    {index < 4 ? <><path d="M4 4h11v11H4ZM9 9h11v11H9Z" fill="none" stroke="currentColor" strokeWidth="1.4" opacity=".5"/><path d={masks[index]} fill="currentColor"/></> : <><path d="M4 4h11v11H4ZM9 9h11v11H9Z" fill={index === 5 ? 'currentColor' : 'none'} stroke={index === 6 ? 'none' : 'currentColor'} strokeWidth="1.4"/>{index === 6 && <path d={masks[0]} fill="currentColor"/>}{index === 4 && <path d="M9 4v16M4 9h16" stroke="currentColor"/>}{index === 7 && <path d="M9 9h6v6H9Z" fill="currentColor"/>}{index === 9 && <path d="M15 9h5v11H9v-5h6Z" fill="currentColor"/>}</>}
  </svg>;
}
export function PathfinderPanel({ locale, document, enabled, onUpdate }: { locale: Locale; document: DocumentSnapshot; enabled: boolean; onUpdate: (snapshot: DocumentSnapshot) => void }) {
  const t = pathfinderLabels[locale]; const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const available = enabled && !busy && !document.activeSavedPath && document.selectedVectorObjects.length >= 2 && document.selectedVectorObjects.length <= 64;
  async function apply(operation: PathfinderOperation) {
    if (!available) return;
    setBusy(true); setError('');
    try { onUpdate(await pathfinderVectors(operation)); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }
  const button = (index: number) => <button key={operations[index]} type="button" aria-label={t.actions[index]} title={t.actions[index]} disabled={!available} onClick={() => void apply(operations[index])}><OperationIcon index={index}/></button>;
  return <div className="pathfinder-panel" aria-busy={busy}>
    <header><h2>{t.title}</h2><span aria-hidden="true">≡</span></header>
    <div className="pathfinder-content"><p>{t.modes}</p><div className="pathfinder-modes">{[0,1,2,3].map(button)}<button className="pathfinder-expand" disabled title={t.expandHint}>{t.expand}</button></div>
    <p>{t.paths}</p><div className="pathfinder-operations">{[4,5,6,7,8,9].map(button)}</div>
    {error && <p className="pathfinder-error" role="alert">{error}</p>}</div>
  </div>;
}
