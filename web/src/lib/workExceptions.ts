import type { ApprovalRequest } from '../types';
import type { WorkState } from './workScene';
export function workExceptions(states: Map<string, WorkState>, _approvals: ApprovalRequest[]) {
  return [...states.values()]
    .flatMap((work) => [
      ...work.failures.map((failure) => ({
        id: failure.interaction.id,
        agentId: work.id,
        at: failure.at,
        title: failure.interaction.label,
        target: failure.interaction.targetName,
        state: failure.retryId ? 'Recovery in progress' : 'Unresolved failure',
        explanation: failure.retryId
          ? 'A linked retry is running. Resolution is not yet verified.'
          : 'No recovery is recorded. Inspect the failed operation before deciding what to do.',
      })),
    ])
    .sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
}
