import { useEffect, useState } from 'react';
import { defaultVectorText, textFonts, type TextSettings, type TextStyle, type VectorText } from '../bridge';
import type { Locale } from '../i18n';
import { textPanelMessages } from '../text-panel-i18n';
import { textMessages } from '../text-i18n';
import { fromHex, toHex } from './BrushControls';

export function TextPanel({ locale, settings, resolution, enabled, editing, onChange, onBegin, onFinish }: {
  locale: Locale; settings: TextSettings | null; resolution: number; enabled: boolean; editing: boolean;
  onChange: (value: TextSettings) => Promise<void>; onBegin: () => void; onFinish: (commit: boolean) => void;
}) {
  const t = textPanelMessages[locale];
  const [fonts, setFonts] = useState<string[]>([]);
  const [draft, setDraft] = useState(settings);
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  useEffect(() => { let active = true; textFonts().then(value => { if (active) setFonts(value); }).catch(cause => { if (active) setError(String(cause)); }); return () => { active = false; }; }, []);
  // Do not reset fields for unrelated document/recovery updates.
  const signature = JSON.stringify(settings);
  useEffect(() => { setDraft(settings); setError(''); }, [signature]);
  const base = draft?.text ?? defaultVectorText;
  const style = draft?.selection?.style ?? (base.runs?.[0]?.start === 0 ? base.runs[0].style : undefined);
  const text = { ...base, ...style, ...draft?.stylePatch };
  const inherited = { ...base, color: draft?.color ?? [32, 32, 32] };
  const wholeStyles = [base.runs?.[0]?.start === 0 ? base.runs[0].style : inherited, ...(base.runs ?? []).map(run => run.style)];
  if ((base.runs ?? []).reduce((length, run) => length + run.end - run.start, 0) < base.content.length) wholeStyles.push(inherited);
  const mixed = draft?.selection?.mixed ?? (['fontFamily', 'fontSize', 'bold', 'italic', 'tracking', 'baselineShift', 'underline', 'strikethrough', 'color'] as (keyof TextStyle)[]).filter(key => wholeStyles.some(value => JSON.stringify(value[key]) !== JSON.stringify(wholeStyles[0][key])));
  const characterKeys = ['fontFamily', 'fontSize', 'bold', 'italic', 'tracking', 'baselineShift', 'underline', 'strikethrough'] as const;
  const color = draft?.stylePatch?.color ?? style?.color ?? draft?.color ?? [32, 32, 32];
  const disabled = !enabled || !draft || pending;
  const pt = 72 / Math.max(1, resolution);
  async function apply(next: TextSettings) {
    setDraft(next); setPending(true); setError('');
    try { await onChange(next); } catch (cause) { setError(String(cause)); } finally { setPending(false); }
  }
  function change(patch: Partial<VectorText>, commit = true) {
    if (!draft) return;
    const isCharacter = Object.keys(patch).every(key => (characterKeys as readonly string[]).includes(key));
    const next = isCharacter
      ? { ...draft, stylePatch: { ...draft.stylePatch, ...patch } as Partial<TextStyle> }
      : { ...draft, stylePatch: undefined, text: { ...draft.text, ...patch } };
    if (commit) void apply(next); else setDraft(next);
  }
  function numeric(key: keyof VectorText, label: string, icon: string, unit: string, factor = 1, min = -4096, max = 4096, step = 0.1) {
    return <label className="type-field" title={label}><span className="type-symbol" aria-hidden="true">{icon}</span><span className="type-number"><input aria-label={label} type="number" min={min} max={max} step={unit === 'pt' ? 'any' : step} placeholder={mixed.includes(key as keyof TextStyle) ? t.mixed : undefined} value={mixed.includes(key as keyof TextStyle) && !Object.prototype.hasOwnProperty.call(draft?.stylePatch ?? {}, key) ? '' : Number((Number(text[key]) * factor).toFixed(2))} onChange={event => change({ [key]: Number(event.target.value) / factor }, false)} onBlur={event => { if (event.currentTarget.validity.valid && draft && (draft.stylePatch || draft.text !== settings?.text)) void apply(draft); }} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} /><span>{unit}</span></span></label>;
  }
  return <div className="text-panel">
    <div className="type-panel-status">
      {editing && <p>{t.editing}</p>}
      {settings && <p>{editing ? (draft?.selection?.length ? `${t.selection}: ${draft.selection.characters}` : t.insertion) : t.whole}{mixed.length > 0 ? ` · ${t.mixed}` : ''}</p>}
      <div className="type-actions">{editing ? <><button onClick={() => onFinish(true)}>{t.done}</button><button onClick={() => onFinish(false)}>{t.cancel}</button></> : <button disabled={!enabled} onClick={onBegin}>{settings ? t.edit : t.add}</button>}</div>
      {!settings && <p>{t.empty}</p>}
      {settings && !enabled && <p>{t.locked}</p>}
      {error && <p role="alert">{error}</p>}
    </div>
    <fieldset disabled={disabled}>
      <section className="type-section">
        <h3>{t.title}<span aria-hidden="true">☰</span></h3>
        <div className="type-section-body">
          <select aria-label={t.font} value={mixed.includes('fontFamily') && !draft?.stylePatch?.fontFamily ? '' : text.fontFamily} onChange={event => change({ fontFamily: event.target.value })}>
            <option value="" disabled>{t.mixed}</option><option value="sans-serif">{textMessages[locale].sans}</option><option value="serif">{textMessages[locale].serif}</option><option value="monospace">{textMessages[locale].mono}</option>
            {!fonts.includes(text.fontFamily) && !['sans-serif', 'serif', 'monospace'].includes(text.fontFamily) && <option>{text.fontFamily}</option>}
            {fonts.map(font => <option key={font}>{font}</option>)}
          </select>
          <select aria-label={t.style} value={mixed.includes('bold') || mixed.includes('italic') ? '' : `${Number(text.bold)}${Number(text.italic)}`} onChange={event => change({ bold: event.target.value[0] === '1', italic: event.target.value[1] === '1' })}>
            <option value="" disabled>{t.mixed}</option><option value="00">{t.regular}</option><option value="10">{t.bold}</option><option value="01">{t.italic}</option><option value="11">{t.boldItalic}</option>
          </select>
          <div className="type-grid">
            {numeric('fontSize', t.size, 'T↕', 'pt', pt, pt, 512 * pt)}
            <label className="type-field" title={t.leading}><span className="type-symbol" aria-hidden="true">A↕</span><span className="type-number"><input aria-label={t.leading} type="number" min={Number((base.fontSize * .8 * pt).toFixed(2))} max={Number((base.fontSize * 3 * pt).toFixed(2))} step="any" value={Number((base.fontSize * base.lineHeight * pt).toFixed(2))} onChange={event => change({ lineHeight: Number(event.target.value) / (base.fontSize * pt) }, false)} onBlur={event => { if (event.currentTarget.validity.valid && draft && (draft.stylePatch || draft.text !== settings?.text)) void apply(draft); }} /><span>pt</span></span></label>
            {numeric('scaleY', t.vertical, 'T↕', '%', 100, 10, 400, 1)}
            {numeric('scaleX', t.horizontal, 'T↔', '%', 100, 10, 400, 1)}
            {numeric('tracking', t.tracking, 'VA', '', 1, -100, 1000, 1)}
            <label className="type-field" title={t.color}><span className="type-symbol" aria-hidden="true">■</span><span className="type-color"><input aria-label={t.color} type="color" value={toHex(color)} onChange={event => { if (draft) void apply({ ...draft, stylePatch: { color: fromHex(event.target.value) } }); }} /><input key={toHex(color)} aria-label={`${t.color} HEX`} type="text" defaultValue={toHex(color)} pattern="#[0-9a-fA-F]{6}" maxLength={7} spellCheck={false} onBlur={event => { if (draft && event.currentTarget.validity.valid && /^#[0-9a-fA-F]{6}$/.test(event.target.value) && event.target.value.toLowerCase() !== toHex(color)) void apply({ ...draft, stylePatch: { color: fromHex(event.target.value) } }); }} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} /></span></label>
            {numeric('baselineShift', t.baseline, 'A↟', 'pt', pt, -512 * pt, 512 * pt)}
            {numeric('rotation', t.rotation, 'T↻', '°', 1, -180, 180, 1)}
          </div>
          <div className="type-toggles">
            {(['bold', 'italic', 'underline', 'strikethrough'] as const).map((key, index) => <button key={key} type="button" aria-label={key === 'strikethrough' ? t.strike : t[key]} title={key === 'strikethrough' ? t.strike : t[key]} aria-pressed={mixed.includes(key) ? 'mixed' : text[key]} className={`type-toggle type-${key}`} onClick={() => change({ [key]: !text[key] })}>{['B', 'I', 'T', 'T'][index]}</button>)}
          </div>
        </div>
      </section>
      <section className="type-section">
        <h3>{t.paragraph}<span aria-hidden="true">☰</span></h3>
        <div className="type-section-body">
          <div className="type-alignments">{(['left', 'center', 'right'] as const).map(align => <button type="button" aria-label={t[align]} title={t[align]} aria-pressed={text.alignment === align} onClick={() => change({ alignment: align })} key={align}><svg width="24" height="22" viewBox="0 0 24 22" aria-hidden="true">{[3,7,11,15,19].map((y, i) => <path key={y} d={`M${align === 'right' && i % 2 ? 8 : align === 'center' && i % 2 ? 5 : 2} ${y}h${i % 2 ? 14 : 20}`} stroke="currentColor" />)}</svg></button>)}</div>
          <div className="type-grid">
            {numeric('indentLeft', t.indentLeft, '↦', 'pt', pt, 0, 4096 * pt)}
            {numeric('indentRight', t.indentRight, '↤', 'pt', pt, 0, 4096 * pt)}
            {numeric('indentFirst', t.indentFirst, '↳', 'pt', pt, -4096 * pt, 4096 * pt)}
            {numeric('boxWidth', t.width, '↔', 'pt', pt, 16 * pt, 8192 * pt)}
            {numeric('spaceBefore', t.before, '↥', 'pt', pt, 0, 512 * pt)}
            {numeric('spaceAfter', t.after, '↧', 'pt', pt, 0, 512 * pt)}
          </div>
          <p className="type-hint">{t.paragraphHint}</p>
        </div>
      </section>
    </fieldset>
    <section className="type-section">
      <h3>OpenType<span aria-hidden="true">☰</span></h3>
      <div className="type-section-body"><div className="type-toggles"><button disabled title={t.ligatures}>fi</button><button disabled title={t.alternates}>Aα</button><button disabled title={t.smallCaps}>Tᴛ</button></div><p className="type-hint">{t.advanced}</p></div>
    </section>
  </div>;
}
