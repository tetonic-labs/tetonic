import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { SkillDetails } from '../src/components/views/SkillDetails';
import { WorkspaceCapabilityLibrary } from '../src/components/views/WorkspaceCapabilityLibrary';
import { agentSetup } from '../src/lib/agentCapabilities';
import { LocalEngine } from '../src/engine/client';
import { type AgentCatalog, type EngineAgent, type WorkspaceSkill } from '../src/engine/contracts';

afterEach(() => vi.restoreAllMocks());
const skill: WorkspaceSkill = {
  id: 'skill_research_version1',
  name: 'research',
  description: 'Gather evidence',
  source: 'Written here',
  enabled: true,
  created_at: '2026-10-08',
};
const agent: EngineAgent = {
  id: 'stable',
  key: 'researcher',
  name: 'Robin',
  purpose: 'Help me research',
  model: 'local-model',
  provider: 'ollama',
  harness: 'general',
  tools: [],
  max_steps: 3,
  max_seconds: 120,
  max_tokens: 1000,
  editable: true,
  definition_digest: 'v1',
};
function catalog(skills: WorkspaceSkill[] = []): AgentCatalog {
  const tools = skills.filter((s) => s.enabled).map((s) => s.id);
  return {
    skills,
    models: ['local-model'],
    harnesses: ['general'],
    tools,
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
    runtime_profiles: ['ollama', 'openai'].map((provider) => ({
      provider,
      harness: 'general',
      tools,
      tool_restriction: null,
      requires_tool_consent: provider !== 'ollama',
    })),
    providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
  };
}

it('adds a workspace skill from the agent editor without losing the draft or implicitly granting it', async () => {
  const client = new LocalEngine('fixture');
  let skills: WorkspaceSkill[] = [];
  vi.spyOn(client, 'agentCatalog').mockImplementation(async () => catalog(skills));
  const add = vi.spyOn(client, 'importSkill').mockImplementation(async () => {
    skills = [skill];
    return skill;
  });
  const update = vi
    .spyOn(client, 'updateAgent')
    .mockImplementation(async (old, input) => ({ ...old, ...input, definition_digest: 'v2' }));
  render(
    <LocalAgentSetup
      client={client}
      agent={agent}
      workspace={{
        organization: 'Workspace',
        team_id: 'team',
        team_name: 'Team',
        agent_id: agent.id,
        agent_name: agent.name,
        model: agent.model,
        input_limit: 12000,
        agents: [agent],
        shaping_agent_key: 'guide',
        tasks: [],
      }}
      onCreated={vi.fn()}
      onBack={vi.fn()}
    />,
  );
  fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
    target: { value: 'Robin with edits' },
  });
  fireEvent.click(screen.getByText('Add tools, MCPs or skills to the workspace'));
  fireEvent.click(screen.getByRole('button', { name: /Write a skill/ }));
  await userEvent.type(screen.getByLabelText('Skill name'), 'research{Enter}');
  expect(update).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText('When should an agent use it?'), {
    target: { value: 'Gather evidence' },
  });
  fireEvent.change(screen.getByLabelText('Instructions'), { target: { value: 'Check sources.' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add skill to workspace' }));
  const checkbox = await screen.findByRole('checkbox', { name: /research Gather evidence/ });
  expect(checkbox).toHaveProperty('checked', false);
  expect(screen.getByLabelText('Name', { exact: true })).toHaveProperty(
    'value',
    'Robin with edits',
  );
  expect(add).toHaveBeenCalledWith({
    content: '---\nname: "research"\ndescription: "Gather evidence"\n---\n\nCheck sources.\n',
    source: 'Written in this workspace',
  });
  expect(update).not.toHaveBeenCalled();
  fireEvent.click(checkbox);
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(update).toHaveBeenCalledOnce());
  expect(update.mock.calls[0][1]).toMatchObject({ name: 'Robin with edits', tools: [skill.id] });
});

it('shows revoked skills as a repairable agent issue without inventing a workspace folder requirement', () => {
  const hosted = {
    ...agent,
    provider: 'openai',
    hosted_consent: true,
    tools: [skill.id],
    tool_disclosure: {
      version: 1,
      provider: 'openai',
      endpoint: 'https://api.openai.com/v1/chat/completions',
      tools: [skill.id],
      workspace: null,
    },
  };
  expect(agentSetup(hosted, catalog([skill]), true).state).toBe('configured');
  expect(agentSetup(hosted, catalog([{ ...skill, enabled: false }]), true)).toMatchObject({
    state: 'needs_setup',
    message: expect.stringContaining('skill was revoked'),
  });
});

it('requires an explicit revocation and displays reviewed content as text', async () => {
  const client = new LocalEngine('fixture');
  vi.spyOn(client, 'skillContent').mockResolvedValue({
    content: '<script>not executed</script>\nSkill instructions',
  });
  const revoke = vi.spyOn(client, 'revokeSkill').mockResolvedValue([{ ...skill, enabled: false }]);
  const changed = vi.fn().mockResolvedValue(undefined);
  render(<SkillDetails skill={skill} client={client} onChanged={changed} />);
  await screen.findByText(/not executed/);
  expect(revoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText('Revoke workspace access'));
  fireEvent.click(screen.getByRole('button', { name: 'Revoke this skill' }));
  await waitFor(() => expect(changed).toHaveBeenCalledOnce());
  expect(revoke).toHaveBeenCalledWith(skill.id);
});

it('keeps a skill draft when returning to the library and restores keyboard focus', async () => {
  const user = userEvent.setup();
  const client = new LocalEngine('fixture');
  const add = vi.spyOn(client, 'importSkill');
  render(<WorkspaceCapabilityLibrary client={client} supported onChanged={vi.fn()} />);
  await user.click(screen.getByRole('button', { name: /Write a skill/ }));
  expect(document.activeElement).toBe(screen.getByLabelText('Skill name'));
  await user.type(screen.getByLabelText('Skill name'), 'research-sources');
  expect(screen.queryByRole('button', { name: /Import a skill/ })).toBeNull();
  await user.click(screen.getByRole('button', { name: 'All abilities' }));
  await waitFor(() =>
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /Write a skill/ })),
  );
  await user.click(screen.getByRole('button', { name: /Write a skill/ }));
  expect(screen.getByLabelText('Skill name')).toHaveProperty('value', 'research-sources');
  expect(add).not.toHaveBeenCalled();
});

it('reveals pasted skill content for review, preserves it on failure, and allows retry', async () => {
  const user = userEvent.setup();
  const client = new LocalEngine('fixture');
  const add = vi
    .spyOn(client, 'importSkill')
    .mockRejectedValueOnce(new Error('Engine connection lost'))
    .mockResolvedValueOnce(skill);
  const changed = vi.fn().mockResolvedValue(undefined);
  render(<WorkspaceCapabilityLibrary client={client} supported onChanged={changed} />);
  await user.click(screen.getByRole('button', { name: /Import a skill/ }));
  expect(screen.queryByLabelText('Review instructions')).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Or paste instructions' }));
  const content = '---\nname: research\ndescription: Gather evidence\n---\nCheck sources.';
  fireEvent.change(screen.getByLabelText('Review instructions'), { target: { value: content } });
  expect(add).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Add skill to workspace' }));
  expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'Engine connection lost');
  expect(screen.getByLabelText('Review instructions')).toHaveProperty('value', content);
  expect(changed).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Add skill to workspace' }));
  await screen.findByText(/Saved to the workspace/);
  expect(add).toHaveBeenLastCalledWith({ content, source: 'Imported SKILL.md' });
  expect(changed).toHaveBeenCalledOnce();
});
