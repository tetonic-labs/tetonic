import { useId, useState } from 'react';
import type { WorkBrief } from '../../engine/contracts';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';

/** An authorized saved brief, not a generated UI summary or an execution plan. */
export function GuideUnderstanding({ brief, onEdit }: { brief: WorkBrief; onEdit?: () => void }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <section className="guide-understanding" aria-label="Working understanding">
      <div className="guide-understanding-heading">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen(!open)}
        >
          Our working understanding
          <span>{open ? 'Hide' : 'View'}</span>
        </button>
        {onEdit && (
          <button type="button" onClick={onEdit}>
            Edit
          </button>
        )}
      </div>
      {open && (
        <div id={id}>
          <FormattedMarkdown text={brief.body} />
          <small>Still a discussion. Tell the Guide what to change, or edit these notes.</small>
        </div>
      )}
    </section>
  );
}
