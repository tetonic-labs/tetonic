import { type LocalApproval } from '../engine/contracts';
import { approvalFor } from './workSignals';
import { needsHelp, type WorkRecord } from '../engine/projections/records';

export function attentionItems(records: WorkRecord[], approvals: LocalApproval[]) {
  const permissions = approvals.filter((a) => a.status === 'pending');
  const questions = records.flatMap((work) =>
    work.latest?.state === 'waiting_human'
      ? (work.latest.human_questions || [])
          .filter((q) => !q.answer)
          .map((question) => ({ work, question }))
      : [],
  );
  const problems = records.filter(
    (work) =>
      needsHelp(work) &&
      !approvalFor(work, permissions) &&
      !questions.some((q) => q.work.id === work.id),
  );
  return {
    permissions,
    questions,
    problems,
    total: permissions.length + questions.length + problems.length,
  };
}
