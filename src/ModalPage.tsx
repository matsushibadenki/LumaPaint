import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { SettingsDialog } from './components/SettingsDialog';
import { NewDocumentDialog } from './components/NewDocumentDialog';
import { ColorSettingsDialog } from './components/ColorSettingsDialog';
import { TransformDialog } from './components/TransformDialog';
import { DirectControlDialog } from './components/DirectControlDialog';
import { ImportImageDialog } from './components/ImportImageDialog';
import { changeColorProfile, createDocument, importRasterLayer, transformObjects, type DocumentSnapshot, type TransformAction } from './bridge';
import { messages, type Locale, type Theme } from './i18n';
import type { ModalKind } from './NativeModal';
type Context = { kind: ModalKind; locale: Locale; theme: Theme; action: TransformAction; document: DocumentSnapshot | null };
export function ModalPage() {
  const [context, setContext] = useState<Context | null>(null);
  const [error, setError] = useState('');
  const [working, setWorking] = useState(false);
  const close = () => { void invoke('close_modal_window', { requestId: null }).catch(cause => setError(String(cause))); };
  useEffect(() => {
    document.body.classList.add('native-modal-page');
    let active = true;
    void invoke<Context>('modal_context').then(value => { if (active) setContext(value); }).catch(cause => { if (active) { setError(String(cause)); void invoke('close_modal_window', { requestId: null }).catch(() => {}); } });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    if (!context) return;
    document.documentElement.lang = context.locale;
    document.documentElement.dataset.theme = context.theme;
    void invoke('modal_ready').catch(cause => setError(String(cause)));
  }, [context?.locale, context?.theme, !!context]);
  async function run<T>(operation: () => Promise<T>): Promise<T> {
    await invoke('modal_busy', { busy: true });
    setWorking(true);
    try { return await operation(); } finally { setWorking(false); await invoke('modal_busy', { busy: false }); }
  }
  function preferences(locale: Locale, theme: Theme) {
    setContext(previous => previous && { ...previous, locale, theme });
    void invoke('modal_change', { change: { type: 'preferences', locale, theme } }).catch(cause => setError(String(cause)));
  }
  if (!context) return error ? <div role="alert">{error}<button onClick={close}>Close</button></div> : null;
  let content;
  switch (context.kind) {
    case 'settings': content = <SettingsDialog locale={context.locale} theme={context.theme} onLocale={locale => preferences(locale, context.theme)} onTheme={theme => preferences(context.locale, theme)} onClose={close}/>; break;
    case 'newDocument': content = <NewDocumentDialog locale={context.locale} onClose={close} onCreate={settings => run(async () => { await createDocument(settings); await invoke('modal_change', { change: { type: 'createdDocument' } }); })}/>; break;
    case 'importImage': content = <ImportImageDialog locale={context.locale} onClose={close} onImport={format => run(async () => { await importRasterLayer(format); })}/>; break;
    case 'transform': content = <TransformDialog locale={context.locale} action={context.action} onClose={close} onApply={values => run(async () => { await transformObjects(context.action, values); })}/>; break;
    case 'directControls': content = context.document && <DirectControlDialog locale={context.locale} doc={context.document} onApply={() => {}} onBusyChange={busy => invoke('modal_busy', { busy })} onClose={close}/>; break;
    case 'colorSettings': content = context.document && <ColorSettingsDialog locale={context.locale} document={context.document} enabled={!working} onClose={() => { if (!working) close(); }} onProfile={profile => { void run(async () => { const document = await changeColorProfile(profile); setContext(previous => previous && { ...previous, document }); }).catch(cause => setError(String(cause))); }}/>;
  }
  return <>{content}{error && <div className="workspace-error" role="alert">{error}<button onClick={() => setError('')}>{messages[context.locale].dismiss}</button></div>}</>;
}
