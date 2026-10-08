import { useState } from 'react';
import { WorkspaceSettings } from './WorkspaceSettings';
import { useLocalEngine } from '../../context/LocalEngineContext';
import type { EngineAgent } from '../../lib/localEngine';
import type { WorkRecord } from '../../lib/workspaceRecords';
import { LocalAgentSetup } from '../work/LocalAgentSetup';
import { EngineAgentDetail } from './EngineAgentDetail';
import { AgentRoster } from './AgentRoster';
import { TeamsPanel } from './TeamsPanel';
import { WorkOverview } from './WorkOverview';
import { AttentionPanel } from './AttentionPanel';

export type TeamPanel = 'work' | 'agents' | 'teams' | 'attention' | 'settings';

export function TeamPanels({
  panel,
  records,
  onWork,
  onAgent,
  dark,
  setDark,
  initialAgentId,
  onOpenAgents = () => {},
  onOpenWork = () => {},
  onTeam = () => {},
  editInitially = false,
}: {
  panel: TeamPanel;
  records: WorkRecord[];
  onWork: (id: string) => void;
  onAgent: (key: string) => void;
  dark: boolean;
  setDark: (value: boolean) => void;
  initialAgentId?: string;
  onOpenAgents?: () => void;
  onOpenWork?: () => void;
  onTeam?: (id: string) => void;
  editInitially?: boolean;
}) {
  const engine = useLocalEngine();
  const { workspace, client } = engine;
  const [creating, setCreating] = useState(false);
  const [agentId, setAgentId] = useState<string | null>(initialAgentId || null);
  const [createdAgent, setCreatedAgent] = useState<EngineAgent | null>(null);
  const [editingAgent, setEditingAgent] = useState<EngineAgent | null>(() =>
    editInitially
      ? workspace?.agents.find((a) => a.key === initialAgentId && a.editable !== false) || null
      : null,
  );
  const [updatedAgentKey, setUpdatedAgentKey] = useState<string | null>(null);
  if (panel === 'work') return <WorkOverview records={records} onWork={onWork} />;
  if (panel === 'attention') return <AttentionPanel records={records} onWork={onWork} />;
  if (panel === 'agents') {
    if (editingAgent && workspace)
      return (
        <LocalAgentSetup
          key={editingAgent.definition_digest}
          client={client}
          workspace={workspace}
          agent={editingAgent}
          onBack={() => setEditingAgent(null)}
          onCreated={async (saved) => {
            await engine.refresh();
            setUpdatedAgentKey(saved.key);
            setEditingAgent(null);
          }}
        />
      );
    if (creating && workspace)
      return (
        <LocalAgentSetup
          client={client}
          workspace={workspace}
          onBack={() => setCreating(false)}
          onCreated={(created) => {
            setCreatedAgent(created);
            setAgentId(created.key);
            setCreating(false);
            void engine.refresh();
          }}
        />
      );
    const profile =
      workspace?.agents.find((entry) => entry.key === agentId) ||
      (createdAgent?.key === agentId ? createdAgent : null);
    if (profile)
      return (
        <EngineAgentDetail
          profile={profile}
          created={createdAgent?.key === profile.key}
          updated={updatedAgentKey === profile.key}
          onEdit={() => {
            setCreatedAgent(null);
            setUpdatedAgentKey(null);
            setEditingAgent(profile);
          }}
          records={records}
          onBack={() => {
            setAgentId(null);
            setCreatedAgent(null);
          }}
          onWork={onWork}
          onAgent={onAgent}
        />
      );
    return (
      <AgentRoster records={records} onSelect={setAgentId} onCreate={() => setCreating(true)} />
    );
  }
  if (panel === 'teams')
    return <TeamsPanel onAgents={onOpenAgents} onWork={onOpenWork} onTeam={onTeam} />;
  return <WorkspaceSettings dark={dark} setDark={setDark} />;
}
