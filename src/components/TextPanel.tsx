import { useEffect, useRef, useState } from 'react';
import { defaultVectorText, textFonts, type TextSettings, type TextStyle, type VectorText } from '../bridge';
import type { Locale } from '../i18n';
import { textPanelMessages } from '../text-panel-i18n';
import { textMessages } from '../text-i18n';
import { fromHex, toHex } from './BrushControls';

function NumberField({ label, value, placeholder, min, max, step, unit, onValidChange }: {
  label: string; value: number | null; placeholder?: string; min: number; max: number; step: number | 'any'; unit: string;
  onValidChange: (value: number) => void;
}) {
  const display = value === null ? '' : String(Number(value.toFixed(2)));
  const [input, setInput] = useState(display);
  const [focused, setFocused] = useState(false);
  useEffect(() => { if (!focused) setInput(display); }, [display, focused]);
  return <span className="type-number"><input aria-label={label} type="number" min={min} max={max} step={step} placeholder={placeholder} value={input} onFocus={() => setFocused(true)} onChange={event => {
    const next = event.currentTarget.value;
    setInput(next);
    if (next !== '' && event.currentTarget.validity.valid) onValidChange(Number(next));
  }} onBlur={event => {
    setFocused(false);
    if (event.currentTarget.value === '' || !event.currentTarget.validity.valid) setInput(display);
  }} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} /><span>{unit}</span></span>;
}

function HexColorField({ label, value, onValidChange }: { label: string; value: string; onValidChange: (value: string) => void }) {
  const [input, setInput] = useState(value);
  const [focused, setFocused] = useState(false);
  useEffect(() => { if (!focused) setInput(value); }, [focused, value]);
  return <input aria-label={`${label} HEX`} type="text" value={input} pattern="#[0-9a-fA-F]{6}" maxLength={7} spellCheck={false} onFocus={() => setFocused(true)} onChange={event => {
    const next = event.currentTarget.value;
    setInput(next);
    if (/^#[0-9a-fA-F]{6}$/.test(next)) onValidChange(next);
  }} onBlur={() => { setFocused(false); if (!/^#[0-9a-fA-F]{6}$/.test(input)) setInput(value); }} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur(); }} />;
}

export function TextPanel({ locale, settings, resolution, enabled, editing, onChange, onBegin, onFinish }: {
  locale: Locale; settings: TextSettings | null; resolution: number; enabled: boolean; editing: boolean;
  onChange: (value: TextSettings) => Promise<void>; onBegin: () => void; onFinish: (commit: boolean) => void;
}) {
  const t = textPanelMessages[locale];
  const [fonts, setFonts] = useState<string[]>([]);
  const [draft, setDraft] = useState(settings);
  const [error, setError] = useState('');
  const pending = useRef(0);
  const changeQueue = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => { let active = true; textFonts().then(value => { if (active) setFonts(value); }).catch(cause => { if (active) setError(String(cause)); }); return () => { active = false; }; }, []);
  // Do not reset fields for unrelated document/recovery updates.
  const signature = JSON.stringify(settings);
  useEffect(() => {
    if (pending.current === 0) setDraft(settings);
    setError('');
  }, [signature]);
  const base = draft?.text ?? defaultVectorText;
  const style = draft?.selection?.style ?? (base.runs?.[0]?.start === 0 ? base.runs[0].style : undefined);
  const text = { ...base, ...style, ...draft?.stylePatch };
  const inherited = { ...base, color: draft?.color ?? [32, 32, 32] };
  const wholeStyles = [base.runs?.[0]?.start === 0 ? base.runs[0].style : inherited, ...(base.runs ?? []).map(run => run.style)];
  if ((base.runs ?? []).reduce((length, run) => length + run.end - run.start, 0) < base.content.length) wholeStyles.push(inherited);
  const mixed = draft?.selection?.mixed ?? (['fontFamily', 'fontSize', 'bold', 'italic', 'tracking', 'baselineShift', 'underline', 'strikethrough', 'color'] as (keyof TextStyle)[]).filter(key => wholeStyles.some(value => JSON.stringify(value[key]) !== JSON.stringify(wholeStyles[0][key])));
  const characterKeys = ['fontFamily', 'fontSize', 'bold', 'italic', 'tracking', 'baselineShift', 'underline', 'strikethrough'] as const;
  const color = draft?.stylePatch?.color ?? style?.color ?? draft?.color ?? [32, 32, 32];
  const disabled = !enabled || !draft;
  const pt = 72 / Math.max(1, resolution);
  function apply(next: TextSettings) {
    setDraft(next); setError(''); pending.current += 1;
    const task = changeQueue.current.then(() => onChange(next));
    changeQueue.current = task.then(() => undefined, cause => { setError(String(cause)); });
    void task.finally(() => { pending.current -= 1; }).catch(() => {});
  }
  function change(patch: Partial<VectorText>) {
    if (!draft) return;
    const isCharacter = Object.keys(patch).every(key => (characterKeys as readonly string[]).includes(key));
    const next = isCharacter
      ? { ...draft, stylePatch: { ...draft.stylePatch, ...patch } as Partial<TextStyle> }
      : { ...draft, stylePatch: undefined, text: { ...draft.text, ...patch } };
    apply(next);
  }
  async function finish(commit: boolean) {
    await changeQueue.current;
    onFinish(commit);
  }
  function numeric(key: keyof VectorText, label: string, icon: string, unit: string, factor = 1, min = -4096, max = 4096, step = 0.1) {
    const isMixed = mixed.includes(key as keyof TextStyle) && !Object.prototype.hasOwnProperty.call(draft?.stylePatch ?? {}, key);
    return <label className="type-field" title={label}><span className="type-symbol" aria-hidden="true">{icon}</span><NumberField label={label} value={isMixed ? null : Number(text[key]) * factor} min={min} max={max} step={unit === 'pt' ? 'any' : step} unit={unit} placeholder={isMixed ? t.mixed : undefined} onValidChange={value => change({ [key]: value / factor })} /></label>;
  }
  return <div className="text-panel">
    <div className="type-panel-status">
      {editing && <p>{t.editing}</p>}
      {settings && <p>{editing ? (draft?.selection?.length ? `${t.selection}: ${draft.selection.characters}` : t.insertion) : t.whole}{mixed.length > 0 ? ` · ${t.mixed}` : ''}</p>}
      <div className="type-actions">{editing ? <><button onClick={() => void finish(true)}>{t.done}</button><button onClick={() => void finish(false)}>{t.cancel}</button></> : <button disabled={!enabled} onClick={onBegin}>{settings ? t.edit : t.add}</button>}</div>
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
            <label className="type-field" title={t.leading}><span className="type-symbol" aria-hidden="true">A↕</span><NumberField label={t.leading} value={base.fontSize * base.lineHeight * pt} min={Number((0.1 * pt).toFixed(4))} max={Number((4096 * pt).toFixed(2))} step="any" unit="pt" onValidChange={value => change({ lineHeight: value / (base.fontSize * pt) })} /></label>
            {numeric('scaleY', t.vertical, 'T↕', '%', 100, 10, 400, 1)}
            {numeric('scaleX', t.horizontal, 'T↔', '%', 100, 10, 400, 1)}
            {numeric('tracking', t.tracking, 'VA', '', 1, -100, 1000, 1)}
            <label className="type-field" title={t.color}><span className="type-symbol" aria-hidden="true">■</span><span className="type-color"><input aria-label={t.color} type="color" value={toHex(color)} onChange={event => { if (draft) apply({ ...draft, stylePatch: { ...draft.stylePatch, color: fromHex(event.target.value) } }); }} /><HexColorField label={t.color} value={toHex(color)} onValidChange={value => { if (draft) apply({ ...draft, stylePatch: { ...draft.stylePatch, color: fromHex(value) } }); }} /></span></label>
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
            <label className="type-field" title={t.height}><span className="type-symbol" aria-hidden="true">↕</span><NumberField label={t.height} value={base.boxHeight == null ? 0 : base.boxHeight * pt} min={0} max={8192 * pt} step="any" unit="pt" onValidChange={value => change({ boxHeight: value === 0 ? null : value / pt })} /></label>
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
