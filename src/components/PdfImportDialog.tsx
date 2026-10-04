import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import type { Locale } from '../i18n';
type Info = { token: number; name: string; pages: { index: number; widthPoints: number; heightPoints: number }[]; asLayer: boolean };
export function PdfImportDialog({ locale, onClose, onApply }: { locale: Locale; onClose: () => void; onApply: (token: number, pageIndex: number, dpi: number) => Promise<boolean> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const unit = useMeasurementUnit();
  const [info, setInfo] = useState<Info | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [dpi, setDpi] = useState('144');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const t = {
    ja: { title: 'PDFページを選択', page: 'ページ', dpi: '解像度 (dpi)', cancel: 'キャンセル', open: '開く', import: '新しいレイヤーに読み込む', hint: '選んだページだけを読み込みます。解像度を変更しても用紙の実寸は保持されます。', limit: '解像度は36〜1200dpi、各辺8192pxまでです。', loading: 'ページ情報を確認中…' },
    en: { title: 'Choose PDF Page', page: 'Page', dpi: 'Resolution (dpi)', cancel: 'Cancel', open: 'Open', import: 'Import as New Layer', hint: 'Only the selected page is imported. Physical page dimensions are preserved when changing resolution.', limit: 'Use 36–1200 dpi, up to 8192 px per side.', loading: 'Reading page information…' },
    'zh-CN': { title: '选择PDF页面', page: '页面', dpi: '分辨率 (dpi)', cancel: '取消', open: '打开', import: '导入为新图层', hint: '仅导入所选页面。更改分辨率时保留页面的实际尺寸。', limit: '分辨率为36–1200dpi，每边最多8192像素。', loading: '正在读取页面信息…' },
  }[locale];
  useEffect(() => {
    dialog.current?.showModal();
    let active = true;
    void invoke<Info>('pdf_import_context').then(value => { if (active) setInfo(value); }).catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, []);
  const page = info?.pages[pageIndex];
  const resolution = Number(dpi);
  const width = page ? Math.ceil(page.widthPoints * resolution / 72) : 0;
  const height = page ? Math.ceil(page.heightPoints * resolution / 72) : 0;
  const displayDpi = resolution >= 36 && resolution <= 1200 ? resolution : 144;
  const pointToUnit = displayDpi / 72 / pixelsPerMeasurement(unit, displayDpi);
  const valid = !!page && Number.isInteger(resolution) && resolution >= 36 && resolution <= 1200 && width <= 8192 && height <= 8192;
  return <dialog ref={dialog} className="transform-dialog" aria-labelledby="pdf-import-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <form onSubmit={async event => { event.preventDefault(); if (!info || !valid || busy) return; setBusy(true); setError(''); try { if (await onApply(info.token, pageIndex, resolution)) onClose(); } catch (cause) { setError(String(cause)); } finally { setBusy(false); } }}>
      <h3 id="pdf-import-title">{t.title}</h3>
      {info ? <>
        <p style={{ overflowWrap: 'anywhere' }}>{info.name}</p>
        <label style={{ display: 'grid', gap: 8 }}>{t.page}<select autoFocus disabled={busy} value={pageIndex} onChange={event => setPageIndex(Number(event.target.value))}>
          {info.pages.map(page => <option key={page.index} value={page.index}>{page.index + 1} / {info.pages.length} — {(page.widthPoints * pointToUnit).toFixed(1)} × {(page.heightPoints * pointToUnit).toFixed(1)} {unitSymbols[unit]}</option>)}
        </select></label>
        <label style={{ display: 'grid', gap: 8, marginTop: 12 }}>{t.dpi}<input type="number" min="36" max="1200" step="1" required disabled={busy} value={dpi} onChange={event => setDpi(event.target.value)} /></label>
        {valid && <p>{width} × {height} px</p>}
        <p>{t.hint}</p><p role={!valid ? 'alert' : undefined}>{t.limit}</p>
      </> : <p>{t.loading}</p>}
      {error && <p role="alert">{error}</p>}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, justifyContent: 'flex-end' }}><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button disabled={busy || !valid}>{info?.asLayer ? t.import : t.open}</button></div>
    </form>
  </dialog>;
}
