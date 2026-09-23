import { useEffect, useState } from 'react';
import { deleteAllRecoveries, deleteRecovery, getRecoveryInfo, restoreRecovery, retryRecovery, type DocumentSnapshot, type RecoveryInfo } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';

export function RecoveryControls({ locale, document, onDocument }: {
  locale: Locale; document: DocumentSnapshot; onDocument: (value: DocumentSnapshot) => void;
}) {
  const t = workspaceMessages[locale];
  const [info, setInfo] = useState<RecoveryInfo | null>(null);
  const [selected, setSelected] = useState('');
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try { const next = await getRecoveryInfo(); if (active) setInfo(next); }
      catch (cause) { if (active) setError(String(cause)); }
      finally { if (active) timer = setTimeout(() => void refresh(), 2000); }
    };
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  }, []);
  if (!info && !error) return null;
  const candidates = info?.candidates ?? [];
  const id = candidates.some(candidate => candidate.id === selected) ? selected : candidates[0]?.id;
  const run = async (restore: boolean) => {
    if (busy) return;
    setBusy(true); setError('');
    try {
      if (restore && id) onDocument(await restoreRecovery(id)); else await retryRecovery();
      setInfo(await getRecoveryInfo());
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const remove = async (all: boolean) => {
    if (busy || (!all && !id)) return;
    const message = all ? t.deleteAllRecoveriesConfirm : t.deleteRecoveryConfirm;
    if (!window.confirm(message)) return;
    setBusy(true); setError('');
    try {
      const next = all ? await deleteAllRecoveries() : await deleteRecovery(id!);
      setInfo(next); setSelected('');
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const failure = error || info?.status.error;
  const protectedRevision = info?.status.savedRevision === document.revision;
  const statusText = document.dirty ? protectedRevision && !info?.status.pending ? t.recoverySaved : t.recoveryPending : t.recoveryReady;
  return <div className="recovery-controls">
    {info && !failure && <p className="recovery-state" role="status" title={statusText}>{statusText}</p>}
    {dismissed && candidates.length > 0 && <button className="recovery-reopen" onClick={() => setDismissed(false)}>{t.recoveryFound} ({candidates.length})</button>}
    {failure && <div className="recovery-warning" role="alert"><span>{t.recoveryFailed}</span><button disabled={busy} onClick={() => void run(false)}>{t.retryRecovery}</button><details><summary>{t.recoveryDetails}</summary><p>{failure}</p></details></div>}
    {!dismissed && candidates.length > 0 && <section className="recovery-banner" aria-label={t.recoveryFound}>
      <div><strong>{t.recoveryFound}</strong><p>{t.recoveryHint}</p></div>
      <select aria-label={t.recoveryCopy} value={id} disabled={busy} onChange={event => setSelected(event.target.value)}>
        {candidates.map((candidate, index) => <option key={candidate.id} value={candidate.id}>{index + 1} · {candidate.format === 'tiled' ? t.recoveryTiled : t.recoveryLegacy} · {new Date(candidate.modifiedMs).toLocaleString(locale)}</option>)}
      </select>
      <button disabled={busy} onClick={() => void run(true)}>{t.restoreRecovery}</button>
      <button disabled={busy} onClick={() => void remove(false)}>{t.deleteRecovery}</button>
      {candidates.length > 1 && <button disabled={busy} onClick={() => void remove(true)}>{t.deleteAllRecoveries}</button>}
      <button disabled={busy} onClick={() => setDismissed(true)}>{t.recoveryLater}</button>
    </section>}
  </div>;
}
