import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { messages, type Locale, type Theme } from '../i18n';
import { settingsMessages } from '../settings-i18n';

type Tab = 'general' | 'appearance';
const locales: { value: Locale; label: string }[] = [{ value: 'en', label: 'English' }, { value: 'ja', label: '日本語' }, { value: 'zh-CN', label: '简体中文' }];

export function SettingsDialog({ locale, theme, onLocale, onTheme, onClose }: {
  locale: Locale; theme: Theme; onLocale: (locale: Locale) => void; onTheme: (theme: Theme) => void; onClose: () => void;
}) {
  const [tab, setTab] = useState<Tab>('general');
  const closeButton = useRef<HTMLButtonElement>(null);
  const t = settingsMessages[locale];
  const common = messages[locale];
  useEffect(() => {
    closeButton.current?.focus();
    const keyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [onClose]);

  return createPortal(<div className="settings-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <header><h2 id="settings-title">{t.title}</h2><button ref={closeButton} type="button" className="settings-close" aria-label={t.close} onClick={onClose}>×</button></header>
      <div className="settings-body">
        <div className="settings-tabs" role="tablist" aria-orientation="vertical">
          {(['general', 'appearance'] as const).map(value => <button key={value} type="button" role="tab" aria-selected={tab === value} aria-controls={`settings-panel-${value}`} onClick={() => setTab(value)}>{t[value]}</button>)}
        </div>
        <div className="settings-panel" id={`settings-panel-${tab}`} role="tabpanel">
          {tab === 'general' ? <>
            <h3>{common.language}</h3><p>{t.languageDescription}</p>
            <div className="setting-options">{locales.map(option => <label key={option.value} className="setting-choice"><input type="radio" name="language" value={option.value} checked={locale === option.value} onChange={() => onLocale(option.value)} /><span>{option.label}</span></label>)}</div>
          </> : <>
            <h3>{common.theme}</h3><p>{t.appearanceDescription}</p>
            <div className="appearance-options">{(['system', 'light', 'dark'] as const).map(value => <label key={value} className="appearance-choice"><input type="radio" name="appearance" value={value} checked={theme === value} onChange={() => onTheme(value)} /><span className={`appearance-preview ${value}`} aria-hidden="true"><i /><i /></span><strong>{common[value]}</strong></label>)}</div>
          </>}
        </div>
      </div>
    </section>
  </div>, document.body);
}
