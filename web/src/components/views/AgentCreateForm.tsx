import { useState } from 'react';
import { ArrowLeft, Plus, Search, Cpu, Plug, Terminal, Database } from 'lucide-react';
import type { Team } from '../../types';
import type { WorkspaceResource } from '../../lib/toolLibrary';
import {
  agentTools,
  availableAgentResources,
  configurationForTeam,
  defaultAgentConfiguration,
  harnesses,
  type AgentDraft,
} from '../../lib/agentConfiguration';
import { AgentAdvancedSettings } from './AgentAdvancedSettings';
import type { AgentCatalog, ProviderModelCatalog } from '../../lib/localEngine';
import { AgentProviderKey } from './AgentProviderKey';
import { AgentModelSelect } from './AgentModelSelect';

export function AgentCreateForm({
  teams,
  currentTeamId,
  resources,
  models,
  defaultModel,
  onCreate,
  onBack,
  connected,
}: {
  teams: Team[];
  currentTeamId: string;
  resources: WorkspaceResource[];
  models: string[];
  defaultModel: string;
  onCreate: (draft: AgentDraft) => void;
  onBack?: () => void;
  connected?: {
    catalog: AgentCatalog;
    saving: boolean;
    error: string;
    onSaveKey?: (provider: string, key: string) => Promise<void>;
    onRemoveKey?: (provider: string) => Promise<void>;
    onDiscoverModels?: (provider: string, signal?: AbortSignal) => Promise<ProviderModelCatalog>;
    connectionRevision?: number;
  };
}) {
  const [name, setName] = useState(''),
    [purpose, setPurpose] = useState('');
  const [teamId, setTeamId] = useState(
    teams.some((team) => team.id === currentTeamId) ? currentTeamId : '',
  );
  const [model, setModel] = useState(''),
    [customModel, setCustomModel] = useState('');
  const [provider, setProvider] = useState('ollama');
  const [hostedConsent, setHostedConsent] = useState(false);
  const [hostedToolsConsent, setHostedToolsConsent] = useState(false);
  const hosted = provider !== 'ollama';
  const lab = connected?.catalog.providers?.find((value) => value.id === provider);
  const [configuration, setConfiguration] = useState(() => {
    const value = defaultAgentConfiguration();
    if (connected)
      value.limits = {
        maxSteps: connected.catalog.max_steps,
        maxSeconds: connected.catalog.max_seconds,
        maxTokens: connected.catalog.max_tokens,
      };
    return value;
  });
  const [query, setQuery] = useState(''),
    [notice, setNotice] = useState('');
  const available = availableAgentResources(resources, teamId);
  const matching = available.filter((resource) =>
    `${resource.name} ${resource.description}`.toLowerCase().includes(query.toLowerCase().trim()),
  );
  const resolvedModel =
    model === 'custom' ? customModel.trim() : model || (hosted ? '' : defaultModel);
  const connectedToolGroups: Record<string, string[]> = {
    read_file: ['read_file', 'list_dir', 'grep', 'glob'],
    write_file: ['write_file', 'edit_file'],
  };
  const runtimeProfile = connected?.catalog.runtime_profiles?.find(
    (profile) => profile.provider === provider && profile.harness === configuration.harness,
  );
  const supportedTools = runtimeProfile?.tools ?? (hosted ? [] : (connected?.catalog.tools ?? []));
  const requiresToolConsent =
    hosted && !!runtimeProfile?.requires_tool_consent && configuration.toolIds.length > 0;
  const providerReady =
    !hosted || (!!lab?.key_saved && hostedConsent && (!requiresToolConsent || hostedToolsConsent));
  const tools = connected
    ? agentTools.filter((tool) =>
        connectedToolGroups[tool.id]?.every((id) => connected.catalog.tools?.includes(id)),
      )
    : agentTools;
  const incompatibleTools = connected
    ? configuration.toolIds.filter(
        (id) => !connectedToolGroups[id]?.every((tool) => supportedTools.includes(tool)),
      )
    : [];
  const compatibilityIssue =
    connected?.catalog.runtime_profiles && !runtimeProfile
      ? 'This provider and harness cannot run together on this engine.'
      : incompatibleTools.length
        ? runtimeProfile?.tool_restriction ||
          'The selected tools are unavailable with this provider. Choose another provider or remove them.'
        : '';
  const choices = [...new Set([defaultModel, ...models])].filter(
    (name) => name && name !== 'Not connected' && name !== 'Workspace default',
  );
  function changeTeam(id: string) {
    const next = configurationForTeam(configuration, resources, id);
    setNotice(
      next.resourceIds.length !== configuration.resourceIds.length
        ? 'Tool selections outside this team’s toolkit were cleared.'
        : '',
    );
    setConfiguration(next);
    setTeamId(id);
    setQuery('');
  }
  return (
    <form
      className="simple-form agent-create-form"
      onInvalid={(event) => {
        const disclosure = (event.target as HTMLElement).closest('details');
        if (disclosure) disclosure.open = true;
      }}
      onSubmit={(event) => {
        event.preventDefault();
        if (
          !name.trim() ||
          !resolvedModel ||
          !providerReady ||
          compatibilityIssue ||
          connected?.saving
        )
          return;
        onCreate({
          ...(connected
            ? {
                provider,
                hostedConsent,
                hostedToolsConsent: requiresToolConsent && hostedToolsConsent,
              }
            : {}),
          name: name.trim(),
          purpose: purpose.trim(),
          teamId,
          model: resolvedModel,
          configuration: configurationForTeam(
            {
              ...configuration,
              scope: {
                ...configuration.scope,
                workspacePath: configuration.scope.workspacePath.trim(),
              },
            },
            resources,
            teamId,
          ),
        });
      }}
    >
      {onBack && (
        <button type="button" className="quiet-back" onClick={onBack}>
          <ArrowLeft size={16} />
          {connected ? 'Back to work' : 'Agents'}
        </button>
      )}
      <header className="agent-create-heading">
        <h2>
          A new <i>teammate.</i>
        </h2>
        <p>Give them a purpose. Choose how they work.</p>
      </header>
      <div className="agent-create-basics">
        <div className="agent-identity-fields">
          <label>
            Name
            <input
              autoFocus
              required
              maxLength={60}
              placeholder="What should we call them?"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label>
            What will they help with?
            <textarea
              rows={2}
              maxLength={4000}
              placeholder="Research ideas, improve our project…"
              value={purpose}
              onChange={(event) => setPurpose(event.target.value)}
            />
          </label>
          <label>
            Team
            <select value={teamId} onChange={(event) => changeTeam(event.target.value)}>
              {!connected && <option value="">No team yet</option>}
              {teams.map((team) => (
                <option key={team.id} value={team.id}>
                  {team.name}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="agent-runtime-fields">
          <span className="agent-runtime-title">
            <Cpu size={17} />
            How they work
          </span>
          {!!connected?.catalog.providers?.length && (
            <label>
              Model provider
              <select
                value={provider}
                disabled={connected.saving}
                onChange={(event) => {
                  setProvider(event.target.value);
                  setModel('');
                  setCustomModel('');
                  setHostedConsent(false);
                  setHostedToolsConsent(false);
                }}
              >
                <option value="ollama">On this machine · Ollama</option>
                {connected.catalog.providers.map((value) => (
                  <option key={value.id} value={value.id}>
                    {value.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <AgentModelSelect
            key={`models-${provider}`}
            provider={provider}
            keySaved={!!lab?.key_saved}
            connectionRevision={connected?.connectionRevision}
            discover={connected?.onDiscoverModels}
            choices={choices}
            defaultModel={defaultModel}
            model={model}
            customModel={customModel}
            onModel={setModel}
            onCustomModel={setCustomModel}
            connected={!!connected}
          />
          {hosted && lab && connected?.onSaveKey && (
            <AgentProviderKey
              key={provider}
              provider={lab}
              onSave={connected.onSaveKey}
              onRemove={connected.onRemoveKey}
            />
          )}
          {hosted && (
            <>
              <label className="agent-hosted-consent">
                <input
                  type="checkbox"
                  checked={hostedConsent}
                  onChange={(event) => setHostedConsent(event.target.checked)}
                />
                <span>
                  Allow this agent’s instructions, prompts, and conversation history to be sent to{' '}
                  {lab?.name}. Provider usage charges apply.
                </span>
              </label>
              <p>
                {runtimeProfile?.tools.length
                  ? 'This profile can also read files you explicitly allow below.'
                  : 'This profile works with your prompts and conversation only.'}
              </p>
              <p>Choose a text model with tool calling available to your provider account.</p>
            </>
          )}
          {!hosted && connected?.catalog.local_error && (
            <p role="status">{connected.catalog.local_error}</p>
          )}
          <label>
            Harness
            <select
              value={configuration.harness}
              onChange={(event) =>
                setConfiguration({
                  ...configuration,
                  harness: event.target.value as 'general' | 'coding',
                })
              }
            >
              {harnesses
                .filter((harness) => !connected || connected.catalog.harnesses.includes(harness.id))
                .map((harness) => (
                  <option key={harness.id} value={harness.id}>
                    {harness.name}
                  </option>
                ))}
            </select>
          </label>
          <p>
            {connected
              ? 'Tetonic runs the agent on your machine using the model you choose.'
              : harnesses.find((harness) => harness.id === configuration.harness)?.description}
          </p>
          {!connected && (
            <span className="agent-runtime-caption">
              Model = intelligence. Harness = how it works.
            </span>
          )}
        </div>
      </div>
      <fieldset className="agent-tool-picker">
        <legend>
          Tools{' '}
          <span>{configuration.toolIds.length + configuration.resourceIds.length} selected</span>
        </legend>
        <div className="agent-tool-grid">
          {tools.map((tool) => (
            <label
              className="agent-tool-choice"
              key={tool.id}
              data-selected={configuration.toolIds.includes(tool.id)}
            >
              <input
                type="checkbox"
                aria-label={tool.name}
                checked={configuration.toolIds.includes(tool.id)}
                disabled={
                  !!connected &&
                  !configuration.toolIds.includes(tool.id) &&
                  !connectedToolGroups[tool.id]?.every((id) => supportedTools.includes(id))
                }
                onChange={(event) =>
                  setConfiguration({
                    ...configuration,
                    toolIds: event.target.checked
                      ? [...configuration.toolIds, tool.id]
                      : configuration.toolIds.filter((id) => id !== tool.id),
                  })
                }
              />
              <span>
                <strong>{tool.name}</strong>
                <small>{tool.description}</small>
              </span>
            </label>
          ))}
        </div>
        {connected && (
          <p className="agent-field-note">
            {hosted
              ? 'Workspace tools need a supported execution profile. Your selections are kept when you change providers.'
              : tools.length
                ? 'Only selected tools are granted, within the engine’s configured folder.'
                : 'No workspace tools are available for this provider and host.'}
          </p>
        )}
        {compatibilityIssue && (
          <p className="local-notice" role="alert">
            {compatibilityIssue}
          </p>
        )}
        {requiresToolConsent && !compatibilityIssue && (
          <label className="agent-hosted-consent">
            <input
              type="checkbox"
              checked={hostedToolsConsent}
              onChange={(event) => setHostedToolsConsent(event.target.checked)}
            />
            <span>
              Allow selected file results from{' '}
              {connected?.catalog.workspace_root || 'the engine’s configured folder'} to be sent to{' '}
              {lab?.name}. Files outside this folder and unselected tools stay unavailable.
            </span>
          </label>
        )}
        {!connected && (
          <details className="agent-resource-picker">
            <summary>
              Workspace tools & MCPs{' '}
              <span>
                {configuration.resourceIds.length
                  ? `${configuration.resourceIds.length} selected`
                  : `${available.length} available`}
              </span>
            </summary>
            <p className="agent-field-note">
              {teamId
                ? 'From the selected team’s toolkit.'
                : 'Unassigned resources. Choose a team to see its toolkit.'}{' '}
              Setup drafts still need connecting.
            </p>
            {available.length > 4 && (
              <label className="agent-tool-search">
                <Search size={15} aria-hidden="true" />
                <input
                  type="search"
                  aria-label="Find tools and MCPs"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="Find a tool or MCP…"
                />
              </label>
            )}
            <div className="agent-resource-options">
              {matching.map((resource) => {
                const Icon =
                  resource.kind === 'mcp'
                    ? Plug
                    : resource.kind === 'storage'
                      ? Database
                      : Terminal;
                return (
                  <label
                    className="agent-tool-choice"
                    key={resource.id}
                    data-selected={configuration.resourceIds.includes(resource.id)}
                  >
                    <input
                      type="checkbox"
                      aria-label={resource.name}
                      checked={configuration.resourceIds.includes(resource.id)}
                      onChange={(event) =>
                        setConfiguration({
                          ...configuration,
                          resourceIds: event.target.checked
                            ? [...configuration.resourceIds, resource.id]
                            : configuration.resourceIds.filter((id) => id !== resource.id),
                        })
                      }
                    />
                    <Icon size={16} aria-hidden="true" />
                    <span>
                      <strong>{resource.name}</strong>
                      <small>
                        {resource.kind === 'mcp'
                          ? 'MCP'
                          : resource.kind === 'storage'
                            ? 'Storage'
                            : 'Tool'}{' '}
                        · {resource.source === 'draft' ? 'Setup draft' : 'Preview resource'}
                      </small>
                    </span>
                  </label>
                );
              })}
              {!matching.length && (
                <p className="agent-field-note">
                  {query
                    ? 'No matching resources.'
                    : 'No resources assigned here yet. Add or assign them in Tools & MCPs.'}
                </p>
              )}
            </div>
          </details>
        )}
        {notice && (
          <p className="agent-field-note" role="status">
            {notice}
          </p>
        )}
      </fieldset>
      <AgentAdvancedSettings
        value={configuration}
        onChange={setConfiguration}
        hasTeam={!!teamId}
        enforcedLimits={
          connected
            ? {
                maxSteps: connected.catalog.max_steps,
                maxSeconds: connected.catalog.max_seconds,
                maxTokens: connected.catalog.max_tokens,
              }
            : undefined
        }
      />
      {connected?.error && (
        <p className="local-notice" role="alert">
          {connected.error}
        </p>
      )}
      <footer className="agent-create-footer">
        <p className="preview-footnote">
          {connected ? (
            'Agents and their work are saved in your local engine.'
          ) : (
            <>
              Preview configuration.
              <br />
              Model, tools, and access aren’t connected yet.
            </>
          )}
        </p>
        <button
          type="submit"
          className="canvas-primary"
          disabled={
            !name.trim() ||
            !resolvedModel ||
            !providerReady ||
            !!compatibilityIssue ||
            connected?.saving
          }
        >
          {connected?.saving ? 'Saving…' : 'Create agent'} <Plus size={16} />
        </button>
      </footer>
    </form>
  );
}
