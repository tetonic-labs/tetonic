import { describe, expect, it, vi } from 'vitest';
import { render, screen, within, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AgentCreateForm } from '../src/components/views/AgentCreateForm';

import { mockTeams } from '../src/store/mockData';
import type { WorkspaceResource } from '../src/lib/toolLibrary';

const resources: WorkspaceResource[] = [
  {
    id: 'github-test',
    name: 'Project GitHub',
    description: 'Repository issues',
    kind: 'mcp',
    source: 'sample',
    teamIds: ['team-platform'],
  },
  {
    id: 'notes-test',
    name: 'Personal notes',
    description: 'Notes',
    kind: 'mcp',
    source: 'draft',
    teamIds: ['personal'],
  },
];
function form(currentTeamId = 'team-platform') {
  const onCreate = vi.fn();
  const rendered = render(
    <AgentCreateForm
      teams={mockTeams}
      currentTeamId={currentTeamId}
      resources={resources}
      models={['local-model', 'other-model']}
      defaultModel="local-model"
      onCreate={onCreate}
    />,
  );
  return { ...rendered, onCreate, user: userEvent.setup() };
}
describe('agent configuration', () => {
  it('preserves chosen configuration in the form submission', async () => {
    const { user, onCreate } = form();
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'June');
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model', exact: true }),
      'other-model',
    );
    await user.selectOptions(screen.getByRole('combobox', { name: 'Harness' }), 'coding');
    await user.click(screen.getByRole('checkbox', { name: 'Read files', exact: true }));
    await user.click(screen.getByText('Advanced settings', { exact: true }));
    await user.type(screen.getByRole('textbox', { name: 'Working folder' }), 'C:/work/product');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Shell commands' }), 'deny');
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Steps' }), {
      target: { value: '12' },
    });
    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'June',
        model: 'other-model',
        configuration: expect.objectContaining({
          harness: 'coding',
          toolIds: expect.arrayContaining(['read_file']),
          limits: expect.objectContaining({ maxSteps: 12 }),
        }),
      }),
    );
  });

  it('keeps tool selections scoped when moving between teams and clears team access without a team', async () => {
    const { user, onCreate } = form();
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'June');
    await user.click(screen.getByRole('checkbox', { name: 'Read files', exact: true }));
    await user.click(screen.getByText('Workspace tools & MCPs', { exact: true }));
    await user.click(screen.getByRole('checkbox', { name: 'Project GitHub' }));
    await user.click(screen.getByText('Advanced settings', { exact: true }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Context access' }), 'team');
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Who can request work?' }),
      'team_approval',
    );
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Team', exact: true }),
      'personal',
    );
    expect(screen.queryByRole('checkbox', { name: 'Project GitHub' })).toBeNull();
    expect(screen.getByRole('checkbox', { name: 'Read files', exact: true })).toHaveProperty(
      'checked',
      true,
    );
    expect(screen.getByRole('status').textContent).toContain('were cleared');
    await user.click(screen.getByRole('checkbox', { name: 'Personal notes' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Team', exact: true }), '');
    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        teamId: '',
        configuration: expect.objectContaining({
          toolIds: ['read_file'],
          resourceIds: [],
          scope: { workspacePath: '', context: 'task', requests: 'owner' },
        }),
      }),
    );
  });

  it('supports a custom model without allowing an empty model identifier', async () => {
    const { user, onCreate } = form('all');
    expect(screen.getByRole('combobox', { name: 'Team', exact: true })).toHaveProperty('value', '');
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'June');
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model', exact: true }),
      'custom',
    );
    expect(screen.getByRole('button', { name: 'Create agent', exact: true })).toHaveProperty(
      'disabled',
      true,
    );
    await user.type(screen.getByRole('textbox', { name: 'Model identifier' }), ' my-model:latest ');
    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ model: 'my-model:latest' }));
  });

  it('reveals an invalid advanced limit instead of blocking submission invisibly', async () => {
    const { user, onCreate, container } = form();
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'June');
    await user.click(screen.getByText('Advanced settings', { exact: true }));
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Steps' }), { target: { value: '0' } });
    await user.click(screen.getByText('Advanced settings', { exact: true }));
    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(onCreate).not.toHaveBeenCalled();
    expect(container.querySelector('.agent-advanced')).toHaveProperty('open', true);
  });

  it('requires hosted prompt consent without offering unsupported file tools', async () => {
    const onCreate = vi.fn();
    const user = userEvent.setup();
    render(
      <AgentCreateForm
        teams={mockTeams}
        currentTeamId="team-platform"
        resources={resources}
        models={['local-model']}
        defaultModel="local-model"
        onCreate={onCreate}
        connected={{
          onDiscoverModels: async () => ({
            provider: 'anthropic',
            models: ['claude-sonnet-4-6'],
            capabilities_verified: false,
          }),
          catalog: {
            providers: [{ id: 'anthropic', name: 'Anthropic', key_saved: true }],
            models: ['local-model'],
            harnesses: ['general'],
            max_steps: 10,
            max_seconds: 300,
            max_tokens: 8192,
            tools: ['read_file', 'list_dir', 'grep', 'glob', 'write_file', 'edit_file'],
          },
          saving: false,
          error: '',
        }}
      />,
    );

    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'Claude Assistant');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Model provider' }), 'anthropic');
    await screen.findByRole('option', { name: 'claude-sonnet-4-6', exact: true });
    await user.selectOptions(screen.getByLabelText('Model', { exact: true }), 'claude-sonnet-4-6');
    const generalConsent = screen.getByRole('checkbox', {
      name: /instructions, prompts, and conversation history/,
    });
    expect(screen.getByRole('checkbox', { name: 'Read files', exact: true })).toHaveProperty(
      'disabled',
      true,
    );
    expect(
      screen.queryByRole('checkbox', { name: /use workspace tools and send tool results/ }),
    ).toBeNull();
    expect(screen.getByRole('button', { name: 'Create agent', exact: true })).toHaveProperty(
      'disabled',
      true,
    );

    await user.click(generalConsent);
    expect(screen.getByRole('button', { name: 'Create agent', exact: true })).toHaveProperty(
      'disabled',
      false,
    );

    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: 'anthropic',
        hostedConsent: true,
        configuration: expect.objectContaining({ toolIds: [] }),
      }),
    );
  });
});
