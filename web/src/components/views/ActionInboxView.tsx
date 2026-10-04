import { useRef, useState } from 'react';
import { Check, ChevronLeft, ChevronRight, ArrowLeft } from 'lucide-react';
import { Agent, ApprovalRequest } from '../../types';
import { teammateName } from '../../lib/teammates';
import { Portrait } from '../ui/Portrait';
interface Props {
  approvals: ApprovalRequest[];
  agents?: Agent[];
  initialSelectedId?: string;
  onApprove: (id: string) => void | Promise<void>;
  onReject: (id: string) => void | Promise<void>;
  showHistory?: boolean;
}

export function ActionInboxView({
  approvals,
  agents = [],
  initialSelectedId,
  onApprove,
  onReject,
  showHistory = false,
}: Props) {
  const inFlight = useRef(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [selected, setSelected] = useState(initialSelectedId || ''),
    [receipt, setReceipt] = useState<ApprovalRequest | null>(null),
    [history, setHistory] = useState(showHistory);
  const pending = approvals.filter((a) => a.status === 'pending'),
    reviewed = approvals.filter((a) => a.status !== 'pending');
  const visible = history ? reviewed : pending,
    active = visible.find((a) => a.id === selected) || visible[0];

  async function decide(approved: boolean) {
    if (
      !active ||
      active.status !== 'pending' ||
      inFlight.current ||
      (approved && active.effectUnavailable)
    )
      return;
    inFlight.current = true;
    setSaving(true);
    setError('');
    try {
      await (approved ? onApprove(active.id) : onReject(active.id));
      setReceipt({ ...active, status: approved ? 'approved' : 'rejected' });
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : 'The decision could not be confirmed. Retry to check its status.',
      );
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  }
  if (receipt)
    return (
      <div className="decision-receipt">
        <span className="receipt-mark">
          <Check size={26} />
        </span>
        <h2>{receipt.status === 'approved' ? 'Approved.' : 'Declined.'}</h2>
        <p>{teammateName(receipt.agentId, receipt.agentName)}’s request has your decision.</p>
        <span className="preview-footnote">
          {receipt.source === 'engine'
            ? 'Decision confirmed by the engine.'
            : 'Saved in this preview. No engine action was sent.'}
        </span>
        {pending.length > 0 && (
          <button
            className="canvas-primary"
            onClick={() => {
              setReceipt(null);
              setSelected('');
            }}
          >
            Next request <ChevronRight size={17} />
          </button>
        )}
        <button
          className="text-action"
          onClick={() => {
            setHistory(true);
            setSelected(receipt.id);
            setReceipt(null);
          }}
        >
          See decision
        </button>
      </div>
    );
  if (!active)
    return (
      <div className="decision-receipt">
        <span className="receipt-mark">
          <Check size={26} />
        </span>
        <h2>{history ? 'No past decisions.' : 'No pending approvals.'}</h2>
        <p>
          {history
            ? 'Decisions will stay here for this session.'
            : 'Other work may still need review in Needs attention.'}
        </p>
        <button className="text-action" onClick={() => setHistory(!history)}>
          {history ? 'Current requests' : 'Past decisions'}
        </button>
      </div>
    );
  const agent = agents.find((a) => a.id === active.agentId),
    name = teammateName(active.agentId, active.agentName),
    index = visible.findIndex((a) => a.id === active.id);
  const remote =
    active.type === 'cross_castle_request'
      ? active.payload.match(/->\s*castle\.([\w-]+)\s*\[Run:\s*([^\]]+)\]/)
      : null;
  const title =
    active.type === 'effect'
      ? 'Review the proposed action'
      : active.type === 'bash_command'
        ? active.payload.startsWith('cargo test')
          ? 'Run the project tests?'
          : 'Run this command?'
        : active.type === 'file_write'
          ? 'Apply these file changes?'
          : active.type === 'cross_castle_request'
            ? 'Allow work on your machine?'
            : 'Allow this network request?';
  return (
    <div className="human-decision" key={active.id}>
      <div className="decision-topline">
        {history ? (
          <button className="quiet-back" onClick={() => setHistory(false)}>
            <ArrowLeft size={15} />
            Current requests
          </button>
        ) : (
          <span>Needs your decision</span>
        )}
        <div>
          <button
            aria-label="Previous request"
            disabled={saving || index === 0}
            onClick={() => setSelected(visible[index - 1].id)}
          >
            <ChevronLeft size={16} />
          </button>
          <span>
            {index + 1} of {visible.length}
          </span>
          <button
            aria-label="Next request"
            disabled={saving || index === visible.length - 1}
            onClick={() => setSelected(visible[index + 1].id)}
          >
            <ChevronRight size={16} />
          </button>
        </div>
      </div>
      <div className="decision-person">
        {agent && <Portrait agent={agent} size={48} />}
        <span>{name} is asking</span>
      </div>
      <h2>{title}</h2>
      <p className="decision-why">{active.reason}</p>
      <div className="decision-scope">
        <span>
          {active.type === 'file_write'
            ? 'File to change'
            : active.type === 'cross_castle_request'
              ? 'Requested access'
              : 'Requested action'}
        </span>
        <code>{remote ? remote[2] : active.payload}</code>
        {remote && (
          <small>On {remote[1].charAt(0).toUpperCase() + remote[1].slice(1)}’s workstation</small>
        )}
        {active.diff && <small>{active.diff.summary}</small>}
        <p>From {active.castleOrigin}</p>
      </div>
      <details className="decision-evidence">
        <summary>{active.diff ? 'Inspect changes & details' : 'More details'}</summary>
        <p>{active.title}</p>
        {remote && (
          <pre className="code-block" tabIndex={0}>
            {active.payload}
          </pre>
        )}
        <p className="preview-footnote">
          Request {active.id} ·{' '}
          {active.source === 'engine'
            ? `Expires ${new Date((active.expiresAt || 0) * 1000).toLocaleString()}`
            : `sample expiry ${active.expiresInSecs}s, not a live countdown.`}
        </p>
        {active.diff && (
          <div className="diff-block" tabIndex={0} aria-label="Proposed code diff">
            {active.diff.oldLines.map((l, i) => (
              <pre className="diff-removed" key={'o' + i}>
                − {l.num} {l.text}
              </pre>
            ))}
            {active.diff.newLines.map((l, i) => (
              <pre className="diff-added" key={'n' + i}>
                + {l.num} {l.text}
              </pre>
            ))}
          </div>
        )}
      </details>
      {error && (
        <p className="local-notice" role="alert">
          {error}
        </p>
      )}
      {saving && <p role="status">Waiting for confirmation…</p>}
      {active.status === 'pending' ? (
        <div className="decision-actions">
          <button disabled={saving} className="canvas-secondary" onClick={() => void decide(false)}>
            Decline
          </button>
          <button
            disabled={saving || active.effectUnavailable}
            className="canvas-primary"
            onClick={() => void decide(true)}
          >
            Approve this request <Check size={17} />
          </button>
        </div>
      ) : (
        <p className="decision-saved">Decision: {active.status}</p>
      )}
      <div className="decision-bottom">
        <span>
          {active.source === 'engine' ? 'This engine request only' : 'Preview · this request only'}
        </span>
        {!history && (
          <button
            disabled={saving}
            onClick={() => {
              setHistory(true);
              setSelected('');
            }}
          >
            Past decisions
          </button>
        )}
      </div>
    </div>
  );
}
