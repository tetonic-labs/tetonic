import { useEffect, useId, useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { LocalApproval } from '../../lib/localEngine';

export function Decision({
  approval,
  onWork,
  agentName,
  workTitle,
  onResolved,
}: {
  approval: LocalApproval;
  onWork?: (id: string) => void;
  agentName?: string;
  workTitle?: string;
  onResolved?: (receipt: LocalApproval) => void;
}) {
  const { resolveApproval, isConnected, readErrors } = useLocalEngine();
  const locked = useRef(false);
  const [busy, setBusy] = useState<'allow' | 'decline' | null>(null);
  const [error, setError] = useState('');
  const [receipt, setReceipt] = useState<LocalApproval | null>(null);
  const [now, setNow] = useState(Date.now());
  const heading = useId();
  const card = useRef<HTMLElement>(null);
  const receiptHeading = useRef<HTMLHeadingElement>(null);
  const focusReceipt = useRef(false);
  useEffect(() => {
    if (receipt && focusReceipt.current) {
      receiptHeading.current?.focus();
      focusReceipt.current = false;
    }
  }, [receipt]);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const proposal = approval.proposal;
  const resolved =
    receipt || (['approved', 'rejected'].includes(approval.status) ? approval : null);
  const expired = approval.expires_at * 1000 <= now;
  const available = isConnected && !readErrors.Decisions;
  const canDecide = available && !expired && approval.status === 'pending' && !resolved;
  async function decide(allow: boolean) {
    if (
      locked.current ||
      !canDecide ||
      (allow && !proposal) ||
      approval.expires_at * 1000 <= Date.now()
    )
      return;
    locked.current = true;
    setBusy(allow ? 'allow' : 'decline');
    setError('');
    try {
      const result = await resolveApproval(approval.approval_id, allow, approval.proposal_digest);
      if (
        result.approval_id !== approval.approval_id ||
        result.proposal_digest !== approval.proposal_digest ||
        result.org_id !== approval.org_id ||
        result.team_id !== approval.team_id ||
        result.status !== (allow ? 'approved' : 'rejected')
      )
        throw new Error('Your decision was not confirmed. Refresh to check this request.');
      focusReceipt.current = !!card.current?.contains(document.activeElement);
      setReceipt(result);
      onResolved?.(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Your decision could not be confirmed.');
    } finally {
      locked.current = false;
      setBusy(null);
    }
  }
  return (
    <article
      ref={card}
      className="tw-decision"
      aria-labelledby={heading}
      data-resolved={!!resolved}
    >
      <div className="tw-request-context">
        <span>{agentName ? `${agentName} · Permission` : 'Permission request'}</span>
        {workTitle && <p>{workTitle}</p>}
      </div>
      <h3 id={heading} ref={receiptHeading} tabIndex={-1}>
        {resolved
          ? resolved.status === 'approved'
            ? 'Command approved once'
            : 'Request declined'
          : proposal
            ? 'Allow this command?'
            : 'Action details are missing'}
      </h3>
      {resolved ? (
        <p role="status">
          {resolved.status === 'approved'
            ? 'The engine accepted your permission for this attempt. This is not confirmation that the command ran.'
            : 'The engine confirmed your decision to decline this command.'}
        </p>
      ) : (
        <>
          {proposal ? (
            <>
              <p className="tw-request-scope">For this exact command, on this attempt only.</p>
              <pre className="tw-request-command" tabIndex={0} aria-label="Exact command to review">
                <code>{proposal.command}</code>
              </pre>
              <dl className="tw-request-location">
                <div>
                  <dt>Working folder</dt>
                  <dd>{proposal.working_directory}</dd>
                </div>
                <div>
                  <dt>Shell</dt>
                  <dd>{proposal.shell}</dd>
                </div>
              </dl>
              {!!proposal.confinement_warnings.length && (
                <div role="note" className="tw-request-limits">
                  <strong>Isolation limits</strong>
                  <p>This command can act outside the working folder.</p>
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
          {(!available || expired || approval.status !== 'pending') && (
            <p className="tw-request-unavailable" role="status">
              {!available
                ? 'Reconnect and refresh decisions before responding.'
                : expired
                  ? 'This permission request has expired. Review the work for its current state.'
                  : 'This request is no longer awaiting a decision.'}
            </p>
          )}
          <div className="tw-decision-actions">
            <button
              disabled={!proposal || !canDecide || !!busy || expired}
              title={
                proposal
                  ? 'Allow this exact command once'
                  : 'The exact action must be available to review'
              }
              onClick={() => void decide(true)}
            >
              {busy === 'allow' ? 'Confirming…' : proposal ? 'Allow once' : 'Approve unavailable'}
            </button>
            <button disabled={!canDecide || !!busy} onClick={() => void decide(false)}>
              {busy === 'decline' ? 'Declining…' : 'Decline request'}
            </button>
          </div>
        </>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="tw-request-footer">
        {approval.work_id && onWork && (
          <button className="operator-secondary" onClick={() => onWork(approval.work_id!)}>
            Open related work
          </button>
        )}
        <details>
          <summary>Request details</summary>
          <p>Reference: {approval.approval_id}</p>
          <p>Proposal: {approval.proposal_digest}</p>
          <p>Expires: {new Date(approval.expires_at * 1000).toLocaleString()}</p>
          {resolved && proposal && (
            <pre className="tw-request-command">
              <code>{proposal.command}</code>
            </pre>
          )}
        </details>
      </div>
    </article>
  );
}
