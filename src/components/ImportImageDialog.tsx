import { useEffect, useRef, useState } from 'react';
import type { Locale } from '../i18n';

export function ImportImageDialog({ locale, onImport, onClose }: {
  locale: Locale; onImport: (format: 'all' | 'jpeg' | 'png') => Promise<void>; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [format, setFormat] = useState<'all' | 'jpeg' | 'png'>('all');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const t = {
    ja: { title: '読み込み', format: 'ファイル形式', all: 'JPEG・PNG', choose: 'ファイルを選択…', cancel: 'キャンセル', hint: '中央に仮配置し、移動・拡大縮小・回転後に確定すると新しいピクセルレイヤーになります。', limit: '各辺8192px・約3MiBまで（現在の保存形式の上限）' },
    en: { title: 'Import', format: 'File format', all: 'JPEG and PNG', choose: 'Choose File…', cancel: 'Cancel', hint: 'Place at the center, then move, resize or rotate. Confirm to create a new pixel layer.', limit: 'Up to 8192px per side and about 3 MiB (current storage limit)' },
    'zh-CN': { title: '导入', format: '文件格式', all: 'JPEG和PNG', choose: '选择文件…', cancel: '取消', hint: '居中临时放置，可移动、缩放或旋转。确认后创建新的像素图层。', limit: '每边最多8192像素、约3MiB（当前存储限制）' },
  }[locale];
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="transform-dialog" aria-labelledby="import-image-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <form onSubmit={async event => { event.preventDefault(); setBusy(true); setError(''); try { await onImport(format); onClose(); } catch (cause) { setError(String(cause)); } finally { setBusy(false); } }}>
      <h3 id="import-image-title">{t.title}</h3>
      <label style={{ display: 'grid', gap: 8 }}>{t.format}
        <select autoFocus disabled={busy} value={format} onChange={event => setFormat(event.target.value as typeof format)}>
          <option value="all">{t.all}</option><option value="jpeg">JPEG (.jpg, .jpeg)</option><option value="png">PNG (.png)</option>
        </select>
      </label>
      <p>{t.hint}</p><p>{t.limit}</p>
      {error && <p role="alert">{error}</p>}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button>
        <button disabled={busy}>{t.choose}</button>
      </div>
    </form>
  </dialog>;
}
