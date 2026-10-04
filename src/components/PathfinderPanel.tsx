import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import { useState } from 'react';
import { editCompoundShape, makeCompoundShape, pathfinderVectors, type CompoundShapeEdit, type DocumentSnapshot, type PathfinderOperation } from '../bridge';
import type { Locale } from '../i18n';
export const pathfinderLabels = {
  ja: { title: 'パスファインダー', modes: '形状モード：', paths: 'パスファインダー：', expand: '拡張', expandHint: '元の形状を破棄して通常のパスに確定', live:'複合形状を作成', liveLayer:'複合形状は同じレイヤー内で作成できます。別レイヤーの図形を演算する場合はチェックを外してください。', release:'複合形状を解除', operands:'元の形状（移動量）', actions: ['合体', '前面オブジェクトで型抜き', '交差', '中マド', '分割', '刈り込み', '合流', '切り抜き', 'アウトライン', '背面オブジェクトで型抜き'] },
  en: { title: 'Pathfinder', modes: 'Shape Modes:', paths: 'Pathfinders:', expand: 'Expand', expandHint: 'Convert to an ordinary path and discard operands', live:'Create compound shape', liveLayer:'Compound shapes require one layer. Uncheck this option to combine objects across layers.', release:'Release compound shape', operands:'Operands (translation)', actions: ['Unite', 'Minus Front', 'Intersect', 'Exclude', 'Divide', 'Trim', 'Merge', 'Crop', 'Outline', 'Minus Back'] },
  'zh-CN': { title: '路径查找器', modes: '形状模式：', paths: '路径查找器：', expand: '扩展', expandHint: '转换为普通路径并丢弃原始形状', live:'创建复合形状', liveLayer:'复合形状需要位于同一图层。取消勾选可对不同图层的对象进行运算。', release:'释放复合形状', operands:'原始形状（位移）', actions: ['联集', '减去顶层', '交集', '排除', '分割', '修边', '合并', '裁剪', '轮廓', '减去后层'] },
};
const operations: PathfinderOperation[] = ['unite', 'minusFront', 'intersect', 'exclude', 'divide', 'trim', 'merge', 'crop', 'outline', 'minusBack'];
function OperationIcon({ index }: { index: number }) {
  const masks = ['M4 4h11v5h5v11H9v-5H4Z','M4 4h11v5H9v6H4Z','M9 9h6v6H9Z','M4 4h11v5H9v6H4ZM15 9h5v11H9v-5h6Z'];
  return <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
    {index < 4 ? <><path d="M4 4h11v11H4ZM9 9h11v11H9Z" fill="none" stroke="currentColor" strokeWidth="1.4" opacity=".5"/><path d={masks[index]} fill="currentColor"/></> : <><path d="M4 4h11v11H4ZM9 9h11v11H9Z" fill={index === 5 ? 'currentColor' : 'none'} stroke={index === 6 ? 'none' : 'currentColor'} strokeWidth="1.4"/>{index === 6 && <path d={masks[0]} fill="currentColor"/>}{index === 4 && <path d="M9 4v16M4 9h16" stroke="currentColor"/>}{index === 7 && <path d="M9 9h6v6H9Z" fill="currentColor"/>}{index === 9 && <path d="M15 9h5v11H9v-5h6Z" fill="currentColor"/>}</>}
  </svg>;
}
export function PathfinderPanel({ locale, document, enabled, onUpdate }: { locale: Locale; document: DocumentSnapshot; enabled: boolean; onUpdate: (snapshot: DocumentSnapshot) => void }) {
  const unit=useMeasurementUnit(); const factor=pixelsPerMeasurement(unit,document.resolution);
  const t = pathfinderLabels[locale]; const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const [live, setLive] = useState(true);
  const selected = document.selectedVectorObjects;
  const recipe = selected.length === 1 ? document.compoundShapes?.find(s => s.id === selected[0]) : undefined;
  const editable = enabled && !busy && !document.activeSavedPath;
  const sameLayer = document.layers.some(layer => selected.every(id => layer.objects.some(o => o.id === id)));
  const available = editable && selected.length >= 2 && selected.length <= 64;
  async function run(work: () => Promise<DocumentSnapshot>) {
    if (!editable) return;
    setBusy(true); setError('');
    try { onUpdate(await work()); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }
  const edit = (change: CompoundShapeEdit) => void run(() => editCompoundShape(change));
  const button = (index: number) => <button key={operations[index]} type="button" aria-label={t.actions[index]} title={t.actions[index]} aria-pressed={index < 4 && recipe?.operation === operations[index]} disabled={!(available || (editable && recipe && index < 4)) || (index < 4 && live && !sameLayer && !recipe)} onClick={() => void run(() => recipe && index < 4 ? editCompoundShape({action:'update',operation:operations[index]}) : index < 4 && live && sameLayer ? makeCompoundShape(operations[index]) : pathfinderVectors(operations[index]))}><OperationIcon index={index}/></button>;
  return <div className="pathfinder-panel" aria-busy={busy}>
    <header><h2>{t.title}</h2><span aria-hidden="true">≡</span></header>
    <div className="pathfinder-content"><p>{t.modes}</p><div className="pathfinder-modes">{[0,1,2,3].map(button)}<button className="pathfinder-expand" disabled={!editable || !recipe} title={t.expandHint} onClick={() => edit({action:'expand'})}>{t.expand}</button></div>
    <label className="pathfinder-live"><input type="checkbox" checked={live} onChange={event => setLive(event.target.checked)}/>{t.live}</label>
    {live && !sameLayer && selected.length >= 2 && <p>{t.liveLayer}</p>}<p>{t.paths}</p><div className="pathfinder-operations">{[4,5,6,7,8,9].map(button)}</div>
    {recipe && <div className="pathfinder-operands"><p>{t.operands} · {unitSymbols[unit]}</p>{recipe.operands.map(o => <div key={o.id}><span title={o.name}>{o.name}</span>{([4,5] as const).map((axis,index) => <label key={axis}>{index === 0 ? 'X' : 'Y'}<input key={`${o.id}-${axis}-${o.transform[axis]}-${unit}`}  type="number" aria-label={`${o.name} ${index === 0 ? 'X' : 'Y'}`} defaultValue={Number((o.transform[axis]/factor).toFixed(3))} disabled={!editable} onBlur={event => {const value=Number(event.currentTarget.value); if (event.currentTarget.value.trim() && Number.isFinite(value) && value !== Number((o.transform[axis]/factor).toFixed(3))) {const translation:[number,number]=[o.transform[4],o.transform[5]];translation[index]=value*factor;edit({action:'update',operand:o.id,translation});}}} onKeyDown={event => {if(event.key==='Enter')event.currentTarget.blur();if(event.key==='Escape'){event.currentTarget.value=String(Number((o.transform[axis]/factor).toFixed(3)));event.currentTarget.blur();}}}/></label>)}</div>)}<button disabled={!editable} onClick={() => edit({action:'release'})}>{t.release}</button></div>}
    {error && <p className="pathfinder-error" role="alert">{error}</p>}</div>
  </div>;
}
