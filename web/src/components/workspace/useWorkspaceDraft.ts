import { useEffect, useRef, useState } from 'react';
import { EngineRequestError } from '../../engine/failure';
import { type EngineTask, type WorkTeamSelection } from '../../engine/contracts';

interface PendingSend {
  id: string;
  input: string;
  agent: string;
  parent?: string;
  purpose?: 'work' | 'explore';
  workTeam?: WorkTeamSelection;
}
interface Draft {
  text: string;
  pending?: PendingSend;
  error?: string;
  editable?: boolean;
}
type Drafts = Record<string, Draft>;

function read(key: string): Drafts {
  try {
    const value = JSON.parse(sessionStorage.getItem(key) || '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(
      Object.entries(value).filter(([, draft]) => {
        const d = draft as Draft;
        return (
          d &&
          typeof d.text === 'string' &&
          (!d.pending ||
            (typeof d.pending.id === 'string' &&
              typeof d.pending.input === 'string' &&
              typeof d.pending.agent === 'string' &&
              (!d.pending.purpose || ['work', 'explore'].includes(d.pending.purpose)) &&
              (!d.pending.parent || typeof d.pending.parent === 'string') &&
              (!d.pending.workTeam ||
                (typeof d.pending.workTeam.id === 'string' &&
                  Number.isInteger(d.pending.workTeam.revision) &&
                  d.pending.workTeam.revision > 0))))
        );
      }),
    ) as Drafts;
  } catch {
    return {};
  }
}

// Only unsent text and retry identity live in session storage. Accepted work
// always comes from the engine. Never carry drafts into another destination.
export function useWorkspaceDraft(scope: string) {
  const storageKey = `tetonic_workspace_drafts:${scope}`;
  const [drafts, setDrafts] = useState<Drafts>(() => read(storageKey));
  const data = useRef(drafts);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [storageWarning, setStorageWarning] = useState(false);
  function put(key: string, value: Draft) {
    const next = { ...data.current, [key]: value };
    data.current = next;
    try {
      sessionStorage.setItem(storageKey, JSON.stringify(next));
    } catch {
      if (mounted.current) setStorageWarning(true);
    }
    if (mounted.current) setDrafts(next);
  }
  async function send(
    key: string,
    agent: string,
    parent: string | undefined,
    submit: (
      input: string,
      agent: string,
      parent: string | undefined,
      id: string,
      purpose?: 'work' | 'explore',
      workTeam?: WorkTeamSelection,
    ) => Promise<EngineTask>,
    onAccepted: (task: EngineTask) => void,
    purpose?: 'work' | 'explore',
    workTeam?: WorkTeamSelection,
  ) {
    const draft = data.current[key] || { text: '' };
    if (inFlight.current || !draft.text.trim() || !agent) return;
    const pending = draft.pending || {
      id: crypto.randomUUID(),
      input: draft.text.trim(),
      agent,
      parent,
      purpose,
      workTeam,
    };
    inFlight.current = true;
    setBusyKey(key);
    put(key, { ...draft, pending, error: undefined });
    try {
      const task = pending.workTeam
        ? await submit(
            pending.input,
            pending.agent,
            pending.parent,
            pending.id,
            pending.purpose,
            pending.workTeam,
          )
        : await submit(pending.input, pending.agent, pending.parent, pending.id, pending.purpose);
      put(key, { text: '' });
      if (mounted.current) onAccepted(task);
    } catch (error) {
      // A 4xx can follow a saved-but-not-started request. Preserve its identity
      // on unchanged retries; editing after a definite rejection starts a new
      // request. Uncertain sends remain immutable until reconciled.
      const rejected =
        error instanceof EngineRequestError &&
        [400, 401, 403, 404, 409, 422].includes(error.status);
      put(key, {
        ...draft,
        pending,
        editable: rejected,
        error: error instanceof Error ? error.message : 'We couldn’t confirm your request.',
      });
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusyKey(null);
    }
  }
  return {
    drafts,
    busyKey,
    storageWarning,
    send,
    edit: (key: string, text: string) => {
      const draft = data.current[key];
      if (!draft?.pending || draft.editable)
        put(key, {
          text,
          ...(draft?.pending?.input === text.trim()
            ? { pending: draft.pending, editable: true }
            : {}),
        });
    },
  };
}
