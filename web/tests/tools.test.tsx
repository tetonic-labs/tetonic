import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '../src/App';
import { endpointError, resourcesFromGraph } from '../src/lib/toolLibrary';
import { mockGraphNodes, mockGraphEdges } from '../src/store/graphMockData';
import { mockTeams } from '../src/store/mockData';

describe('workspace tools', () => {
  it('includes unassigned resources and keeps shared connections as one identity', () => {
    const resources = resourcesFromGraph(mockGraphNodes, mockGraphEdges, mockTeams);
    expect(resources.length).toBe(
      mockGraphNodes.filter((n) => ['connector', 'tool', 'storage'].includes(n.type)).length,
    );
    expect(new Set(resources.map((r) => r.id)).size).toBe(resources.length);
    const unassigned = resourcesFromGraph(mockGraphNodes, [], mockTeams);
    expect(unassigned.every((r) => !r.teamIds.length)).toBe(true);
    expect(resources.find((r) => r.id === 'mcp-github')?.catalogId).toBe('github');
  });
  it.each([
    'javascript:alert(1)',
    'https://user:secret@example.com/mcp',
    'http://example.com/mcp',
    'https://example.com/mcp?token=secret',
    'bad url',
  ])('rejects unsuitable endpoint %s', (value) => expect(endpointError(value)).not.toBe(''));
  it.each(['https://example.com/mcp', 'http://localhost:3000/mcp', 'http://127.0.0.1:3000/mcp'])(
    'accepts remote HTTPS and local endpoint %s',
    (value) => expect(endpointError(value)).toBe(''),
  );
  it('creates, edits, scopes, and removes a draft without pretending it is a connected map resource', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    const world = container.querySelector('.universe-world') as HTMLElement;
    const camera = world.style.transform;
    await user.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Add tool or MCP' }));
    await user.type(screen.getByRole('textbox', { name: 'Search tools and MCPs' }), 'Custom');
    await user.click(screen.getByRole('button', { name: /Custom MCP/ }));
    await user.type(
      screen.getByRole('textbox', { name: 'Name', exact: true }),
      'Knowledge archive',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Server URL' }),
      'https://knowledge.example.com/mcp',
    );
    await user.click(screen.getByRole('button', { name: 'Save setup' }));
    const details = screen.getByRole('complementary', {
      name: 'Knowledge archive resource details',
    });
    expect(within(details).getByText('Setup draft · not connected')).toBeTruthy();
    expect(container.querySelectorAll('[data-entity^="resource-"]')).toHaveLength(0);
    await user.click(within(details).getByRole('checkbox', { name: 'Platform Core Guild' }));
    await user.click(within(details).getByRole('button', { name: 'Edit setup' }));
    await user.clear(screen.getByRole('textbox', { name: 'Name', exact: true }));
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'Shared knowledge');
    await user.click(screen.getByRole('button', { name: 'Save setup' }));
    await user.click(screen.getByRole('button', { name: 'Close window' }));
    expect(world.style.transform).toBe(camera);
    await user.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    await user.type(
      screen.getByRole('textbox', { name: 'Search tools and MCPs' }),
      'Shared knowledge',
    );
    await user.click(screen.getByRole('button', { name: 'Manage Shared knowledge' }));
    expect(screen.getByRole('checkbox', { name: 'Platform Core Guild' })).toHaveProperty(
      'checked',
      true,
    );
    expect(screen.getByText('https://knowledge.example.com/mcp')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Remove draft' }));
    expect(screen.queryByRole('button', { name: 'Manage Shared knowledge' })).toBeNull();
  });
  it('opens the toolkit scoped to the room and connects existing resources back to recorded activity', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Open Platform Core Guild conversation' }));
    await user.click(screen.getByRole('button', { name: 'Team tools' }));
    expect(screen.getByRole('combobox', { name: 'Filter resources by team' })).toHaveProperty(
      'value',
      'team-platform',
    );
    await user.click(screen.getByRole('button', { name: 'Manage GitHub MCP Connector' }));
    await user.click(screen.getByRole('button', { name: 'View recorded activity' }));
    expect(screen.getByRole('dialog', { name: 'GitHub MCP Connector' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Manage in Tools & MCPs' }));
    expect(
      screen.getByRole('complementary', { name: 'GitHub MCP Connector resource details' }),
    ).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Close window' }));
    expect(screen.getByRole('region', { name: 'Platform Core Guild team room' })).toBeTruthy();
  });
  it('adds a runtime tool without a server form and carries the chosen team into setup', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Filter resources by team' }),
      'personal',
    );
    await user.click(screen.getByRole('button', { name: 'Add tool or MCP' }));
    await user.type(screen.getByRole('textbox', { name: 'Search tools and MCPs' }), 'Terminal');
    await user.click(screen.getByRole('button', { name: /Runtime tool Terminal/ }));
    expect(screen.queryByRole('textbox', { name: 'Server URL' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Save setup' }));
    const detail = screen.getByRole('complementary', { name: 'Terminal resource details' });
    expect(within(detail).getByRole('checkbox', { name: 'Personal Fleet' })).toHaveProperty(
      'checked',
      true,
    );
    expect(within(detail).getByText('Setup draft · not connected')).toBeTruthy();
  });
});
