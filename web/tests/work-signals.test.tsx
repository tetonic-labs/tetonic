import { describe, expect, it } from 'vitest';
import { attentionItems } from '../src/lib/attentionItems';
import { combinedSignal, journeySummary, workSignal } from '../src/lib/workSignals';
import { stateLabel, type WorkRecord } from '../src/engine/projections/records';
import {
  type EngineTask,
  type LocalApproval,
  type WorkHumanQuestion,
} from '../src/engine/contracts';

function record(id: string, state: EngineTask['state'], root?: string): WorkRecord {
  const task: EngineTask = {
    id,
    state,
    input: id,
    agent_key: id,
    agent_name: id,
    run_id: null,
    sequence: 1,
    messages: [],
    ...(root
      ? {
          plan: {
            source_work_id: 'shape',
            root_work_id: root,
            assignment_key: id === root ? null : id,
            title: id,
            depends_on: [],
          },
        }
      : {}),
  };
  return { id, title: id, turns: [task], latest: task };
}
const approval: LocalApproval = {
  org_id: 'org',
  team_id: 'team',
  approval_id: 'a',
  work_id: 'child',
  status: 'pending',
  proposal_digest: 'digest',
  request_id: 'r',
  expires_at: 4102444800,
};
describe('work signals across concurrent contributions', () => {
  it('keeps a finished coordinator from concealing running, waiting, or blocked contributions', () => {
    const root = record('root', 'completed', 'root');
    for (const [state, expected] of [
      ['running', 'working'],
      ['not_started', 'waiting'],
      ['failed', 'blocked'],
      ['waiting_human', 'needs_you'],
    ] as const) {
      const summary = journeySummary(root, [root, record('child', state, 'root')]);
      expect(summary.signal).toBe(expected);
      expect(summary.done).toBe(1);
      expect(summary.members).toHaveLength(2);
    }
    expect(journeySummary(root, [root, record('child', 'completed', 'root')]).signal).toBe('done');
  });
  it('prioritizes requests for human input while preserving the number of running contributions', () => {
    const root = record('root', 'running', 'root');
    const summary = journeySummary(
      root,
      [root, record('child', 'running', 'root'), record('other', 'running', 'root')],
      [approval],
    );
    expect(summary).toMatchObject({ signal: 'needs_you', attention: 1, active: 2, done: 0 });
    expect(summary.agents).toEqual(['root', 'child', 'other']);
  });
  it('counts a permission once instead of counting its waiting work as another problem', () => {
    const records = [record('child', 'waiting_human'), record('failure', 'failed')];
    expect(attentionItems(records, [approval])).toMatchObject({
      total: 2,
      problems: [records[1]],
      questions: [],
    });
    expect(attentionItems(records, [{ ...approval, status: 'approved' }]).permissions).toEqual([]);
  });
  it('shows answerable questions instead of a second generic problem card', () => {
    const work = record('child', 'waiting_human');
    const question: WorkHumanQuestion = {
      id: 'q',
      work_id: 'child',
      source_work_id: 'shape',
      attempt_id: 'a',
      content: { question: 'Which audience?', why: 'Shapes the research', options: [] },
      deadline: 4102444800,
      answer: null,
      response_id: null,
    };
    work.latest!.human_questions = [question];
    expect(attentionItems([work], [])).toMatchObject({
      total: 1,
      problems: [],
      questions: [{ work, question }],
    });
    work.latest!.human_questions = [
      { ...question, answer: 'New customers', response_id: 'receipt' },
    ];
    expect(attentionItems([work], [])).toMatchObject({ total: 0, problems: [], questions: [] });
    expect(workSignal(work)).toBe('waiting');
    expect(stateLabel(work)).toBe('Waiting to continue');
    // A second unresolved question or a separate permission remains actionable.
    expect(workSignal(work, [approval])).toBe('needs_you');
    work.latest!.human_questions.push({ ...question, id: 'another' });
    expect(workSignal(work)).toBe('needs_you');
    expect(attentionItems([work], []).questions).toHaveLength(1);
  });
  it('reports real planning activity and does not mark absent or stopped work done', () => {
    const work = record('source', 'completed');
    expect(
      workSignal(work, [], [{ ...record('planner', 'running').latest!, planning_for: 'source' }]),
    ).toBe('working');
    expect(workSignal({ id: 'saved', title: 'Saved', turns: [] })).toBe('waiting');
    expect(workSignal(record('stopped', 'canceled'))).toBe('stopped');
    expect(combinedSignal(['done', 'unknown'])).toBe('unknown');
  });
});
