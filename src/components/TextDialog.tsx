import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { Brush, DocumentSnapshot, TextSettings, VectorText } from '../bridge';
import type { Locale } from '../i18n';
import { textMessages } from '../text-i18n';
import { fromHex, toHex } from './BrushControls';

export function TextDialog({ locale, document, color, enabled, onApply, onClose }: {
  locale: Locale; document: DocumentSnapshot; color: Brush['color']; enabled: boolean;
  onApply: (settings: TextSettings) => Promise<void>; onClose: () => void;
}) {
  const t = textMessages[locale];
  const dialog = useRef<HTMLElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const initial = document.textObjects.find(item => document.selectedVectorObjects.includes(item.id));
  const newSettings = (): TextSettings => ({ id: null, text: { content: t.defaultText, fontFamily: 'sans-serif', fontSize: 48, lineHeight: 1.4, bold: false }, position: [Math.min(48, document.width / 10), Math.min(48, document.height / 10)], color });
  const [draft, setDraft] = useState<TextSettings>(() => initial ?? newSettings());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const locked = draft.id !== null && !document.textObjects.find(item => item.id === draft.id)?.editable;
  const valid = draft.text.content.trim().length > 0 && [...draft.text.content].length <= 4096 && draft.text.content.split('\n').length <= 64;
  const patchText = (patch: Partial<VectorText>) => setDraft(current => ({ ...current, text: { ...current.text, ...patch } }));
  useEffect(() => { input.current?.focus(); }, []);
  return createPortal(<div className="settings-backdrop">
    <section ref={dialog} className="settings-dialog text-dialog" role="dialog" aria-modal="true" aria-labelledby="text-title" onKeyDown={event => {
      event.stopPropagation();
      if (event.key === 'Escape' && !busy) onClose();
      if (event.key === 'Tab') {
        const controls = [...dialog.current!.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)')];
        const first = controls[0], last = controls[controls.length - 1];
        if (event.shiftKey && globalThis.document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && globalThis.document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    }}>
      <header><h2 id="text-title">{t.title}</h2><button type="button" className="settings-close" disabled={busy} aria-label={t.close} onClick={onClose}>×</button></header>
      <form className="text-settings-content" onSubmit={async event => {
        event.preventDefault();
        if (!enabled || busy || locked || !valid) return;
        setBusy(true); setError('');
        try { await onApply(draft); onClose(); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
      }}>
        <label>{t.object}<select disabled={busy} value={draft.id ?? ''} onChange={event => { const item = document.textObjects.find(value => value.id === event.target.value); setDraft(item ?? newSettings()); setError(''); }}><option value="">{t.create}</option>{document.textObjects.map(item => <option key={item.id} value={item.id}>{item.text.content.split('\n')[0].slice(0, 40)}</option>)}</select></label>
        <label>{t.content}<textarea ref={input} rows={3} required disabled={busy || locked} value={draft.text.content} onChange={event => patchText({ content: event.target.value })} /></label>
        <div className="text-settings-grid">
          <label>{t.font}<select disabled={busy || locked} value={draft.text.fontFamily} onChange={event => patchText({ fontFamily: event.target.value as VectorText['fontFamily'] })}><option value="sans-serif">{t.sans}</option><option value="serif">{t.serif}</option><option value="monospace">{t.mono}</option></select></label>
          <label>{t.size}<input type="number" min="1" max="512" step="1" required disabled={busy || locked} value={draft.text.fontSize} onChange={event => patchText({ fontSize: Number(event.target.value) })} /></label>
          <label>{t.leading}<input type="number" min="0.8" max="3" step="0.1" required disabled={busy || locked} value={draft.text.lineHeight} onChange={event => patchText({ lineHeight: Number(event.target.value) })} /></label>
          <label className="text-bold"><input type="checkbox" disabled={busy || locked} checked={draft.text.bold} onChange={event => patchText({ bold: event.target.checked })} />{t.bold}</label>
          {([0, 1] as const).map(index => <label key={index}>{index === 0 ? t.x : t.y}<input type="number" min="-99999" max="99999" step="any" required disabled={busy || locked} value={draft.position[index]} onChange={event => setDraft(current => ({ ...current, position: index === 0 ? [Number(event.target.value), current.position[1]] : [current.position[0], Number(event.target.value)] }))} /></label>)}
          <label>{t.color}<input type="color" disabled={busy || locked} value={toHex(draft.color)} onChange={event => setDraft(current => ({ ...current, color: fromHex(event.target.value) }))} /></label>
        </div>
        <div className="text-live-preview" aria-label={t.preview} style={{ fontFamily: draft.text.fontFamily, fontSize: Math.min(72, Math.max(1, draft.text.fontSize)), lineHeight: draft.text.lineHeight, fontWeight: draft.text.bold ? 700 : 400, color: toHex(draft.color) }}>{draft.text.content}</div>
        <p className="muted">{t.hint}</p>
        {!enabled && <p>{t.unsupported}</p>}{locked && <p>{t.locked}</p>}{!valid && <p role="alert">{t.invalid}</p>}{error && <p role="alert">{error}</p>}
        <footer><button type="button" disabled={busy} onClick={onClose}>{t.cancel}</button><button type="submit" disabled={!enabled || busy || locked || !valid}>{busy ? t.saving : t.apply}</button></footer>
      </form>
    </section>
  </div>, globalThis.document.body);
}
