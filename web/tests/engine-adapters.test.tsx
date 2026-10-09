import { describe, expect, it } from 'vitest';
import { engineApprovalToUI } from '../src/engine/projections/agents';

describe('engine approval projection', () => {
  it('preserves the proposal identity without inventing an effect or an agent', () => {
    const approval = engineApprovalToUI({
      approval_id: 'approval-1',
      org_id: 'org',
      team_id: 'team',
      proposal_digest: 'opaque-digest',
      status: 'pending',
      request_id: 'request-1',
      expires_at: 2000000000,
    });
    expect(approval).toMatchObject({
      source: 'engine',
      proposalDigest: 'opaque-digest',
      effectUnavailable: true,
      agentId: '',
      type: 'effect',
      requestedAt: '',
      expiresAt: 2000000000,
    });
    expect(approval.payload).not.toContain('opaque-digest');
  });
});
