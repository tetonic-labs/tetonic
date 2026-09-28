import type { WorkItem, WorkReceipt } from '../types/workroom';

export const kindLabels = {
  assignment: 'Assignment',
  responsibility: 'Ongoing responsibility',
  response: 'Event response',
};
export const statusLabels = {
  draft: 'Shaping',
  active: 'In motion',
  needs_input: 'Your judgment',
  watching: 'Watching',
  paused: 'Paused',
  awaiting_ack: 'Awaiting acknowledgment',
  review: 'Ready to review',
};
export const needsJudgment = (item: WorkItem) =>
  item.status === 'needs_input' || item.status === 'review';
export type WorkAction =
  | { type: 'add'; item: WorkItem }
  | { type: 'edit'; id: string; patch: Partial<WorkItem> }
  | { type: 'decide'; id: string; optionId: string; note: string }
  | { type: 'advance'; id: string }
  | { type: 'dispatch'; id: string }
  | { type: 'pause'; id: string };

export function workroomReducer(items: WorkItem[], action: WorkAction): WorkItem[] {
  if (action.type === 'add') return [...items, action.item];
  return items.map((item) => {
    if (item.id !== action.id) return item;
    if (action.type === 'edit') return { ...item, ...action.patch };
    if (action.type === 'decide') {
      if (item.status !== 'needs_input' || item.receipt) return item;
      const option = item.decision?.options.find((o) => o.id === action.optionId);
      if (!option) return item;
      const receipt: WorkReceipt = {
        choice: option.title,
        note: action.note.trim(),
        phase: 'recorded',
        acknowledgment: option.acknowledgment,
        result: option.result,
      };
      return {
        ...item,
        receipt,
        status: 'awaiting_ack',
        summary: `Direction recorded: ${option.title}. The team has not acknowledged it yet.`,
        next: 'Await acknowledgment before assuming the plan changed.',
      };
    }
    if (action.type === 'advance') {
      if (!item.fixture || !item.receipt || item.receipt.phase === 'reported') return item;
      if (item.receipt.phase === 'recorded')
        return {
          ...item,
          status: 'active',
          receipt: { ...item.receipt, phase: 'acknowledged' },
          summary: item.receipt.acknowledgment!,
          next: 'Review the reported result at the next checkpoint.',
        };
      return {
        ...item,
        status: 'review',
        receipt: { ...item.receipt, phase: 'reported' },
        summary: item.receipt.result!,
        next: 'Inspect the sample result. A report is not independent verification.',
      };
    }
    if (action.type === 'pause') {
      if (item.kind !== 'responsibility' || !['watching', 'paused'].includes(item.status))
        return item;
      return { ...item, status: item.status === 'paused' ? 'watching' : 'paused' };
    }
    if (action.type === 'dispatch') {
      if (
        item.status !== 'draft' ||
        !item.title.trim() ||
        !item.intent.trim() ||
        !item.success.trim() ||
        !item.boundary.trim() ||
        !item.leadId ||
        !item.agentIds.includes(item.leadId) ||
        (item.kind !== 'assignment' && !item.trigger.trim())
      )
        return item;
      return {
        ...item,
        status: 'awaiting_ack',
        receipt: { choice: 'Work brief submitted', note: '', phase: 'recorded' },
        summary: 'Brief saved for this team. No engine is connected to accept or run it.',
        next: 'Await team acceptance. No execution has started.',
      };
    }
    return item;
  });
}
export function newWork(id: string, intent: string, context: string): WorkItem {
  return {
    id,
    title: intent.trim().slice(0, 76),
    context: context === 'all' ? 'General' : context,
    kind: 'assignment',
    status: 'draft',
    intent: intent.trim(),
    success:
      'Bring back a proposed approach, the important open questions, and a useful first step for review.',
    boundary:
      'Research, organize, and draft independently. Ask before spending money, publishing, contacting anyone, or changing connected systems.',
    trigger: '',
    teamId: '',
    agentIds: [],
    leadId: '',
    summary: 'A thought to shape into work.',
    next: 'Define a useful result and choose a lead.',
    cadence: 'At a meaningful checkpoint',
    notes: [],
    clarifications: [],
    milestones: [],
    evidence: [],
    fixture: false,
  };
}
