import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Locale } from '../i18n';
import './edfx-editor.css';

type TextLocale = 'en' | 'ja' | 'zh-Hans';
type Paragraph = { locale: TextLocale; text: string };
type Page = { paragraphs: Paragraph[]; preview: string | null; placed: boolean };
type Edit = { type: 'open' | 'openPending' | 'loadPlaced' | 'save' | 'exportSvg' } |
  { type: 'new'; locale: TextLocale; face: number } |
  { type: 'reflow'; paragraphs: Paragraph[] };

const textLocales = [['en', 'English'], ['ja', '日本語'], ['zh-Hans', '简体中文']] as const;
const labels = {
  en: { place: 'Place on Canvas', apply: 'Update Canvas', applied: 'Canvas updated', open: 'Open EDFX…', create: 'New from Font…', face: 'Font face index', language: 'Language',
    paragraph: 'Paragraph', add: 'Add Paragraph', remove: 'Remove', update: 'Update Layout', save: 'Save EDFX…',
    export: 'Export SVG…', close: 'Close', discard: 'Discard Changes', preview: 'Page Preview',
    previewError: 'The page preview is unavailable.', error: 'EDFX operation failed',
    info: 'EDFX 0.1 supports one horizontal page. The embedded font must cover all text and permit editable embedding.',
    edited: 'Layout needs updating', unsaved: 'Unsaved EDFX changes', saved: 'EDFX saved', exported: 'SVG exported' },
  ja: { place: 'キャンバスに配置', apply: 'キャンバスを更新', applied: 'キャンバスを更新しました', open: 'EDFXを開く…', create: 'フォントから新規作成…', face: 'フォントのフェイス番号', language: '言語',
    paragraph: '段落', add: '段落を追加', remove: '削除', update: '版面を更新', save: 'EDFXを保存…',
    export: 'SVGを書き出す…', close: '閉じる', discard: '変更を破棄', preview: '版面プレビュー',
    previewError: '版面プレビューを表示できません。', error: 'EDFXの処理に失敗しました',
    info: 'EDFX 0.1は横書きの単ページに対応します。全文字を収録し、編集用の埋め込みが許可されたフォントを選んでください。',
    edited: '版面の更新が必要です', unsaved: '未保存のEDFX変更があります', saved: 'EDFXを保存しました', exported: 'SVGを書き出しました' },
  'zh-CN': { place: '置入画布', apply: '更新画布', applied: '画布已更新', open: '打开 EDFX…', create: '从字体新建…', face: '字体字面索引', language: '语言',
    paragraph: '段落', add: '添加段落', remove: '删除', update: '更新版面', save: '保存 EDFX…',
    export: '导出 SVG…', close: '关闭', discard: '放弃更改', preview: '版面预览',
    previewError: '无法显示版面预览。', error: 'EDFX 操作失败',
    info: 'EDFX 0.1 支持单页横排。请选用涵盖全部文字且允许可编辑嵌入的字体。',
    edited: '版面需要更新', unsaved: 'EDFX 有未保存的更改', saved: 'EDFX 已保存', exported: 'SVG 已导出' },
};

export function EdfxEditor({ locale, pendingFile, loadPlaced, canvasAvailable, onClose }: { locale: Locale; pendingFile: boolean; loadPlaced: boolean; canvasAvailable: boolean; onClose: () => void }) {
  const t = labels[locale];
  const [page, setPage] = useState<Page | null>(null);
  const [draft, setDraft] = useState<Paragraph[]>([]);
  const [saved, setSaved] = useState<Paragraph[] | null>(null);
  const [canvasVersion, setCanvasVersion] = useState<Paragraph[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [face, setFace] = useState('0');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const running = useRef(false);
  const textLocale = locale === 'zh-CN' ? 'zh-Hans' : locale;
  const differs = (paragraphs: Paragraph[] | null) => JSON.stringify(draft) !== JSON.stringify(paragraphs);
  const pending = !!page && differs(page.paragraphs);
  const dirty = !!page && differs(page.placed ? canvasVersion : saved);
  const faceIndex = Number(face);
  const validFace = face !== '' && Number.isInteger(faceIndex) && faceIndex >= 0 && faceIndex <= 0xffffffff;

  async function run(edit: Edit | { type: 'apply' }) {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setStatus('');
    setError('');
    try {
      await invoke('modal_busy', { busy: true });
      if (edit.type === 'apply') {
        await invoke('edfx_canvas_apply');
        setPage(current => current && { ...current, placed: true });
        setCanvasVersion(draft);
        setStatus(t.applied);
        return;
      }
      const next = await invoke<Page | null>('edfx_edit', { edit });
      if (!next) return;
      setPage(next);
      setDraft(next.paragraphs);
      if (edit.type === 'loadPlaced') setCanvasVersion(next.paragraphs);
      if (edit.type === 'open' || edit.type === 'openPending' || edit.type === 'save') setSaved(next.paragraphs);
      if (edit.type === 'new') setSaved(null);
      if (edit.type === 'save') setStatus(t.saved);
      if (edit.type === 'exportSvg') setStatus(t.exported);
    } catch (cause) {
      setError(String(cause));
    } finally {
      try {
        await invoke('modal_busy', { busy: false }).catch(() => {});
      } finally {
        running.current = false;
        setBusy(false);
      }
    }
  }
  const initialOpen = useRef(false);
  useEffect(() => {
    if ((pendingFile || loadPlaced) && !initialOpen.current) {
      initialOpen.current = true;
      void run({ type: loadPlaced ? 'loadPlaced' : 'openPending' });
    }
  }, [pendingFile, loadPlaced]);
  function confirmDiscard() {
    return !dirty || window.confirm(t.discard + '?');
  }
  const closeRequest = useRef<() => void>(() => {});
  closeRequest.current = () => { if (!running.current && confirmDiscard()) onClose(); };
  useEffect(() => {
    let active = true;
    const listener = getCurrentWebviewWindow().listen('edfx-close-request', () => {
      if (active) closeRequest.current();
    });
    return () => { active = false; void listener.then(unlisten => unlisten()).catch(() => {}); };
  }, []);
  function change(index: number, update: Partial<Paragraph>) {
    setDraft(items => items.map((item, i) => i === index ? { ...item, ...update } : item));
    setStatus('');
  }

  return <dialog open className="edfx-editor" aria-label="EDFX">
    <header className="edfx-toolbar">
      <button disabled={busy} onClick={() => { if (confirmDiscard()) void run({ type: 'open' }); }}>{t.open}</button>
      <label>{t.face}<input type="number" min={0} max={0xffffffff} step={1} value={face} disabled={busy} onChange={event => setFace(event.currentTarget.value)} /></label>
      <button disabled={busy || !validFace} onClick={() => { if (confirmDiscard()) void run({ type: 'new', face: faceIndex, locale: textLocale }); }}>{t.create}</button>
      <span tabIndex={0} title={t.info} aria-label={t.info}>ⓘ</span>
    </header>
    <div className="edfx-body">
      <div className="edfx-paragraphs">
        {draft.map((item, index) => <fieldset key={index} disabled={busy}>
          <legend>{t.paragraph} {index + 1}</legend>
          <div className="edfx-row">
            <select aria-label={t.language} value={item.locale} onChange={event => change(index, { locale: event.currentTarget.value as TextLocale })}>
              {textLocales.map(([value, title]) => <option key={value} value={value}>{title}</option>)}
            </select>
            <button aria-label={`${t.remove} ${t.paragraph} ${index + 1}`} disabled={draft.length <= 1} onClick={() => setDraft(items => items.filter((_, i) => i !== index))}>{t.remove}</button>
          </div>
          <textarea lang={item.locale} aria-label={`${t.paragraph} ${index + 1}`} rows={3} wrap="soft" value={item.text}
            onChange={event => change(index, { text: event.currentTarget.value.replace(/[\r\n]+/g, ' ') })} />
        </fieldset>)}
        {page && <button disabled={busy || draft.length >= 20} onClick={() => setDraft(items => [...items, { locale: textLocale, text: '' }])}>{t.add}</button>}
      </div>
      <div className="edfx-preview" aria-label={t.preview}>
        {page?.preview ? <img src={page.preview} alt={t.preview} width={300} height={420} /> : page && <p>{t.previewError}</p>}
      </div>
    </div>
    {error && <p role="alert" className="edfx-message">{t.error} · {error}</p>}
    <footer className="edfx-toolbar">
      <span role="status">{status || (pending ? t.edited : dirty ? t.unsaved : '')}</span>
      <button disabled={busy || !pending} onClick={() => void run({ type: 'reflow', paragraphs: draft })}>{t.update}</button>
      {canvasAvailable && <button disabled={busy || !page || pending || (page.placed && !dirty)}
        onClick={() => void run({ type: 'apply' })}>{page?.placed ? t.apply : t.place}</button>}
      <button disabled={busy || !page || pending} onClick={() => void run({ type: 'save' })}>{t.save}</button>
      <button disabled={busy || !page || pending} onClick={() => void run({ type: 'exportSvg' })}>{t.export}</button>
      <button disabled={busy} onClick={() => closeRequest.current()}>{t.close}</button>
    </footer>
  </dialog>;
}
