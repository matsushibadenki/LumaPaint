import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { Locale } from '../i18n';
import { commandPaletteLabels } from '../command-palette-i18n';
import { accelerator, binding, displayKey, executeCommand, useCommandAvailability, useShortcuts } from '../shortcuts';
import './command-palette.css';

export function CommandPalette({ locale, onClose }: { locale: Locale; onClose: () => void }) {
  const labels = commandPaletteLabels[locale];
  const { commands, settings } = useShortcuts();
  const unavailable = useCommandAvailability();
  const dialog = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState('');
  const [activeId, setActiveId] = useState('');
  const normalize = (value: string) => value.normalize('NFKC').toLowerCase();
  const entries = useMemo(() => {
    const terms = normalize(query).trim().split(/\s+/);
    return commands.filter(command => command.id !== 'app.commandPalette').flatMap(command => {
      const key = binding(settings, command);
      const shortcut = displayKey(key);
      const haystack = normalize([command.label, command.category, shortcut, accelerator(key)].join(' '));
      return terms.every(term => haystack.includes(term)) ? [{ ...command, shortcut }] : [];
    }).sort((a, b) => Number(unavailable.has(a.id)) - Number(unavailable.has(b.id)));
  }, [commands, settings, query, unavailable]);
  const enabled = entries.filter(entry => !unavailable.has(entry.id));
  const active = enabled.find(entry => entry.id === activeId) ?? enabled[0];

  useLayoutEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);

  function execute(id: string) {
    dialog.current?.close();
    onClose();
    executeCommand(id);
  }

  return <dialog ref={dialog} className="command-palette" aria-label={labels.title}
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => { if (event.target === dialog.current) onClose(); }}>
    <div className="command-palette-content">
      <div className="command-palette-search">
        <input autoFocus role="combobox" aria-label={labels.search} placeholder={labels.search}
          aria-expanded="true" aria-controls="command-palette-results"
          aria-activedescendant={active ? `command-result-${active.id}` : undefined}
          value={query} onChange={event => { setQuery(event.currentTarget.value); setActiveId(''); }}
          onKeyDown={event => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (event.key === 'Enter' && active) {
              event.preventDefault();
              execute(active.id);
            } else if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && enabled.length) {
              event.preventDefault();
              const offset = event.key === 'ArrowDown' ? 1 : -1;
              const index = enabled.findIndex(entry => entry.id === active?.id);
              const next = enabled[(index + offset + enabled.length) % enabled.length];
              setActiveId(next.id);
              document.getElementById(`command-result-${next.id}`)?.scrollIntoView({ block: 'nearest' });
            }
          }} />
        <button type="button" aria-label={labels.close} onClick={onClose}>×</button>
      </div>
      <ul id="command-palette-results" role="listbox" aria-label={labels.title}>
        {entries.map(entry => <li key={entry.id} id={`command-result-${entry.id}`}
          role="option" aria-selected={active?.id === entry.id} aria-disabled={unavailable.has(entry.id)}
          onMouseDown={event => event.preventDefault()}
          onClick={() => { if (!unavailable.has(entry.id)) execute(entry.id); }}>
          <span>{entry.label}<small>{entry.category}</small>
            {unavailable.has(entry.id) && <small>{unavailable.get(entry.id) || labels.unavailable}</small>}
          </span>
          {entry.shortcut && <kbd>{entry.shortcut}</kbd>}
        </li>)}
      </ul>
      {!entries.length && <p role="status">{labels.empty}</p>}
    </div>
  </dialog>;
}
