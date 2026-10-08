import { useRef, useState } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { EngineTask, LocalApproval } from '../../lib/localEngine';
import { Decision } from './Decision';

/** Same engine requests in the inbox and work. Receipts here are presentation only. */
export function ApprovalRequests({
  workIds,
  tasks = [],
  onWork,
}: {
  workIds?: string[];
  tasks?: EngineTask[];
  onWork?: (id: string) => void;
}) {
  const engine = useLocalEngine();
  const [receipts, setReceipts] = useState<LocalApproval[]>([]);
  const order = useRef<string[]>([]);
  const identity = (a: LocalApproval) => `${a.approval_id}:${a.proposal_digest}`;
  const inScope = (a: LocalApproval) => !workIds || (!!a.work_id && workIds.includes(a.work_id));
  const pending = (engine.approvals?.pending_approvals || []).filter(
    (a) => a.status === 'pending' && inScope(a),
  );
  const requests = new Map(pending.map((a) => [identity(a), a]));
  for (const receipt of receipts.filter(inScope)) requests.set(identity(receipt), receipt);
  for (const id of requests.keys()) if (!order.current.includes(id)) order.current.push(id);
  const allTasks = [...tasks, ...(engine.workspace?.tasks || [])];
  return (
    <div className="tw-approval-requests">
      {order.current
        .filter((id) => requests.has(id))
        .map((id) => {
          const approval = requests.get(id)!;
          const task = allTasks.find((t) => t.id === approval.work_id);
          return (
            <Decision
              key={id}
              approval={approval}
              agentName={task?.agent_name}
              workTitle={task?.plan?.title || task?.input.split('\n')[0]}
              onWork={onWork}
              onResolved={(receipt) =>
                setReceipts((old) => [...old.filter((a) => identity(a) !== id), receipt])
              }
            />
          );
        })}
    </div>
  );
}
