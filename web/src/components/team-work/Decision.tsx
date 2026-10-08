import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { LocalApproval } from '../../lib/localEngine';

export function Decision({
  approval,
  onWork,
}: {
  approval: LocalApproval;
  onWork: (id: string) => void;
}) {
  const { resolveApproval, isConnected, readErrors } = useLocalEngine();
  const locked = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [receipt, setReceipt] = useState<string | null>(null);
  const proposal = approval.proposal;
  async function decide(allow: boolean) {
    if (locked.current) return;
    locked.current = true;
    setBusy(true);
    setError('');
    try {
      const result = await resolveApproval(approval.approval_id, allow, approval.proposal_digest);
      if (
        result.approval_id !== approval.approval_id ||
        result.proposal_digest !== approval.proposal_digest ||
        result.status !== (allow ? 'approved' : 'rejected')
      )
        throw new Error('Your decision was not confirmed. Refresh to check this request.');
      setReceipt(allow ? 'Command approved once' : 'Request declined');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Your decision could not be confirmed.');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  return (
    <article className="tw-decision">
      <h3>{receipt || (proposal ? 'Run this command?' : 'An action needs your permission')}</h3>
      {receipt ? (
        <p>The engine confirmed your decision.</p>
      ) : (
        <>
          {proposal ? (
            <>
              <p>This permission is for this command, on this attempt only.</p>
              <pre style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
                <code>{proposal.command}</code>
              </pre>
              <p>
                In <strong>{proposal.working_directory}</strong> · {proposal.shell}
              </p>
              {!!proposal.confinement_warnings.length && (
                <div role="note">
                  <strong>Limits of this computer’s isolation</strong>
                  <p>
                    The working folder is not a security boundary for this command. Review these
                    limits before allowing it.
                  </p>
                  <ul>
                    {proposal.confinement_warnings.map((warning) => (
                      <li key={warning}>{warning}</li>
                    ))}
                  </ul>
                </div>
              )}
            </>
          ) : (
            <p>
              The proposed action hasn’t been included with this request. You can’t approve it until
              its effects are available to review.
            </p>
          )}
          {approval.work_id && (
            <button onClick={() => onWork(approval.work_id!)}>Open related work</button>
          )}
          <details>
            <summary>Request details</summary>
            <p>Reference: {approval.approval_id}</p>
            <p>Proposal: {approval.proposal_digest}</p>
            <p>Expires: {new Date(approval.expires_at * 1000).toLocaleString()}</p>
          </details>
          <div className="tw-decision-actions">
            <button
              disabled={
                !proposal ||
                !isConnected ||
                !!readErrors.Decisions ||
                busy ||
                approval.expires_at * 1000 <= Date.now()
              }
              title={
                proposal
                  ? 'Allow this exact command once'
                  : 'The exact action must be available to review'
              }
              onClick={() => void decide(true)}
            >
              {proposal ? 'Allow once' : 'Approve unavailable'}
            </button>
            <button
              disabled={
                !isConnected ||
                !!readErrors.Decisions ||
                busy ||
                approval.expires_at * 1000 <= Date.now()
              }
              onClick={() => void decide(false)}
            >
              {busy ? 'Confirming…' : 'Decline request'}
            </button>
          </div>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </article>
  );
}
