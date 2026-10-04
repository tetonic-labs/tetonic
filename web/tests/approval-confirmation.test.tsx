import { describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ActionInboxView } from '../src/components/views/ActionInboxView';
import { mockApprovals } from '../src/store/mockData';
import type { ApprovalRequest } from '../src/types';

describe('acknowledged decisions', () => {
  it('sends one decision and waits for acknowledgement before showing its receipt', async () => {
    let acknowledge!: () => void;
    const onApprove = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          acknowledge = resolve;
        }),
    );
    render(
      <ActionInboxView
        approvals={[{ ...mockApprovals[0], source: 'engine' }]}
        onApprove={onApprove}
        onReject={vi.fn()}
      />,
    );
    const approve = screen.getByRole('button', { name: /Approve this request/ });
    fireEvent.click(approve);
    fireEvent.click(approve);
    expect(onApprove).toHaveBeenCalledOnce();
    expect(screen.queryByRole('heading', { name: 'Approved.' })).toBeNull();
    expect(approve).toHaveProperty('disabled', true);
    await act(async () => acknowledge());
    expect(screen.getByRole('heading', { name: 'Approved.' })).toBeTruthy();
    expect(screen.getByText('Decision confirmed by the engine.')).toBeTruthy();
    expect(screen.queryByText(/HMAC/)).toBeNull();
  });

  it('keeps a failed decision unresolved and allows an explicit retry', async () => {
    const onReject = vi
      .fn()
      .mockRejectedValueOnce(new Error('Response lost; reconnect to confirm.'))
      .mockResolvedValue(undefined);
    render(
      <ActionInboxView approvals={[mockApprovals[0]]} onApprove={vi.fn()} onReject={onReject} />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Decline' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Response lost');
    expect(screen.queryByRole('heading', { name: 'Declined.' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Decline' }));
    expect(await screen.findByRole('heading', { name: 'Declined.' })).toBeTruthy();
    expect(onReject).toHaveBeenCalledTimes(2);
  });

  it('does not allow approval when the proposed effect is unavailable', () => {
    const approval: ApprovalRequest = {
      id: 'approval-1',
      source: 'engine',
      agentId: '',
      agentName: 'Your team',
      type: 'effect',
      title: 'Proposed action',
      reason: 'The engine has not supplied inspectable effect details.',
      payload: 'Proposed action details unavailable.',
      proposalDigest: 'opaque-digest',
      effectUnavailable: true,
      castleOrigin: 'Local engine',
      requestedAt: '',
      expiresAt: 2000000000,
      expiresInSecs: 0,
      status: 'pending',
    };
    const onApprove = vi.fn();
    render(<ActionInboxView approvals={[approval]} onApprove={onApprove} onReject={vi.fn()} />);
    expect(screen.getByRole('button', { name: /Approve this request/ })).toHaveProperty(
      'disabled',
      true,
    );
    expect(screen.queryByText('opaque-digest')).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Run this command?' })).toBeNull();
    expect(approval.proposalDigest).toBe('opaque-digest');
    expect(approval.agentId).toBe('');
    expect(onApprove).not.toHaveBeenCalled();
  });
});
