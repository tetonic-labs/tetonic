import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HuddleView } from '../src/components/views/HuddleView';
import { ViewErrorBoundary } from '../src/components/ui/ViewErrorBoundary';
import { mockAgents, mockStreamEvents } from '../src/store/mockData';
describe('activity evidence', () => {
  it('labels failed tool results with their actual exit code', async () => {
    const user = userEvent.setup();
    render(
      <HuddleView
        agent={mockAgents[0]}
        agents={mockAgents}
        onSelectAgent={vi.fn()}
        onSendMessage={vi.fn()}
        events={[
          {
            ...mockStreamEvents[3],
            metadata: { exitCode: 7, toolName: 'shell:test', durationMs: 0 },
          },
        ]}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    expect(screen.getByText('Exit 7')).toBeTruthy();
    expect(screen.queryByText('Exit 0')).toBeNull();
    expect(screen.getByText('0 ms')).toBeTruthy();
  });
  it('reports clipboard failures instead of showing false success', async () => {
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValueOnce(new Error('denied'));
    render(
      <HuddleView
        agent={mockAgents[0]}
        agents={mockAgents}
        onSelectAgent={vi.fn()}
        onSendMessage={vi.fn()}
        events={[mockStreamEvents[3]]}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    await user.click(screen.getByRole('button', { name: /Copy tool result/ }));
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      expect.stringContaining('Clipboard access was denied'),
    );
  });
  it('retains the multiline composer and sends only on explicit submission', async () => {
    const user = userEvent.setup();
    const send = vi.fn();
    render(
      <HuddleView
        agent={mockAgents[0]}
        agents={mockAgents}
        events={[]}
        onSendMessage={send}
        onSelectAgent={vi.fn()}
      />,
    );
    await user.type(screen.getByRole('textbox'), 'Investigate{Enter}then report');
    expect(send).not.toHaveBeenCalled();
    await user.keyboard('{Control>}{Enter}{/Control}');
    expect(send).toHaveBeenCalledExactlyOnceWith('Investigate\nthen report');
  });
  it('provides a recovery message if rendering fails', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    function Broken(): never {
      throw new Error('fixture');
    }
    render(
      <ViewErrorBoundary>
        <Broken />
      </ViewErrorBoundary>,
    );
    expect(screen.getByRole('alert')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Retry view' })).toBeTruthy();
    log.mockRestore();
  });
});
