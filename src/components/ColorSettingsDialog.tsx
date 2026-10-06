import {colorProfiles as profiles, modeLabels} from '../document-color-modes';
import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import type { ColorProfile, DocumentSnapshot } from '../bridge';
import type { Locale } from '../i18n';
import { colorSettingsMessages } from '../color-settings-i18n';

export function ColorSettingsDialog({ locale, document, enabled, onProfile, onClose }: {
  locale: Locale; document: DocumentSnapshot; enabled: boolean;
  onProfile: (profile: ColorProfile) => void; onClose: () => void;
}) {
  const closeButton = useRef<HTMLButtonElement>(null);
  const t = colorSettingsMessages[locale];
  const compatible = profiles.filter(profile => profile.mode === document.colorMode);
  useEffect(() => {
    closeButton.current?.focus();
    const keyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [onClose]);

  return createPortal(<div className="settings-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="settings-dialog color-settings-dialog" role="dialog" aria-modal="true" aria-labelledby="color-settings-title">
      <header><h2 id="color-settings-title">{t.title}</h2><button ref={closeButton} type="button" className="settings-close" aria-label={t.close} onClick={onClose}>×</button></header>
      <div className="color-settings-content">
        <div className="document-color-summary"><span>{t.documentMode}</span><strong>{modeLabels[locale][document.colorMode]} · {document.bitDepth} bits</strong></div>
        <label className="color-profile-field"><span>{t.profile}</span><select value={document.colorProfile} disabled={!enabled} onChange={event => onProfile(event.target.value as ColorProfile)}>{compatible.map(profile => <option key={profile.value} value={profile.value}>{profile.label}</option>)}</select></label>
        <p>{t.description}</p><p className="muted">{t.savedWithDocument}</p>
      </div>
    </section>
  </div>, globalThis.document.body);
}
