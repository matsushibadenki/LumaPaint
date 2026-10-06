import { useEffect, useMemo, useRef, useState } from 'react';
import type { Locale } from '../i18n';
import { fontCatalog, fontPreview, fontViewerMessages, readFontPreferences, saveFontPreferences, type FontFace, type FontPreferences } from '../font-viewer';
import './font-viewer.css';

function Preview({ face, sample, size, height, locale }: { face: FontFace; sample: string; size: number; height: number; locale: Locale }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(280);
  const [url, setUrl] = useState('');
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const observer = new ResizeObserver(entries => setWidth(Math.max(120, Math.min(1024, Math.floor(entries[0].contentRect.width / 16) * 16))));
    observer.observe(node); return () => observer.disconnect();
  }, []);
  useEffect(() => {
    let live = true; let objectUrl = '';
    setLoading(true); setError(false);
    const timer = window.setTimeout(() => {
      void fontPreview(face, Array.from(sample).slice(0, 128).join(''), size, width, height).then(blob => {
        if (!live) return;
        objectUrl = URL.createObjectURL(blob); setUrl(objectUrl); setLoading(false);
      }).catch(() => { if (live) { setError(true); setLoading(false); setUrl(''); } });
    }, 160);
    return () => { live = false; clearTimeout(timer); if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [face.postscript, sample, size, width, height]);
  return <div ref={ref} className={`font-glyph-preview${loading ? ' loading' : ''}`} style={{ height }} aria-busy={loading}>
    {url && <img src={url} alt={`${face.family} ${face.style}: ${sample}`} width={width} height={height} />}
    {error && <span className="font-preview-error">{fontViewerMessages[locale].unavailable}</span>}
  </div>;
}
export function FontViewer({ locale, currentFont, currentBold, currentItalic, usedFonts, enabled, onApply }: { locale: Locale; currentFont: string; currentBold: boolean; currentItalic: boolean; usedFonts: string[]; enabled: boolean; onApply: (font: FontFace) => Promise<void> }) {
  const t = fontViewerMessages[locale];
  const [faces, setFaces] = useState<FontFace[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const [sample, setSample] = useState(t.defaultSample);
  const [size, setSize] = useState(36);
  const [listSize, setListSize] = useState(20);
  const [mode, setMode] = useState('name');
  const [search, setSearch] = useState('');
  const [category, setCategory] = useState('all');
  const [scope, setScope] = useState('all');
  const [prefs, setPrefs] = useState(readFontPreferences);
  const [group, setGroup] = useState('');
  const [groupName, setGroupName] = useState('');
  const [expanded, setExpanded] = useState(new Set<string>());
  const [scrollTop, setScrollTop] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const [listHeight, setListHeight] = useState(320);
  const [applying, setApplying] = useState(false);
  useEffect(() => { let live = true; void fontCatalog().then(value => { if (live) { setFaces(value); setBusy(false); } }).catch(cause => { if (live) { setError(String(cause)); setBusy(false); } }); return () => { live = false; }; }, []);
  useEffect(() => { const refresh = () => setPrefs(readFontPreferences()); window.addEventListener('storage', refresh); window.addEventListener('font-viewer-preferences', refresh); return () => { window.removeEventListener('storage', refresh); window.removeEventListener('font-viewer-preferences', refresh); }; }, []);
  useEffect(() => { const node = list.current; if (!node) return; const observer = new ResizeObserver(entries => setListHeight(entries[0].contentRect.height)); observer.observe(node); return () => observer.disconnect(); }, [busy]);
  const selectedFace = useMemo(()=>{
    const wanted=selected??currentFont;
    const exact=faces.find(f=>f.postscript===wanted);
    if(exact && (selected!==null || ((exact.weight>=600)===currentBold && exact.italic===currentItalic)))return exact;
    const aliases:Record<string,string[]>={'sans-serif':['Hiragino Sans','Arial','Noto Sans'],'serif':['Hiragino Mincho ProN','Times New Roman','Noto Serif'],'monospace':['Menlo','Consolas','DejaVu Sans Mono']};
    const names=exact?[exact.family]:(aliases[wanted]??[wanted,currentFont]);
    const weight=currentBold?700:400;
    for(const family of names){const candidates=faces.filter(f=>f.family===family).sort((a,b)=>(Math.abs(a.weight-weight)+Number(a.italic!==currentItalic)*1000)-(Math.abs(b.weight-weight)+Number(b.italic!==currentItalic)*1000));if(candidates[0])return candidates[0];}
    return faces[0];
  },[faces,selected,currentFont,currentBold,currentItalic]);
  const matches = useMemo(() => {
    const query = search.toLocaleLowerCase().trim();
    const favorites = new Set(prefs.favorites); const recent = new Set(prefs.recent); const used = new Set(usedFonts);
    const collection = group ? new Set(prefs.groups.find(g => g.id === group)?.fonts ?? []) : null;
    return faces.filter(f => (!query || `${f.family} ${f.style} ${f.postscript}`.toLocaleLowerCase().includes(query)) && (category !== 'japanese' || f.japanese) && (category !== 'latin' || (f.latin && !f.japanese)) && (category !== 'favorites' || favorites.has(f.postscript)) && (scope !== 'recent' || recent.has(f.postscript)) && (scope !== 'used' || used.has(f.postscript) || used.has(f.family)) && (!collection || collection.has(f.postscript)));
  }, [faces, search, category, scope, group, prefs, usedFonts]);
  const rows = useMemo(() => {
    const families = new Map<string, FontFace[]>(); for (const f of matches) { const group = families.get(f.family) ?? []; group.push(f); families.set(f.family, group); }
    const output: { face: FontFace; child: boolean; count: number }[] = [];
    for (const values of families.values()) {
      const representative = [...values].sort((a, b) => (Math.abs(a.weight - 400) + Number(a.italic) * 1000) - (Math.abs(b.weight - 400) + Number(b.italic) * 1000))[0];
      output.push({ face: representative, child: false, count: values.length });
      if (expanded.has(representative.family)) for (const face of values) output.push({ face, child: true, count: 0 });
    }
    return output;
  }, [matches, expanded]);
  useEffect(() => { setScrollTop(0); if (list.current) list.current.scrollTop = 0; }, [search, category, scope, group]);
  const rowHeight = Math.max(58, listSize + 38); const first = Math.max(0, Math.floor(scrollTop / rowHeight) - 2); const visible = rows.slice(first, first + Math.ceil(listHeight / rowHeight) + 5);
  function save(value: FontPreferences) { setPrefs(value); saveFontPreferences(value); }
  function toggleFavorite(face: FontFace) { const latest = readFontPreferences(); save({ ...latest, favorites: latest.favorites.includes(face.postscript) ? latest.favorites.filter(f => f !== face.postscript) : [...latest.favorites, face.postscript] }); }
  async function apply(face: FontFace) { if (!enabled || applying) return; setApplying(true); setError(''); try { await onApply(face); const latest = readFontPreferences(); save({ ...latest, recent: [face.postscript, ...latest.recent.filter(f => f !== face.postscript)].slice(0, 30) }); } catch (cause) { setError(String(cause)); } finally { setApplying(false); } }
  function changeGroup(remove: boolean) { if (!selectedFace || !group) return; const latest = readFontPreferences(); save({ ...latest, groups: latest.groups.map(g => g.id !== group ? g : { ...g, fonts: remove ? g.fonts.filter(f => f !== selectedFace.postscript) : [...new Set([...g.fonts, selectedFace.postscript])] }) }); }
  return <div className="font-viewer">
    {selectedFace && <><div className="font-large-preview"> <Preview face={selectedFace} sample={sample} size={size} height={160} locale={locale} /><button className="font-reset" aria-label={t.reset} title={t.reset} onClick={() => setSample(t.defaultSample)}>↵</button></div>
      <details className="font-sample-edit"><summary>{t.sample}</summary><textarea aria-label={t.sample} maxLength={128} value={sample} onChange={e => setSample(e.target.value.replace(/[\r\t]/g, ' '))} rows={2} /></details>
      <div className="font-current"><span title={selectedFace.postscript}>{selectedFace.family} · {selectedFace.style}</span><button disabled={!enabled || applying} onClick={() => void apply(selectedFace)}>{t.apply}</button></div>
      <label className="font-size-control"><span>{t.previewSize}</span><input aria-label={t.previewSize} type="range" min={12} max={72} value={size} onChange={e => setSize(Number(e.target.value))} /><output>{size}</output></label>
      <small className="font-fallback-note">{t.fallback}</small></>}
    <div className="font-list-options"><select aria-label={t.previewMode} value={mode} onChange={e => setMode(e.target.value)}><option value="name">{t.nameMode}</option><option value="sample">{t.previewMode}</option></select><label className="font-size-control"><input aria-label={t.listSize} type="range" min={14} max={36} value={listSize} onChange={e => setListSize(Number(e.target.value))} /><output>{listSize}</output></label></div>
    <input className="font-search" type="search" aria-label={t.search} placeholder={`⌕ ${t.search}`} value={search} onChange={e => setSearch(e.target.value)} />
    <div className="font-filter-bar" role="group" aria-label={t.title}>{(['all', 'japanese', 'latin', 'favorites'] as const).map(c => <button key={c} aria-pressed={category === c} onClick={() => setCategory(c)}>{t[c]}</button>)}</div>
    <div className="font-filter-bar"><button aria-pressed={scope === 'used'} onClick={() => setScope(scope === 'used' ? 'all' : 'used')}>{t.used}</button><button aria-pressed={scope === 'recent'} onClick={() => setScope(scope === 'recent' ? 'all' : 'recent')}>{t.recent}</button></div>
    <details className="font-collection-editor"><summary>{t.group}{group ? ` · ${prefs.groups.find(g=>g.id===group)?.name ?? ""}` : ""}</summary><div className="font-collections"><select aria-label={t.group} value={group} onChange={e => setGroup(e.target.value)}><option value="">{t.group}</option>{prefs.groups.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select>{group && <button aria-label={t.removeGroup} title={t.removeGroup} onClick={() => { const latest = readFontPreferences(); save({ ...latest, groups: latest.groups.filter(g => g.id !== group) }); setGroup(''); }}>−</button>}</div>
    <form className="font-collections" onSubmit={e => { e.preventDefault(); const name = groupName.trim(); if (!name || prefs.groups.length >= 30) return; const id = crypto.randomUUID(); const latest = readFontPreferences(); save({ ...latest, groups: [...latest.groups, { id, name, fonts: [] }] }); setGroup(id); setGroupName(''); }}><input aria-label={t.groupName} placeholder={t.groupName} maxLength={80} value={groupName} onChange={e => setGroupName(e.target.value)} /><button aria-label={t.addGroup} title={t.addGroup} disabled={!groupName.trim() || prefs.groups.length >= 30}>＋</button></form>
    {group && selectedFace && <div className="font-group-actions"><button onClick={() => changeGroup(false)}>{t.addToGroup}</button><button onClick={() => changeGroup(true)}>{t.removeFromGroup}</button></div>}
    </details>
    <div className="font-list-summary" aria-live="polite">{busy ? t.loading : `${matches.length} ${t.count}`}</div>
    {error && <p role="alert">{error}</p>}
    <div className="font-list" ref={list} onScroll={e => setScrollTop(e.currentTarget.scrollTop)} role="list" aria-label={t.title}>
      {!busy && rows.length === 0 && <p>{t.empty}</p>}
      <div style={{ height: rows.length * rowHeight, position: 'relative' }}>{visible.map(({ face, child, count }, index) => <div className={`font-row${child ? ' child' : ''}${selectedFace?.postscript === face.postscript ? ' selected' : ''}`} key={`${child ? 'style' : 'family'}-${face.postscript}`} style={{ top: (first + index) * rowHeight, height: rowHeight }} role="listitem">
        <button className="font-expand" aria-label={`${t.styles}: ${face.family}`} aria-expanded={expanded.has(face.family)} disabled={child || count < 2} onClick={() => setExpanded(old => { const next = new Set(old); if (next.has(face.family)) next.delete(face.family); else next.add(face.family); return next; })}>{child || count < 2 ? '' : expanded.has(face.family) ? '▾' : '▸'}</button>
        <button className="font-star" aria-label={`${t.favorite}: ${face.family} ${face.style}`} aria-pressed={prefs.favorites.includes(face.postscript)} onClick={() => toggleFavorite(face)}>{prefs.favorites.includes(face.postscript) ? '★' : '☆'}</button>
        <button className="font-row-choice" title={`${face.family} ${face.style}\n${face.postscript}`} aria-pressed={selectedFace?.postscript === face.postscript} onClick={() => setSelected(face.postscript)} onDoubleClick={() => void apply(face)}>
          <Preview face={face} sample={mode === 'name' ? Array.from(child ? face.style : face.family).slice(0, 60).join('') : Array.from(sample.replace(/\n/g, ' ')).slice(0, 60).join('')} size={listSize} height={Math.max(32, Math.min(68, listSize + 10))} locale={locale} />
          <span className="font-row-caption">{child ? face.style : face.family}<small>{face.adobe ? 'Adobe Fonts' : face.style}{count > 1 ? ` · ${count}` : ''}</small></span>
        </button>
      </div>)}</div>
    </div>
  </div>;
}
