import { useId, useState } from 'react';
import { History } from 'lucide-react';
import type { HuddlePlan } from '../../engine/contracts';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

/** Previous revisions are evidence, never alternate proposals to launch. */
export function ProposalHistory({
  plans,
  currentRevision,
}: {
  plans: HuddlePlan[];
  currentRevision?: number;
}) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const earlier = plans.filter((plan) => plan.revision !== currentRevision);
  if (!earlier.length) return null;
  return (
    <div className="tw-proposal-history">
      <button
        type="button"
        className="px-text-button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
      >
        <History size={14} aria-hidden="true" />
        {open ? 'Hide earlier versions' : `Earlier versions (${earlier.length})`}
      </button>
      {open && (
        <section id={id} aria-label="Earlier proposal versions">
          <p>
            Read-only history. These versions were replaced; they are not queued and cannot be
            started here.
          </p>
          {earlier.map((plan) => (
            <article key={plan.revision}>
              <strong>Version {plan.revision} · replaced</strong>
              <p>{plan.content?.title || 'Unfinished proposal'}</p>
              {plan.content && (
                <details>
                  <summary>Read this version</summary>
                  <FormattedMarkdown text={plan.content.summary} />
                  <ol>
                    {plan.content.assignments.map((assignment) => (
                      <li key={assignment.key}>
                        {assignment.title} · {assignment.agent_key}
                      </li>
                    ))}
                  </ol>
                </details>
              )}
            </article>
          ))}
        </section>
      )}
    </div>
  );
}
