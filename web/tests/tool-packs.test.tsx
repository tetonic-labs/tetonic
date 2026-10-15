import { useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentToolPacks } from '../src/components/views/AgentToolPacks';
import { ToolPackLibrary } from '../src/components/views/ToolPackLibrary';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { isFolderTool } from '../src/lib/agentCapabilities';
import { LocalEngine } from '../src/engine/client';
import type { AgentCatalog, EngineAgent, ToolPack } from '../src/engine/contracts';
import { changePack, packRequest, packToolLabel } from '../src/lib/toolPacks';

const pack: ToolPack = {
  org_id: 'org',
  team_id: 'team',
  id: 'read',
  revision: 1,
  name: 'Research',
  description: 'Find evidence',
  tools: ['read_file', 'grep'],
  builtin: false,
};
const catalog: AgentCatalog = {
  models: ['local'],
  harnesses: ['general'],
  tools: ['read_file', 'grep', 'list_dir', 'run_shell', 'mcp_calendar_search', 'skill_research'],
  tool_packs: [pack],
  skills: [
    {
      id: 'skill_research',
      name: 'Evidence',
      description: 'Research carefully',
      source: 'Written here',
      enabled: true,
      created_at: '',
    },
  ],
  mcp_connections: [
    {
      id: 'calendar',
      name: 'Calendar',
      endpoint: 'https://example.com',
      status: 'discovered',
      message: 'Ready',
      tools: [
        { id: 'mcp_calendar_search', name: 'Search', description: 'Find events', input_schema: {} },
      ],
    },
  ],
  workspace_root: 'C:/work',
  max_steps: 10,
  max_seconds: 120,
  max_tokens: 4096,
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it('preserves individual extras, exclusions and overlapping packs when changing a version', () => {
  const other = { ...pack, id: 'other', tools: ['read_file', 'list_dir'] };
  const selected = ['read_file', 'list_dir', 'run_shell'];
  expect(packRequest([pack, other], selected)).toEqual({
    tools: ['run_shell'],
    toolPackSelection: {
      packs: [
        { id: 'read', revision: 1 },
        { id: 'other', revision: 1 },
      ],
      excluded_tools: ['grep'],
    },
  });
  const removed = changePack([pack, other], selected, 'read');
  expect(removed.tools).toEqual(['run_shell', 'read_file', 'list_dir']);
  const next = { ...pack, revision: 2, tools: ['grep', 'mcp_calendar_search'] };
  const changed = changePack([pack, other], selected, 'read', next);
  expect(changed.tools).toEqual(['run_shell', 'read_file', 'list_dir', 'mcp_calendar_search']);
  expect(changed.packs.find((p) => p.id === 'read')?.revision).toBe(2);
});

it('keeps pinned packs until the user reviews and applies a new version', () => {
  const updated = { ...pack, revision: 2, tools: ['read_file', 'run_shell'] };
  const change = vi.fn();
  function Picker() {
    const [value, setValue] = useState({ packs: [pack], tools: ['read_file', 'grep'] });
    return (
      <AgentToolPacks
        catalog={{ ...catalog, tool_packs: [updated] }}
        {...value}
        supported={catalog.tools!}
        disabled={false}
        onChange={(packs, tools) => {
          change(packs, tools);
          setValue({ packs, tools });
        }}
      />
    );
  }
  render(<Picker />);
  expect(change).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Review v2' }));
  expect(screen.getByText('Adds: Run a command.')).toBeTruthy();
  expect(screen.getByText('Removes: Search file contents.')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Use this version' }));
  expect(change).toHaveBeenLastCalledWith([updated], ['read_file', 'run_shell']);
});

it('shows unavailable members without silently giving an incomplete pack', () => {
  render(
    <AgentToolPacks
      catalog={catalog}
      packs={[]}
      tools={[]}
      supported={['read_file']}
      disabled={false}
      onChange={vi.fn()}
    />,
  );
  expect(screen.getByRole('checkbox', { name: 'Use Research pack' })).toHaveProperty(
    'disabled',
    true,
  );
  expect(screen.getByText(/Needs access to: Search file contents/)).toBeTruthy();
});

it('saves a custom pack with tools, MCPs and skills and retries without a duplicate', async () => {
  const client = new LocalEngine('fixture');
  const save = vi
    .spyOn(client, 'saveToolPack')
    .mockRejectedValueOnce(new Error('Connection lost'))
    .mockResolvedValue(pack);
  const changed = vi.fn().mockResolvedValue(undefined);
  const outerSubmit = vi.fn((e) => e.preventDefault());
  render(
    <form onSubmit={outerSubmit}>
      <ToolPackLibrary catalog={catalog} client={client} onChanged={changed} />
    </form>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Create pack' }));
  fireEvent.change(screen.getByLabelText('Pack name'), { target: { value: 'My research' } });
  fireEvent.click(screen.getByRole('checkbox', { name: 'Read a file' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Calendar · Search' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Evidence · Skill' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save pack' }));
  await screen.findByText('Connection lost');
  fireEvent.click(screen.getByRole('button', { name: 'Save pack' }));
  await waitFor(() => expect(changed).toHaveBeenCalledOnce());
  expect(save.mock.calls[0][0].tools).toEqual([
    'mcp_calendar_search',
    'read_file',
    'skill_research',
  ]);
  expect(save.mock.calls[1]).toEqual(save.mock.calls[0]);
  expect(outerSubmit).not.toHaveBeenCalled();
});

it('agent editing sends pinned selections plus explicit exclusions and individual additions', async () => {
  const agent: EngineAgent = {
    key: 'robin',
    id: 'id',
    definition_digest: 'v1',
    name: 'Robin',
    purpose: 'Read',
    model: 'local',
    provider: 'ollama',
    harness: 'general',
    max_steps: 3,
    max_seconds: 60,
    max_tokens: 1024,
    tools: [],
  };
  const client = new LocalEngine('fixture');
  vi.spyOn(client, 'agentCatalog').mockResolvedValue(catalog);
  const save = vi.spyOn(client, 'updateAgent').mockResolvedValue(agent);
  render(
    <LocalAgentSetup
      client={client}
      agent={agent}
      workspace={{
        organization: 'Org',
        team_id: 'team',
        team_name: 'Team',
        agent_id: 'id',
        agent_name: 'Robin',
        model: 'local',
        input_limit: 12000,
        agents: [agent],
        shaping_agent_key: 'guide',
        tasks: [],
      }}
      onBack={vi.fn()}
      onCreated={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole('checkbox', { name: 'Use Research pack' }));
  const picker = screen.getByRole('region', { name: 'Agent tool packs' });
  fireEvent.click(within(picker).getByRole('button', { name: 'View tools' }));
  fireEvent.click(within(picker).getByRole('checkbox', { name: 'Search file contents' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Terminal', exact: true }));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(save).toHaveBeenCalledOnce());
  expect(save.mock.calls[0][1]).toMatchObject({
    tools: ['run_shell'],
    tool_pack_selection: { packs: [{ id: 'read', revision: 1 }], excluded_tools: ['grep'] },
  });
});

it('selecting a pack requires fresh hosted tool consent for the full effective selection', async () => {
  const agent: EngineAgent = {
    key: 'robin',
    id: 'id',
    definition_digest: 'v1',
    name: 'Robin',
    purpose: 'Research',
    model: 'account-model',
    provider: 'openai',
    hosted_consent: true,
    harness: 'general',
    max_steps: 3,
    max_seconds: 60,
    max_tokens: 1024,
    tools: ['read_file'],
    tool_disclosure: {
      version: 1,
      provider: 'openai',
      endpoint: 'https://api.openai.com/v1/responses',
      tools: ['read_file'],
      workspace: 'C:/work',
    },
  };
  const client = new LocalEngine('fixture');
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    ...catalog,
    providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
    runtime_profiles: [
      {
        provider: 'openai',
        harness: 'general',
        tools: catalog.tools!,
        requires_tool_consent: true,
        tool_restriction: null,
      },
    ],
  });
  vi.spyOn(client, 'providerModels').mockResolvedValue({
    provider: 'openai',
    models: ['account-model'],
    capabilities_verified: false,
  });
  const save = vi.spyOn(client, 'updateAgent').mockResolvedValue(agent);
  render(
    <LocalAgentSetup
      client={client}
      agent={agent}
      workspace={{
        organization: 'Org',
        team_id: 'team',
        team_name: 'Team',
        agent_id: 'id',
        agent_name: 'Robin',
        model: 'local',
        input_limit: 12000,
        agents: [agent],
        shaping_agent_key: 'guide',
        tasks: [],
      }}
      onBack={vi.fn()}
      onCreated={vi.fn()}
    />,
  );
  const consent = await screen.findByRole('checkbox', {
    name: /Allow selected tool inputs and results/,
  });
  expect(consent).toHaveProperty('checked', true);
  fireEvent.click(screen.getByRole('checkbox', { name: 'Use Research pack' }));
  expect(consent).toHaveProperty('checked', false);
  expect(screen.getByRole('button', { name: 'Save changes' })).toHaveProperty('disabled', true);
  fireEvent.click(consent);
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(save).toHaveBeenCalledOnce());
  expect(save.mock.calls[0][1]).toMatchObject({
    tools: [],
    hosted_tools_consent: true,
    expected_workspace_root: 'C:/work',
    tool_pack_selection: { packs: [{ id: 'read', revision: 1 }], excluded_tools: [] },
  });
});

it('selects Build & verify as exact tools without granting a general terminal', () => {
  const build: ToolPack = {
    ...pack,
    id: 'builtin:build-verify',
    builtin: true,
    name: 'Build & verify',
    description: 'Read, change, inspect and check pending edits.',
    tools: [
      'read_file',
      'read_files',
      'list_dir',
      'grep',
      'glob',
      'write_file',
      'edit_file',
      'inspect_changes',
      'run_checks',
    ],
  };
  const onChange = vi.fn();
  const available = { ...catalog, tools: build.tools, tool_packs: [build] };
  render(
    <AgentToolPacks
      catalog={available}
      packs={[]}
      tools={[]}
      supported={build.tools}
      disabled={false}
      onChange={onChange}
    />,
  );
  fireEvent.click(screen.getByRole('checkbox', { name: 'Use Build & verify pack' }));
  expect(onChange).toHaveBeenCalledWith([build], build.tools);
  expect(packRequest([build], build.tools)).toEqual({
    tools: [],
    toolPackSelection: { packs: [{ id: build.id, revision: 1 }], excluded_tools: [] },
  });
  fireEvent.click(screen.getByRole('button', { name: 'View tools' }));
  for (const tool of ['read_files', 'inspect_changes', 'run_checks']) {
    expect(isFolderTool(tool)).toBe(true);
    expect(screen.getByText(packToolLabel(tool, available))).toBeTruthy();
  }
  expect(onChange.mock.calls[0][1]).not.toContain('run_shell');
});
