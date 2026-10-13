import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { GuideAttentionSettings as Settings } from '../../engine/contracts';
import './guide-updates.css';

export function GuideAttentionSettings() {
  const { guideAttention, client, isConnected, refresh } = useLocalEngine();
  const [draft, setDraft] = useState<Settings>();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const gate = useRef(false);
  const pending = useRef<{ settings: Settings; id: string } | undefined>(undefined);
  if (!guideAttention) return null;
  const saved = guideAttention.settings;
  const value = draft || saved;
  function edit(change: Partial<Settings>) {
    setDraft({ ...value, ...change });
    pending.current = undefined;
    setNotice('');
  }
  async function save() {
    if (gate.current || !draft || !isConnected) return;
    gate.current = true;
    setBusy(true);
    setNotice('');
    pending.current ||= { settings: draft, id: crypto.randomUUID() };
    try {
      await client.saveGuideAttention(pending.current.settings, pending.current.id);
      pending.current = undefined;
      setDraft(undefined);
      await refresh();
      setNotice('Guide update settings saved.');
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Could not save Guide settings.');
    } finally {
      gate.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="guide-attention-settings" aria-label="Automatic Guide updates">
      <div className="workspace-setting-row">
        <div>
          <h3>Let the Guide keep an eye on work</h3>
          <p>
            Assess important changes while your engine is running. It can explain and suggest; it
            cannot start or change work on its own.
          </p>
        </div>
        <label className="workspace-theme-toggle">
          <input
            type="checkbox"
            role="switch"
            aria-label="Automatic Guide updates"
            disabled={!isConnected || busy}
            checked={value.enabled}
            onChange={(e) =>
              edit({
                enabled: e.target.checked,
                daily_tokens: value.daily_tokens || 24000,
                assessment_tokens: value.assessment_tokens || 8000,
              })
            }
          />
          <span aria-hidden="true" />
          {value.enabled ? 'On' : 'Off'}
        </label>
      </div>
      {value.enabled && (
        <div className="guide-attention-limits">
          <label>
            Daily assessment allowance
            <input
              type="number"
              min={0}
              max={200000}
              step={1000}
              disabled={busy}
              value={value.daily_tokens}
              onChange={(e) => edit({ daily_tokens: Number(e.target.value) })}
            />
          </label>
          <label>
            Maximum per assessment
            <input
              type="number"
              min={1024}
              max={32000}
              step={1000}
              disabled={busy}
              value={value.assessment_tokens}
              onChange={(e) => edit({ assessment_tokens: Number(e.target.value) })}
            />
          </label>
          <p>
            Tokens, including input and output. {guideAttention.reserved_today.toLocaleString()}{' '}
            reserved today (UTC). Each assessment reserves its full allowance, even if interrupted.
            This is not a billing cap.
          </p>
        </div>
      )}
      {draft && (
        <div className="guide-update-actions">
          <button type="button" disabled={!isConnected || busy} onClick={() => void save()}>
            {busy ? 'Saving…' : 'Save Guide settings'}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setDraft(undefined);
              pending.current = undefined;
            }}
          >
            Discard changes
          </button>
        </div>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
