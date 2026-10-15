import { useEffect, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../engine/connection';
import type { EngineTask, WorkDraftView } from '../../engine/contracts';
import './saved-work-drafts.css';

/** Read-only inspection stays beside the interrupted contribution. */
export function SavedWorkDrafts({ task }: { task: EngineTask }) {
  return (
    <>
      {task.saved_drafts?.map((draft) => (
        <SavedDraft
          key={`${connectionDraftScope()}:${task.id}:${draft.artifact_id}`}
          task={task.id}
          draft={draft}
        />
      ))}
    </>
  );
}

function SavedDraft({
  task,
  draft,
}: {
  task: string;
  draft: { artifact_id: string; file_count: number };
}) {
  const { client, isConnected } = useLocalEngine();
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<WorkDraftView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [previous, setPrevious] = useState<number[]>([]);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => {
    controller.current?.abort();
    setView(null);
    setOpen(false);
    setBusy(false);
    setError('');
    return () => {
      controller.current?.abort();
    };
  }, [isConnected, client]);

  async function load(file?: number, offset = 0, history: number[] = []) {
    if (!isConnected) return;
    controller.current?.abort();
    const request = new AbortController();
    controller.current = request;
    setBusy(true);
    setError('');
    try {
      const result = await client.readWorkDraft(
        task,
        draft.artifact_id,
        file,
        offset,
        request.signal,
      );
      if (request.signal.aborted) return;
      if (
        result.artifact_id !== draft.artifact_id ||
        (file !== undefined && (result.page?.file !== file || result.page.offset !== offset))
      )
        throw new Error('The saved file could not be confirmed. Try again.');
      setView(result);
      setPrevious(history);
    } catch (e) {
      if (!request.signal.aborted) {
        setView(null);
        setError(e instanceof Error ? e.message : 'The saved draft could not be loaded.');
      }
    } finally {
      if (!request.signal.aborted) setBusy(false);
    }
  }
  const page = view?.page;
  const file = page ? view?.files[page.file] : undefined;
  return (
    <section className="tw-saved-draft" aria-label="Saved uncommitted draft">
      <div className="tw-saved-draft-heading">
        <div>
          <strong>Unfinished work saved</strong>
          <p>
            {draft.file_count} {draft.file_count === 1 ? 'file' : 'files'} · not applied to your
            folder
          </p>
        </div>
        <button
          aria-expanded={open}
          disabled={!isConnected}
          onClick={() => {
            setOpen(!open);
            if (!open) void load();
            else {
              controller.current?.abort();
              setBusy(false);
            }
          }}
        >
          {open ? 'Close draft' : 'Review saved draft'}
        </button>
      </div>
      {open && (
        <div className="tw-saved-draft-body">
          <p className="tw-small">
            These are pending edits, not a verified result. Reviewing them does not restart work or
            apply changes.
          </p>
          {busy && <p role="status">Reading saved draft…</p>}
          {error && (
            <div role="alert">
              <p>{error}</p>
              <button disabled={busy || !isConnected} onClick={() => void load()}>
                Try reading again
              </button>
            </div>
          )}
          {view && (
            <>
              <ul className="tw-saved-draft-files" aria-label="Saved files">
                {view.files.map((entry, index) => (
                  <li key={index}>
                    <button
                      disabled={busy || !entry.text_available}
                      aria-pressed={page?.file === index}
                      onClick={() => void load(index)}
                    >
                      <span>{entry.path}</span>
                      <small>
                        {entry.operation} · {entry.byte_count.toLocaleString()} bytes
                      </small>
                    </button>
                    {!entry.text_available && (
                      <small>
                        No text preview for this change. Inspect the original work before applying
                        it.
                      </small>
                    )}
                  </li>
                ))}
              </ul>
              {page && file && (
                <div className="tw-saved-draft-text" aria-label={`Draft of ${file.path}`}>
                  <strong>{file.path}</strong>
                  <pre>{page.content || '(Empty file)'}</pre>
                  <div className="tw-saved-draft-pages">
                    <span>
                      {page.offset === 0 && page.next_offset === null
                        ? 'Complete file'
                        : `Bytes ${page.offset.toLocaleString()}–${(page.next_offset ?? file.byte_count).toLocaleString()} of ${file.byte_count.toLocaleString()}`}
                    </span>
                    {!!previous.length && (
                      <button
                        disabled={busy}
                        onClick={() =>
                          void load(page.file, previous.at(-1)!, previous.slice(0, -1))
                        }
                      >
                        Previous section
                      </button>
                    )}
                    {page.next_offset !== null && (
                      <button
                        disabled={busy}
                        onClick={() =>
                          void load(page.file, page.next_offset!, [...previous, page.offset])
                        }
                      >
                        Next section
                      </button>
                    )}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </section>
  );
}
