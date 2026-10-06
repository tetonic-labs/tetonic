export type WorkKind = 'assignment' | 'responsibility' | 'response';
export type WorkStatus =
  | 'draft'
  | 'active'
  | 'needs_input'
  | 'watching'
  | 'paused'
  | 'awaiting_ack'
  | 'review'
  | 'done';
export interface WorkOption {
  id: string;
  title: string;
  consequence: string;
  recommended?: boolean;
  acknowledgment: string;
  result: string;
}
export interface WorkDecision {
  question: string;
  whyYou: string;
  known: string;
  unknown: string;
  fallback: string;
  options: WorkOption[];
}
export interface WorkReceipt {
  choice: string;
  note: string;
  phase: 'recorded' | 'acknowledged' | 'reported';
  acknowledgment?: string;
  result?: string;
}
export interface WorkItem {
  id: string;
  title: string;
  context: string;
  kind: WorkKind;
  status: WorkStatus;
  intent: string;
  success: string;
  boundary: string;
  trigger: string;
  teamId: string;
  agentIds: string[];
  leadId: string;
  summary: string;
  next: string;
  cadence: string;
  notes: string[];
  clarifications: string[];
  milestones: { title: string; detail: string; state: 'done' | 'current' | 'next' }[];
  evidence: { title: string; text: string }[];
  decision?: WorkDecision;
  receipt?: WorkReceipt;
  fixture: boolean;
}
