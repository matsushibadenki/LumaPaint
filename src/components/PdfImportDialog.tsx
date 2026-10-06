import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import type { Locale } from '../i18n';
type Info = { token: number; name: string; pages: { index: number; widthPoints: number; heightPoints: number }[]; asLayer: boolean };
export function PdfImportDialog({ locale, onClose, onApply }: { locale: Locale; onClose: () => void; onApply: (token: number, pageIndex: number, dpi: number, allPages: boolean, selectedPages: number[]) => Promise<boolean> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const unit = useMeasurementUnit();
  const [info, setInfo] = useState<Info | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [allPages, setAllPages] = useState(true);
  const [selected, setSelected] = useState<number[]>([0]);
  const [batch, setBatch] = useState(0);
  const [previews, setPreviews] = useState<Record<number, string | null>>({});
  const [dpi, setDpi] = useState('144');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const t = {
    ja: { title: 'PDF読み込み', all: '全ページを文書として開く', page: 'ページ', dpi: '解像度 (dpi)', cancel: 'キャンセル', open: '開く', import: '新しいレイヤーに読み込む', hint: 'サムネイルをクリックして複数ページを選択できます。レイヤーへの読み込みは指定ページのみです。解像度を変更しても用紙の実寸は保持されます。', limit: '解像度は36〜1200dpi、各辺8192px、一度に512ページまでです。', loading: 'ページ情報を確認中…' },
    en: { title: 'Import PDF', all: 'Open all pages as one document', page: 'Page', dpi: 'Resolution (dpi)', cancel: 'Cancel', open: 'Open', import: 'Import as New Layer', hint: 'Click thumbnails to select multiple pages. Layer import uses one selected page. Physical page dimensions are preserved when changing resolution.', limit: 'Use 36–1200 dpi, up to 8192 px per side and 512 pages per import.', loading: 'Reading page information…' },
    'zh-CN': { title: '导入PDF', all: '将所有页面作为一个文档打开', page: '页面', dpi: '分辨率 (dpi)', cancel: '取消', open: '打开', import: '导入为新图层', hint: '点击缩略图可选择多个页面。图层导入仅使用所选页面。更改分辨率时保留页面的实际尺寸。', limit: '分辨率为36–1200dpi，每边最多8192像素，每次最多导入512页。', loading: '正在读取页面信息…' },
  }[locale];
  useEffect(() => {
    dialog.current?.showModal();
    let active = true;
    void invoke<Info>('pdf_import_context').then(value => { if (active) setInfo(value); }).catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    if (!info || busy) return;
    let active = true;
    setPreviews({});
    // One bounded worker: only the visible group is decoded, never the full PDF.
    void (async () => {
      for (const page of info.pages.slice(batch * 12, batch * 12 + 12)) {
        if (!active) break;
        let preview: string | null = null;
        try { preview = await invoke<string>('pdf_import_thumbnail', { token: info.token, pageIndex: page.index }); } catch { /* Selection remains available when preview conversion fails. */ }
        if (active) setPreviews(current => ({ ...current, [page.index]: preview }));
      }
    })();
    return () => { active = false; };
  }, [info, batch, busy]);
  const page = info?.pages[pageIndex];
  const resolution = Number(dpi);
  const width = page ? Math.ceil(page.widthPoints * resolution / 72) : 0;
  const height = page ? Math.ceil(page.heightPoints * resolution / 72) : 0;
  const displayDpi = resolution >= 36 && resolution <= 1200 ? resolution : 144;
  const pointToUnit = displayDpi / 72 / pixelsPerMeasurement(unit, displayDpi);
  const importAll = allPages && !info?.asLayer;
  const selectedPages = importAll ? info?.pages : info?.pages.filter(p => selected.includes(p.index));
  const valid = !!page && Number.isInteger(resolution) && resolution >= 36 && resolution <= 1200 && !!selectedPages?.length && selectedPages.length <= 512 && selectedPages.every(p => Math.ceil(p.widthPoints * resolution / 72) <= 8192 && Math.ceil(p.heightPoints * resolution / 72) <= 8192);
  return <dialog ref={dialog} className="transform-dialog pdf-import-dialog" aria-labelledby="pdf-import-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <form onSubmit={async event => { event.preventDefault(); if (!info || !valid || busy) return; setBusy(true); setError(''); try { if (await onApply(info.token, pageIndex, resolution, importAll, selected)) onClose(); } catch (cause) { setError(String(cause)); } finally { setBusy(false); } }}>
      <h3 id="pdf-import-title">{t.title}</h3>
      {info ? <>
        <p style={{ overflowWrap: 'anywhere' }}>{info.name}</p>
        {!info.asLayer && <label><input type="checkbox" checked={allPages} disabled={busy} onChange={event => setAllPages(event.target.checked)} />{t.all}</label>}
        <div className="pdf-page-grid" role="group" aria-label={t.page}>
          {info.pages.slice(batch * 12, batch * 12 + 12).map(p => <button type="button" key={p.index} className="pdf-page-card" aria-pressed={importAll || selected.includes(p.index)} disabled={busy} onClick={() => {
            setAllPages(false); setPageIndex(p.index);
            setSelected(current => info.asLayer ? [p.index] : importAll ? [p.index] : current.includes(p.index) ? current.filter(i => i !== p.index) : [...current, p.index].sort((a, b) => a - b));
          }}>
            <span className="pdf-page-preview">{previews[p.index] ? <img src={previews[p.index]!} alt={`${t.page} ${p.index + 1}`} /> : <span>{previews[p.index] === null ? '—' : '…'}</span>}</span>
            <span>{t.page} {p.index + 1}</span>
            <small>{(p.widthPoints * pointToUnit).toFixed(1)} × {(p.heightPoints * pointToUnit).toFixed(1)} {unitSymbols[unit]}</small>
          </button>)}
        </div>
        <div className="pdf-page-navigation"><button type="button" disabled={busy || batch === 0} onClick={() => setBatch(b => b - 1)} aria-label={locale === 'ja' ? '前のページ一覧' : locale === 'en' ? 'Previous pages' : '上一组页面'}>‹</button><span>{batch * 12 + 1}–{Math.min((batch + 1) * 12, info.pages.length)} / {info.pages.length} · {importAll ? info.pages.length : selected.length} {t.page}</span><button type="button" disabled={busy || (batch + 1) * 12 >= info.pages.length} onClick={() => setBatch(b => b + 1)} aria-label={locale === 'ja' ? '次のページ一覧' : locale === 'en' ? 'Next pages' : '下一组页面'}>›</button></div>
        <label style={{ display: 'grid', gap: 8, marginTop: 12 }}>{t.dpi}<input type="number" min="36" max="1200" step="1" required disabled={busy} value={dpi} onChange={event => setDpi(event.target.value)} /></label>
        {valid && <p>{width} × {height} px</p>}
        <p>{t.hint}</p><p role={!valid ? 'alert' : undefined}>{t.limit}</p>
      </> : <p>{t.loading}</p>}
      {error && <p role="alert">{error}</p>}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, justifyContent: 'flex-end' }}><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button disabled={busy || !valid}>{info?.asLayer ? t.import : t.open}</button></div>
    </form>
  </dialog>;
}
