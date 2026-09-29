import { useState } from 'react';
import type { DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';

export type SavedPathAction = 'new' | 'activate' | 'deactivate' | 'create' | 'update' | 'rename' | 'delete' | 'clip' | 'release';
const labels = {
  ja: { title: '保存パス', hint: '新規パスを作成し、選択中のパスにペン・図形ツールで描画します。クリッピング指定はドキュメント全体に適用されます。', empty: '保存パスはありません', name: 'パス名', create: '新規パス', update: '選択から更新', rename: '名前を変更', clip: 'クリッピングパスに指定', release: 'クリッピングを解除', remove: 'パスを削除', active: 'クリッピングパス', path: 'パス' },
  en: { title: 'Saved paths', hint: 'Create and select a path, then draw with pen or shape tools. Clipping applies to the entire document.', empty: 'No saved paths', name: 'Path name', create: 'New path', update: 'Update from selection', rename: 'Rename', clip: 'Set clipping path', release: 'Release clipping', remove: 'Delete path', active: 'Clipping path', path: 'Path' },
  'zh-CN': { title: '保存的路径', hint: '新建并选择路径，然后使用钢笔或形状工具绘制。剪切路径应用于整个文档。', empty: '没有保存的路径', name: '路径名称', create: '新建路径', update: '从所选路径更新', rename: '重命名', clip: '设为剪切路径', release: '解除剪切', remove: '删除路径', active: '剪切路径', path: '路径' },
};
export function PathsPanel({ document, locale, enabled, onAction }: { document: DocumentSnapshot; locale: Locale; enabled: boolean; onAction: (action: SavedPathAction, id: string | null, name: string) => Promise<void> }) {
  const t = labels[locale];
  const paths = document.savedPaths ?? [];
  const id = document.activeSavedPath;
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const selected = paths.find(path => path.id === id);
  const run = async (action: SavedPathAction, target: string | null = selected?.id ?? null) => {
    setBusy(true); setError('');
    try { await onAction(action, target, name.trim() || `${t.path} ${paths.length + 1}`); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  return <section className="paths-panel" aria-label={t.title}>
    <p className="paths-hint">{t.hint}</p>
    <div className="saved-path-list" role="listbox" aria-label={t.title}>
      {paths.length === 0 && <p className="paths-hint">{t.empty}</p>}
      {paths.map(path => <button key={path.id} type="button" role="option" aria-selected={id === path.id} disabled={!enabled || busy} onClick={() => { setName(path.name); void run('activate', path.id); }}>
        <span className="path-color-stripe" aria-hidden="true" style={{ backgroundColor: `rgb(${(path.guideColor ?? [48,144,255]).slice(0,3).join(',')})` }} />
        <svg viewBox="0 0 32 32" aria-hidden="true"><path d="M6 24V8H26V24Z" fill="none" stroke="currentColor"/><path d="M4 6H8V10H4ZM24 22H28V26H24Z" fill="currentColor"/></svg>
        <span>{path.name}{path.clipping && <small>{t.active}</small>}</span>
      </button>)}
    </div>
    <label>{t.name}<input value={name} maxLength={120} disabled={!enabled || busy} onChange={event => setName(event.target.value)} /></label>
    <div className="paths-actions">
      <button disabled={!enabled || busy} onClick={() => void run('new')}>{t.create}</button>
      <button disabled={!enabled || busy || !selected || !name.trim()} onClick={() => void run('rename')}>{t.rename}</button>
      <button disabled={!enabled || busy || !selected || selected.clipping} onClick={() => void run('clip')}>{t.clip}</button>
      <button disabled={!enabled || busy || !paths.some(path => path.clipping)} onClick={() => void run('release')}>{t.release}</button>
      <button disabled={!enabled || busy || !selected} onClick={() => void run('delete')}>{t.remove}</button>
    </div>
    {error && <p role="alert">{error}</p>}
  </section>;
}
