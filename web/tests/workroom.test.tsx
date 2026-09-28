import { describe, it, expect } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '../src/App';
import { mockAgents, mockTeams } from '../src/store/mockData';
import { workroomExamples } from '../src/store/workroomExamples';
import { newWork, workroomReducer } from '../src/lib/workroom';

describe('general-purpose work', () => {
  it('keeps direction, acknowledgment, and reported results distinct and scoped', () => {
    const initial = workroomExamples(mockAgents, mockTeams);
    const result = workroomReducer(initial, {
      type: 'decide',
      id: 'release',
      optionId: 'defer-export',
      note: 'Keep publishing with me.',
    });
    expect(result.find((i) => i.id === 'checkout')).toBe(initial[0]);
    expect(result[1].status).toBe('awaiting_ack');
    expect(result[1].receipt?.phase).toBe('recorded');
    expect(result[1].receipt?.note).toBe('Keep publishing with me.');
    const duplicate = workroomReducer(result, {
      type: 'decide',
      id: 'release',
      optionId: 'extend',
      note: '',
    });
    expect(duplicate[1]).toBe(result[1]);
    const acknowledged = workroomReducer(result, { type: 'advance', id: 'release' });
    expect(acknowledged[1].receipt?.phase).toBe('acknowledged');
    expect(acknowledged[1].status).toBe('active');
    const reported = workroomReducer(acknowledged, { type: 'advance', id: 'release' });
    expect(reported[1].status).toBe('review');
    expect(reported[1].summary).toContain('Nothing has been published');
  });

  it('requires responsibility boundaries, a lead, and a trigger before dispatching a loop', () => {
    const item = {
      ...newWork('mine', 'Keep requests organized', 'Business'),
      kind: 'responsibility' as const,
      success: 'Every request is categorized.',
      leadId: 'agt-builder',
      agentIds: ['agt-builder'],
    };
    expect(workroomReducer([item], { type: 'dispatch', id: 'mine' })[0].status).toBe('draft');
    const withTrigger = { ...item, trigger: 'A new request arrives.' };
    const dispatched = workroomReducer([withTrigger], { type: 'dispatch', id: 'mine' });
    expect(dispatched[0].status).toBe('awaiting_ack');
    expect(workroomReducer(dispatched, { type: 'advance', id: 'mine' })[0]).toBe(dispatched[0]);
    expect(
      workroomReducer([{ ...withTrigger, boundary: '' }], { type: 'dispatch', id: 'mine' })[0]
        .status,
    ).toBe('draft');
  });

  it('preserves context and an unfinished decision when exploring the map', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.selectOptions(screen.getByRole('combobox', { name: 'Work context' }), 'Products');
    expect(screen.queryByRole('button', { name: 'Open Restore checkout reliability' })).toBeNull();
    expect(screen.getByRole('button', { name: /more in other contexts/ })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Open Get Atlas ready for release' }));
    await user.click(screen.getByRole('radio', { name: /Release the core; defer export/ }));
    await user.type(
      screen.getByRole('textbox', { name: /Add a condition or context/ }),
      'Keep the release private.',
    );
    await user.click(screen.getByRole('button', { name: 'Map', exact: true }));
    expect(screen.getByRole('region', { name: /Agent activity map/ })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Work', exact: true }));
    expect(
      (screen.getByRole('textbox', { name: /Add a condition or context/ }) as HTMLTextAreaElement)
        .value,
    ).toBe('Keep the release private.');
    await user.click(screen.getByRole('button', { name: 'Back to work' }));
    expect(
      (screen.getByRole('combobox', { name: 'Work context' }) as HTMLSelectElement).value,
    ).toBe('Products');
    await user.click(screen.getByRole('button', { name: 'Open Get Atlas ready for release' }));
    expect(
      (screen.getByRole('radio', { name: /Release the core; defer export/ }) as HTMLInputElement)
        .checked,
    ).toBe(true);
    await user.click(screen.getByRole('button', { name: 'Record direction' }));
    expect(screen.getByRole('region', { name: 'Direction receipt' }).textContent).toContain(
      'The team has not acknowledged it yet',
    );
    await user.click(screen.getByRole('button', { name: 'Load sample acknowledgment' }));
    await user.click(screen.getByRole('button', { name: 'Load sample result' }));
    expect(screen.getByRole('region', { name: 'Direction receipt' }).textContent).toContain(
      'not been independently verified',
    );
  });

  it('shapes a new task force, keeps brainstorming separate, and records a local handoff', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.type(
      screen.getByRole('textbox', { name: 'What would you like help with?' }),
      'Prepare a workshop',
    );
    await user.click(screen.getByRole('button', { name: 'Shape this work' }));
    expect(
      (screen.getByRole('button', { name: 'Send to team in preview' }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(screen.queryByRole('textbox', { name: 'How will we know it’s useful?' })).toBeNull();
    await user.type(
      screen.getByRole('textbox', { name: 'Add context to the work' }),
      'Keep it under an hour.',
    );
    await user.click(screen.getByRole('button', { name: 'Add context', exact: true }));
    await user.type(
      screen.getByRole('textbox', { name: 'Add a thought' }),
      'Maybe make it interactive.',
    );
    await user.click(screen.getByRole('button', { name: 'Keep this thought' }));
    await user.click(screen.getByRole('button', { name: 'Back to work' }));
    await user.click(screen.getByRole('button', { name: 'Open Prepare a workshop' }));
    expect(screen.getByText('Open question: Maybe make it interactive.')).toBeTruthy();
    expect(screen.getByText('Keep it under an hour.')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Send to team in preview' }));
    expect(screen.getByRole('region', { name: 'Direction receipt' }).textContent).toContain(
      'No engine is connected',
    );
    expect(screen.queryByRole('button', { name: /Load sample/ })).toBeNull();
  });

  it('keeps an investigation request open and makes loop pause an explicit preview operation', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'Open Restore checkout reliability' }));
    await user.click(screen.getByText('I need more context first'));
    await user.type(
      screen.getByRole('textbox', { name: 'What should the team investigate?' }),
      'Check regional differences.',
    );
    await user.click(screen.getByRole('button', { name: 'Save investigation request' }));
    expect(screen.getByRole('region', { name: 'Decision brief' })).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Record direction' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    await user.click(screen.getByRole('button', { name: 'Back to work' }));
    await user.click(screen.getByRole('button', { name: 'Open Keep incoming requests moving' }));
    await user.click(screen.getByRole('button', { name: 'Pause sample loop' }));
    expect(
      screen.getByRole('heading', { name: 'This responsibility is paused in the preview.' }),
    ).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Resume sample loop' }));
    expect(screen.getByRole('button', { name: 'Pause sample loop' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /Needs you/ }));
    expect(
      within(screen.getByRole('region', { name: 'Work decisions' })).getByRole('button', {
        name: /Restore checkout reliability/,
      }),
    ).toBeTruthy();
  });
});
