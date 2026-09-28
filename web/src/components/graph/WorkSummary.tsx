import type { WorkState } from '../../lib/workScene';
import { sampleTime, workStatus } from '../../lib/workEvidence';

export function WorkSummary({ work, elapsed }: { work: WorkState | undefined; elapsed: number }) {
  return (
    <section className="work-summary" aria-label="Current sample work">
      <span className="work-provenance">Current playback · {sampleTime(elapsed)} · not live</span>
      <strong>{workStatus(work)}</strong>
      {work?.interaction && (
        <p>
          {work.interaction.label} · {work.interaction.targetName}
        </p>
      )}
      {work?.waiting && (
        <p>Waiting was recorded. The sample does not specify who must unblock it.</p>
      )}
      {work?.failures.map((failure) => (
        <div className="work-summary-failure" key={failure.interaction.id}>
          <strong>
            {failure.retryId ? 'Recovery in progress' : 'Unresolved failure'}:{' '}
            {failure.interaction.targetName}
          </strong>
          <p>{failure.interaction.label}</p>
          <small>
            {failure.retryId
              ? 'A linked retry is running; its result is not yet known.'
              : 'No successful recovery has been recorded for this operation.'}
          </small>
        </div>
      ))}
    </section>
  );
}
