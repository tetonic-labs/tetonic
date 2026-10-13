import { useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import './guide-updates.css';

export function GuideUpdates({
  onWork,
  compact = false,
}: {
  onWork?: (id: string) => void;
  compact?: boolean;
}) {
  const { guideAttention, client, isConnected, readErrors, refresh } = useLocalEngine();
  const [open, setOpen] = useState(!compact);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [limit, setLimit] = useState(5);
  const [error, setError] = useState('');
  const items = guideAttention?.items || [];
  const unread = items.filter((item) => !item.seen && item.state !== 'resolved').length;
  if (!items.length && !readErrors['Guide updates']) return null;
  async function read(key: string) {
    try {
      await client.seeGuideUpdate(key);
      await refresh();
      setError('');
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Could not mark this update read.');
    }
  }
  return (
    <section className="guide-updates" aria-label="Guide updates">
      {compact ? (
        <button
          className="guide-updates-toggle"
          type="button"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          Updates{unread ? ` · ${unread} new` : ''}
          <span>{open ? 'Hide' : 'View'}</span>
        </button>
      ) : (
        <h3>Updates from your work</h3>
      )}
      {open && (
        <>
          {!isConnected && (
            <p role="status">
              Last known updates. The engine must be running to notice new developments.
            </p>
          )}
          {readErrors['Guide updates'] && (
            <p role="status">Could not refresh updates. These may have changed.</p>
          )}
          {isConnected &&
            guideAttention?.observed_at &&
            Date.now() - Date.parse(guideAttention.observed_at) > 60000 && (
              <p role="status">
                The Guide’s update check is behind. Inspect work for its current state.
              </p>
            )}
          {guideAttention && !guideAttention.settings.enabled && (
            <p>Recorded changes only. Automatic Guide assessments are off in workspace settings.</p>
          )}
          {guideAttention?.settings.enabled &&
            guideAttention.reserved_today + guideAttention.settings.assessment_tokens >
              guideAttention.settings.daily_tokens && (
              <p>
                Automatic assessments have reached today’s allowance. Engine changes still appear
                here.
              </p>
            )}
          {guideAttention?.partial && (
            <p>
              Showing up to 100 updates, with pending changes first. Full work history is in the
              inspector.
            </p>
          )}
          <div className="guide-updates-list">
            {items.slice(0, limit).map((item) => {
              const text = item.update || item.signal.detail;
              const more = text.length > 440;
              const expandedItem = expanded.includes(item.signal.key);
              return (
                <article
                  key={item.signal.key}
                  data-kind={item.signal.kind}
                  data-seen={item.seen}
                  data-resolved={item.state === 'resolved'}
                >
                  <div className="guide-update-heading">
                    <strong>{item.signal.title}</strong>
                    <span>
                      {item.state === 'resolved'
                        ? 'Situation changed'
                        : item.signal.kind === 'result'
                          ? 'Result available'
                          : item.signal.kind === 'budget'
                            ? 'Usage approaching limit'
                            : 'Needs a look'}
                    </span>
                  </div>
                  <small>
                    {item.update
                      ? 'The Guide’s assessment'
                      : item.state === 'assessing'
                        ? 'Guide is checking · recorded change below'
                        : 'Recorded engine change'}
                    {item.state === 'unavailable' ? ' · not assessed automatically' : ''}
                  </small>
                  <FormattedMarkdown
                    text={expandedItem || !more ? text : `${text.slice(0, 440)}…`}
                  />
                  <div className="guide-update-actions">
                    {more && (
                      <button
                        type="button"
                        aria-expanded={expandedItem}
                        onClick={() =>
                          setExpanded((old) =>
                            expandedItem
                              ? old.filter((k) => k !== item.signal.key)
                              : [...old, item.signal.key],
                          )
                        }
                      >
                        {expandedItem ? 'Show less' : 'Read more'}
                      </button>
                    )}
                    {onWork && (
                      <button type="button" onClick={() => onWork(item.signal.work_id)}>
                        Inspect work
                      </button>
                    )}
                    {!item.seen && (
                      <button
                        type="button"
                        disabled={!isConnected}
                        onClick={() => void read(item.signal.key)}
                      >
                        Mark read
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
          {items.length > limit && (
            <button type="button" onClick={() => setLimit((n) => n + 5)}>
              Show earlier updates
            </button>
          )}
          {error && <p role="alert">{error}</p>}
        </>
      )}
    </section>
  );
}
