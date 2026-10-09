import { useId, useState, type ReactNode } from 'react';
import { AgentSkills } from './AgentSkills';
import { CapabilityPolicyEditor } from './CapabilityPolicyEditor';
import type { LocalEngine } from '../../engine/client';
import { ArrowLeft, Plus, Search, Cpu, Plug, Terminal, Database, UserRound } from 'lucide-react';
import { Portrait } from '../ui/Portrait';
import { engineAgentToUI } from '../../engine/projections/agents';
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
import {
  type AgentCatalog,
  type EngineAgent,
  type ProviderModelCatalog,
} from '../../engine/contracts';
import { AgentProviderKey } from './AgentProviderKey';
import { AgentModelSelect } from './AgentModelSelect';
import { AgentMcpTools } from './AgentMcpTools';
import { agentToolGroups, isFolderTool, supportedAgentTools, toolDescription } from '../../lib/agentCapabilities';

export function AgentCreateForm({
  teams,
  currentTeamId,
  resources,
  models,
  defaultModel,
  onCreate,
  onBack,
  backLabel,
  connected,
  agent,
  guide = false,
}: {
  teams: Team[];
  currentTeamId: string;
  resources: WorkspaceResource[];
  models: string[];
  defaultModel: string;
  onCreate: (draft: AgentDraft) => void;
  onBack?: () => void;
  backLabel?: string;
  agent?: EngineAgent;
  guide?: boolean;
  connected?: {
    permissionsClient?: LocalEngine;
    catalog: AgentCatalog;
    library?: ReactNode;
    saving: boolean;
    error: string;
    onSaveKey?: (provider: string, key: string) => Promise<void>;
    onRemoveKey?: (provider: string) => Promise<void>;
    onDiscoverModels?: (provider: string, signal?: AbortSignal) => Promise<ProviderModelCatalog>;
    connectionRevision?: number;
    refreshing?: boolean;
    onRefresh?: () => void;
    onDiscoverMcp?: (id: string) => Promise<void>;
  };
}) {
  const saveHintId = useId();
  const [name, setName] = useState(agent?.name || ''),
    [purpose, setPurpose] = useState(agent?.purpose || '');
  const [teamId, setTeamId] = useState(
    teams.some((team) => team.id === currentTeamId) ? currentTeamId : '',
  );
  const [model, setModel] = useState(agent?.model || ''),
    [customModel, setCustomModel] = useState('');
  const [provider, setProvider] = useState(
    () =>
      agent?.provider ||
      (connected && !models.length
        ? connected.catalog.providers?.find((provider) => provider.key_saved)?.id || 'ollama'
        : 'ollama'),
  );
  const [selectedTools, setSelectedTools] = useState<string[]>(agent?.tools || []);
  const [folderChoice, setFolderChoice] = useState<string | undefined>(
    agent?.workspace_root || undefined,
  );
  const workingFolder = folderChoice ?? connected?.catalog.workspace_root ?? '';
  const usesFolder = selectedTools.some(isFolderTool);
  const [hostedConsent, setHostedConsent] = useState(!!agent?.hosted_consent);
  const scopeKey = JSON.stringify([
    provider,
    model,
    customModel,
    workingFolder,
    [...selectedTools].sort(),
  ]);
  const [approvedScope, setApprovedScope] = useState<string | null>(() => {
    const disclosure = agent?.tool_disclosure;
    const hasWorkspaceTools = selectedTools.some(isFolderTool);
    return disclosure?.version === 1 &&
      disclosure.provider === provider &&
      JSON.stringify([...disclosure.tools].sort()) === JSON.stringify([...selectedTools].sort()) &&
      disclosure.workspace === (hasWorkspaceTools ? workingFolder || null : null)
      ? scopeKey
      : null;
  });
  const hostedToolsConsent = approvedScope === scopeKey;
  const hosted = provider !== 'ollama';
  const lab = connected?.catalog.providers?.find((value) => value.id === provider);
  const [configuration, setConfiguration] = useState(() => {
    const value = defaultAgentConfiguration();
    if (connected)
      value.limits = {
        maxSteps: connected.catalog.default_steps ?? connected.catalog.max_steps,
        maxSeconds: connected.catalog.default_seconds ?? connected.catalog.max_seconds,
        maxTokens: Math.min(value.limits.maxTokens, connected.catalog.max_tokens),
      };
    if (agent) {
      value.harness = agent.harness as typeof value.harness;
      value.toolIds = agentTools
        .filter((tool) => agentToolGroups[tool.id]?.some((id) => agent.tools?.includes(id)))
        .map((tool) => tool.id);
      value.limits = {
        maxSteps: agent.max_steps,
        maxSeconds: agent.max_seconds,
        maxTokens: agent.max_tokens,
      };
    }
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
  const runtimeProfile = connected?.catalog.runtime_profiles?.find(
    (profile) => profile.provider === provider && profile.harness === configuration.harness,
  );
  const supportedTools = connected
    ? supportedAgentTools(connected.catalog, provider, configuration.harness)
    : [];
  const requiresToolConsent =
    hosted && !!runtimeProfile?.requires_tool_consent && selectedTools.length > 0;
  const providerReady =
    !hosted || (!!lab?.key_saved && hostedConsent && (!requiresToolConsent || hostedToolsConsent));
  const tools = connected
    ? agentTools.filter(
        (tool) =>
          configuration.toolIds.includes(tool.id) ||
          agentToolGroups[tool.id]?.some((id) => connected.catalog.tools?.includes(id)),
      )
    : agentTools;
  const incompatibleTools = connected
    ? selectedTools.filter((tool) => !supportedTools.includes(tool))
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
  const saveHint = !name.trim()
    ? 'Start with a name for your agent.'
    : hosted && !lab?.key_saved
      ? `Connect ${lab?.name || provider} above to choose a model.`
      : !resolvedModel
        ? 'Choose a model to continue.'
        : hosted && !hostedConsent
          ? 'Review and allow conversation sharing above.'
          : compatibilityIssue
            ? 'Review the model and tool compatibility above.'
            : requiresToolConsent && !hostedToolsConsent
              ? 'Review and allow sharing for the selected tools.'
              : agent
                ? 'Changes apply to new work.'
                : 'You can change these settings later.';
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
                expectedWorkspaceRoot:
                  requiresToolConsent && hostedToolsConsent
                    ? workingFolder || undefined
                    : undefined,
                workspaceRoot:
                  usesFolder && connected.catalog.workspace_folders
                    ? workingFolder || undefined
                    : undefined,
                tools: selectedTools,
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
          {backLabel || (connected ? 'Back to work' : 'Agents')}
        </button>
      )}
      <header className="agent-create-heading">
        <div className="agent-editor-identity">
          {agent ? (
            <Portrait agent={engineAgentToUI(agent)} size={52} square={false} />
          ) : (
            <UserRound size={32} aria-hidden="true" />
          )}
          <h2>
            {agent ? (
              <>
                Edit <i>{agent.name}.</i>
              </>
            ) : (
              <>
                A new <i>teammate.</i>
              </>
            )}
          </h2>
        </div>
        <p>
          {guide
            ? 'Choose who helps you think and plan. Changes apply to new replies and teams you start.'
            : agent
              ? 'Changes apply to new work. Work already started keeps its current settings.'
              : 'Give them a lasting role. Assign specific work whenever you’re ready.'}
        </p>
      </header>
      <div className="agent-create-basics" data-guide={guide}>
        {!guide && (
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
            <label className="agent-purpose-field">
              What will they help with?
              <textarea
                rows={2}
                maxLength={4000}
                placeholder="Research ideas, improve our project…"
                value={purpose}
                onChange={(event) => setPurpose(event.target.value)}
              />
            </label>
            {teams.length > 1 || !connected ? (
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
            ) : (
              <p className="agent-team-note">
                In {teams.find((team) => team.id === teamId)?.name || 'your workspace'}
              </p>
            )}
          </div>
        )}
        <div className="agent-runtime-fields">
          <h3 className="agent-runtime-title">
            <Cpu size={17} />
            Model & connection
          </h3>
          <div
            className="agent-model-fields"
            data-local={!hosted && !!connected?.catalog.providers?.length}
          >
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
                    setApprovedScope(null);
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
            {hosted && lab && connected?.onSaveKey && (
              <AgentProviderKey
                key={provider}
                provider={lab}
                loadModels
                onSave={connected.onSaveKey}
                onRemove={connected.onRemoveKey}
              />
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
          </div>
          {hosted && (
            <>
              <label className="agent-hosted-consent">
                <input
                  type="checkbox"
                  checked={hostedConsent}
                  onChange={(event) => setHostedConsent(event.target.checked)}
                />
                <span>
                  {guide
                    ? 'Allow the Guide’s conversation, workspace activity summaries, agent capabilities, plans, and inspected work results to be sent to '
                    : 'Allow this agent’s instructions, prompts, and conversation history to be sent to '}
                  {lab?.name}. Provider usage charges apply.
                </span>
              </label>
              {!guide && !runtimeProfile?.tools.length && (
                <p>This profile works with your prompts and conversation only.</p>
              )}
            </>
          )}
          {!hosted && connected?.catalog.local_error && (
            <p role="status">{connected.catalog.local_error}</p>
          )}
          <details className="agent-execution-details">
            <summary>Where and how this agent runs</summary>
            {connected?.onRefresh && (
              <button
                type="button"
                className="px-text-button"
                disabled={connected.refreshing || connected.saving}
                onClick={connected.onRefresh}
              >
                {connected.refreshing ? 'Checking setup…' : 'Refresh engine setup'}
              </button>
            )}
            {!guide && (!connected || connected.catalog.harnesses.length > 1) && (
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
                    .filter(
                      (harness) => !connected || connected.catalog.harnesses.includes(harness.id),
                    )
                    .map((harness) => (
                      <option key={harness.id} value={harness.id}>
                        {harness.name}
                      </option>
                    ))}
                </select>
              </label>
            )}
            <p>
              {guide
                ? 'The Guide can read work status and save proposals. You choose when a plan starts.'
                : connected
                  ? hosted
                    ? `Tetonic runs the agent here. Model requests go to ${lab?.name || provider}. Codex and Claude Code runtimes are not connected yet.`
                    : 'Tetonic runs the agent and its tools here, using your local model.'
                  : harnesses.find((harness) => harness.id === configuration.harness)?.description}
            </p>
            {!connected && (
              <span className="agent-runtime-caption">
                Model = intelligence. Harness = how it works.
              </span>
            )}
          </details>
        </div>
      </div>
      {!guide && (
        <fieldset className="agent-tool-picker">
          <legend>
            Tools & skills{' '}
            <span>
              {configuration.toolIds.length +
                configuration.resourceIds.length +
                selectedTools.filter((name) => name.startsWith('mcp_') || name.startsWith('skill_'))
                  .length}{' '}
              selected
            </span>
          </legend>
          {connected && usesFolder && (
            <div className="agent-working-folder">
              <label>
                Working folder
                {connected.catalog.workspace_folders ? (
                  <select
                    value={workingFolder}
                    onChange={(event) => setFolderChoice(event.target.value)}
                  >
                    <option value="">Choose an approved folder</option>
                    {workingFolder &&
                      !connected.catalog.workspace_folders.includes(workingFolder) && (
                        <option value={workingFolder}>{workingFolder} · unavailable</option>
                      )}
                    {connected.catalog.workspace_folders.map((path) => (
                      <option key={path} value={path}>
                        {path}
                      </option>
                    ))}
                  </select>
                ) : (
                  <span>{workingFolder || 'No working folder configured'}</span>
                )}
              </label>
              <p className="agent-field-note">
                File and terminal tools stay within this folder. Saving applies to new work; active
                runs keep their existing access.
              </p>
              <details>
                <summary>Need a different folder?</summary>
                <p>
                  The host operator can add it to <code>agent_folders</code> in the engine
                  configuration. Refresh setup after the engine reloads. Your discussion stays
                  saved. This does not grant access to any other folders.
                </p>
              </details>
            </div>
          )}
          <div className="agent-tool-grid">
            {tools.map((tool) => {
              const group = agentToolGroups[tool.id] || [];
              const availableTools = group.filter((name) => supportedTools.includes(name));
              const selected = configuration.toolIds.includes(tool.id);
              const described = selected
                ? selectedTools.filter((name) => group.includes(name))
                : availableTools;
              return (
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
                      !availableTools.length
                    }
                    onChange={(event) => {
                      if (connected)
                        setSelectedTools(
                          event.target.checked
                            ? [
                                ...selectedTools.filter((name) => !group.includes(name)),
                                ...availableTools,
                              ]
                            : selectedTools.filter((name) => !group.includes(name)),
                        );
                      setConfiguration({
                        ...configuration,
                        toolIds: event.target.checked
                          ? [...configuration.toolIds, tool.id]
                          : configuration.toolIds.filter((id) => id !== tool.id),
                      });
                    }}
                  />
                  <span>
                    <strong>{tool.name}</strong>
                    <small>
                      {connected
                        ? toolDescription(described) || 'Unavailable with this provider'
                        : tool.description}
                    </small>
                  </span>
                </label>
              );
            })}
          </div>
          {connected && (
            <p className="agent-field-note">
              {tools.length
                ? 'Only selected abilities are granted. They stay with this agent across assignments.'
                : 'No workspace tools are available for this provider and host.'}
            </p>
          )}
          {connected &&
            selectedTools
              .filter(
                (id) =>
                  !id.startsWith('mcp_') &&
                  !id.startsWith('skill_') &&
                  !Object.values(agentToolGroups).some((group) => group.includes(id)),
              )
              .map((id) => (
                <label className="agent-tool-choice" key={id}>
                  <input
                    type="checkbox"
                    checked
                    aria-label={`Selected tool: ${id}`}
                    onChange={() => setSelectedTools(selectedTools.filter((name) => name !== id))}
                  />
                  <span>
                    <strong>{id}</strong>
                    <small>
                      {supportedTools.includes(id)
                        ? 'Selected for this agent'
                        : 'Unavailable. Remove this tool or restore its access.'}
                    </small>
                  </span>
                </label>
              ))}
          {compatibilityIssue && (
            <p className="local-notice" role="alert">
              {compatibilityIssue}
            </p>
          )}
          {connected && selectedTools.includes('run_shell') && (
            <p className="agent-field-note">
              Terminal runs local commands and installed command-line tools. Each command appears in
              Needs you for your approval. The working folder is not a security boundary on every
              operating system; any isolation gaps are shown with the command.
            </p>
          )}
          {connected && (
            <AgentMcpTools
              connections={connected.catalog.mcp_connections || []}
              selected={selectedTools}
              supported={supportedTools}
              onSelect={setSelectedTools}
              onDiscover={connected.onDiscoverMcp}
              disabled={connected.saving}
            />
          )}
          {connected && (
            <>
              <AgentSkills
                skills={connected.catalog.skills || []}
                selected={selectedTools}
                supported={supportedTools}
                onSelect={setSelectedTools}
                disabled={connected.saving}
              />
              {connected.library && (
                <details className="agent-capability-entry">
                  <summary>Add tools, MCPs or skills to the workspace</summary>
                  {connected.library}
                </details>
              )}
            </>
          )}
          {requiresToolConsent && !compatibilityIssue && (
            <label className="agent-hosted-consent">
              <input
                type="checkbox"
                checked={hostedToolsConsent}
                onChange={(event) => setApprovedScope(event.target.checked ? scopeKey : null)}
              />
              <span>
                Allow selected tool inputs and results to be sent to {lab?.name}.
                {selectedTools.some(
                  isFolderTool,
                ) && ` Working folder: ${workingFolder || 'the engine’s configured folder'}.`}{' '}
                Only the selected tools are included in this approval.
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
      )}
      {connected &&
        !guide &&
        (agent && connected.permissionsClient ? (
          <CapabilityPolicyEditor
            scope="agent"
            scopeId={agent.id}
            client={connected.permissionsClient}
          />
        ) : (
          !agent && (
            <p className="agent-field-note">
              Your agent inherits workspace permissions. After creating it, use Edit agent to set
              its autonomy and capability limits.
            </p>
          )
        ))}
      <AgentAdvancedSettings
        guide={guide}
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
        <p id={saveHintId} className="agent-save-hint" aria-live="polite">
          {connected ? saveHint : 'Preview only. Model, tools, and access aren’t connected yet.'}
        </p>
        <button
          type="submit"
          className="canvas-primary"
          aria-describedby={saveHintId}
          disabled={
            !name.trim() ||
            !resolvedModel ||
            !providerReady ||
            !!compatibilityIssue ||
            connected?.saving
          }
        >
          {connected?.saving ? 'Saving…' : agent ? 'Save changes' : 'Create agent'}{' '}
          {!agent && <Plus size={16} />}
        </button>
      </footer>
    </form>
  );
}
