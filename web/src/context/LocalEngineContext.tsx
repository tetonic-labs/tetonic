import React, { createContext, useContext, useEffect, useRef, useState, useCallback } from 'react';
import { LocalEngine } from '../engine/client';
import { takeConnectionToken } from '../engine/connection';
import {
  type EngineWorkspace,
  type AgentCatalog,
  type LocalWorkItem,
  type LocalApprovalsInspection,
  type LocalDigestResponse,
  type LocalTeamInfo,
  type WorkTeamSelection,
  type EngineTask,
  type LocalApproval,
} from '../engine/contracts';
import type { Agent } from '../types';
import { engineAgentToUI } from '../engine/projections/agents';
import { mergeTasks, mergeWorkspace } from '../engine/projections/records';

interface LocalEngineContextType {
  isConnected: boolean;
  isConnecting: boolean;
  error: string | null;
  lastUpdated: number | null;
  readErrors: Record<string, string>;
  client: LocalEngine;
  refresh: () => Promise<void>;
  workspace: EngineWorkspace | null;
  catalog: AgentCatalog | null;
  workItems: LocalWorkItem[];
  approvals: LocalApprovalsInspection | null;
  digest: LocalDigestResponse | null;
  teams: LocalTeamInfo[];
  uiAgents: Agent[];
  submitTask: (
    input: string,
    agent_key?: string,
    parent_id?: string,
    request_id?: string,
    purpose?: 'work' | 'explore',
    work_team?: WorkTeamSelection,
  ) => Promise<EngineTask>;
  cancelTask: (id: string) => Promise<EngineTask>;
  createWorkItem: (
    title: string,
    agent_key?: string,
    goal_id?: string,
    lead_id?: string,
    agent_ids?: string[],
  ) => Promise<LocalWorkItem>;
  updateWorkItemNotes: (id: string, notes: string[]) => Promise<void>;
  updateWorkItem: (
    id: string,
    patch: {
      notes?: string[];
      status?: string;
      lead_id?: string;
      agent_ids?: string[];
    },
  ) => Promise<void>;
  resolveApproval: (
    approval_id: string,
    allow: boolean,
    proposal_digest: string,
  ) => Promise<LocalApproval>;
  refreshDigest: () => Promise<LocalDigestResponse | null>;
  reconnect: () => void;
}

const LocalEngineContext = createContext<LocalEngineContextType | null>(null);

export function LocalEngineProvider({
  children,
  client: providedClient,
}: {
  children: React.ReactNode;
  client?: LocalEngine;
}) {
  const [token, setToken] = useState(() => takeConnectionToken());
  const engineRef = useRef<LocalEngine | null>(null);

  const [workspace, setWorkspace] = useState<EngineWorkspace | null>(null);
  const [catalog, setCatalog] = useState<AgentCatalog | null>(null);
  const [workItems, setWorkItems] = useState<LocalWorkItem[]>([]);
  const [approvals, setApprovals] = useState<LocalApprovalsInspection | null>(null);
  const [digest, setDigest] = useState<LocalDigestResponse | null>(null);
  const [teams, setTeams] = useState<LocalTeamInfo[]>([]);

  const [isConnected, setIsConnected] = useState(false);
  const [isConnecting, setIsConnecting] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<number | null>(null);
  const [readErrors, setReadErrors] = useState<Record<string, string>>({});
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const catalogRead = useRef<{
    engine: LocalEngine;
    controller: AbortController;
    pending: Promise<void> | null;
    nextReadAt: number;
  } | null>(null);
  const pollVersion = useRef(0);
  const alive = useRef(true);

  if (!engineRef.current) {
    engineRef.current = providedClient || new LocalEngine(token);
  }

  // Discovery can contact model providers. Keep it off the live-work critical path;
  // explicit refreshes after settings changes still fetch the new catalog immediately.
  const refreshCatalog = useCallback((force = false): Promise<void> => {
    const engine = engineRef.current;
    if (!engine || !alive.current) return Promise.resolve();
    const previous = catalogRead.current;
    if (!force && previous?.engine === engine && !previous.controller.signal.aborted) {
      if (previous.pending) return previous.pending;
      if (Date.now() < previous.nextReadAt) return Promise.resolve();
    }
    previous?.controller.abort();
    const read = {
      engine,
      controller: new AbortController(),
      pending: null as Promise<void> | null,
      nextReadAt: 0,
    };
    catalogRead.current = read;
    const isCurrent = () =>
      alive.current &&
      engine === engineRef.current &&
      catalogRead.current === read &&
      !read.controller.signal.aborted;
    read.pending = (async () => {
      try {
        const result = await engine.agentCatalog(read.controller.signal);
        if (!isCurrent()) return;
        setCatalog(result);
        setCatalogError(null);
        read.nextReadAt = Date.now() + 30000;
      } catch (err) {
        if (!isCurrent()) return;
        setCatalogError(err instanceof Error ? err.message : 'Could not refresh agent setup');
        read.nextReadAt = Date.now() + 5000;
      } finally {
        read.pending = null;
      }
    })();
    return read.pending;
  }, []);

  const poll = useCallback(async (signal?: AbortSignal) => {
    const engine = engineRef.current;
    if (!engine) return;
    const version = ++pollVersion.current;

    try {
      const [ws, items, apprv, tm] = await Promise.allSettled([
        engine.snapshot(signal),
        engine.workItems(signal),
        engine.approvals(signal),
        engine.teams(signal),
      ]);

      if (
        signal?.aborted ||
        !alive.current ||
        engine !== engineRef.current ||
        version !== pollVersion.current
      )
        return;
      if (ws.status === 'rejected') throw ws.reason;

      setWorkspace((old) => mergeWorkspace(old, ws.value));
      if (items.status === 'fulfilled') setWorkItems(items.value);
      if (apprv.status === 'fulfilled') setApprovals(apprv.value);
      if (tm.status === 'fulfilled') setTeams(tm.value);
      const errors: Record<string, string> = {};
      for (const [name, result] of [
        ['Work details', items],
        ['Decisions', apprv],
        ['Teams', tm],
      ] as const)
        if (result.status === 'rejected')
          errors[name] =
            result.reason instanceof Error ? result.reason.message : 'Could not refresh';
      setReadErrors(errors);
      setLastUpdated(Date.now());

      setIsConnected(true);
      setError(null);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (
        signal?.aborted ||
        !alive.current ||
        engine !== engineRef.current ||
        version !== pollVersion.current
      )
        return;
      setIsConnected(false);
      setError(msg);
    } finally {
      if (
        !signal?.aborted &&
        alive.current &&
        engine === engineRef.current &&
        version === pollVersion.current
      )
        setIsConnecting(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    await Promise.all([poll(), refreshCatalog(true)]);
  }, [poll, refreshCatalog]);

  useEffect(() => {
    alive.current = true;
    const controller = new AbortController();
    let active = true;
    let timeoutId: number | undefined;

    async function tick() {
      void refreshCatalog();
      await poll(controller.signal);
      if (active) timeoutId = window.setTimeout(tick, 1500);
    }

    tick();

    return () => {
      active = false;
      alive.current = false;
      controller.abort();
      catalogRead.current?.controller.abort();
      if (timeoutId) window.clearTimeout(timeoutId);
    };
  }, [poll, refreshCatalog, token]);

  const submitTask = useCallback(
    async (
      input: string,
      agent_key = 'Local assistant',
      parent_id?: string,
      request_id?: string,
      purpose?: 'work' | 'explore',
      work_team?: WorkTeamSelection,
    ) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      const requestId = request_id || crypto.randomUUID();
      const task = work_team
        ? await engine.submit(requestId, input, agent_key, parent_id, purpose, work_team)
        : purpose
          ? await engine.submit(requestId, input, agent_key, parent_id, purpose)
          : await engine.submit(requestId, input, agent_key, parent_id);
      if (
        task.id !== requestId ||
        task.input !== input.trim() ||
        task.agent_key !== agent_key ||
        (task.purpose || 'work') !== (purpose || 'work') ||
        (task.parent_id || undefined) !== parent_id ||
        (work_team &&
          (task.work_team?.id !== work_team.id ||
            task.work_team.revision !== work_team.revision)) ||
        (!parent_id && !work_team && !!task.work_team)
      )
        throw new Error('The reply did not confirm this request. Check your work before retrying.');
      if (engine !== engineRef.current)
        throw new Error(
          'The workspace connection changed. Reopen the original workspace to check this request.',
        );
      setWorkspace((old) => (old ? { ...old, tasks: mergeTasks(old.tasks, [task]) } : old));
      void poll();
      return task;
    },
    [poll],
  );

  const cancelTask = useCallback(
    async (id: string) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      const task = await engine.cancel(id);
      if (task.id !== id) throw new Error('The engine did not confirm which request was stopped.');
      if (engine !== engineRef.current)
        throw new Error(
          'The workspace connection changed. Check the original workspace for the stop result.',
        );
      setWorkspace((old) => (old ? { ...old, tasks: mergeTasks(old.tasks, [task]) } : old));
      void poll();
      return task;
    },
    [poll],
  );

  const createWorkItem = useCallback(
    async (
      title: string,
      agent_key = 'Local assistant',
      goal_id?: string,
      lead_id?: string,
      agent_ids?: string[],
    ) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      const id = crypto.randomUUID();
      const item = await engine.createWorkItem({
        id,
        title,
        agent_key,
        goal_id,
        lead_id,
        agent_ids,
      });
      poll();
      return item;
    },
    [poll],
  );

  const updateWorkItemNotes = useCallback(
    async (id: string, notes: string[]) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      await engine.patchWorkItem(id, { notes });
      poll();
    },
    [poll],
  );

  const updateWorkItem = useCallback(
    async (
      id: string,
      patch: {
        notes?: string[];
        status?: string;
        lead_id?: string;
        agent_ids?: string[];
      },
    ) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      await engine.patchWorkItem(id, patch);
      poll();
    },
    [poll],
  );

  const resolveApproval = useCallback(
    async (approval_id: string, allow: boolean, proposal_digest: string) => {
      const engine = engineRef.current;
      if (!engine) throw new Error('Local engine not available');
      const resolved = await engine.resolveApproval(approval_id, allow, proposal_digest);
      poll();
      return resolved;
    },
    [poll],
  );

  const refreshDigest = useCallback(async () => {
    const engine = engineRef.current;
    if (!engine) return null;
    const dig = await engine.digest();
    setDigest(dig);
    return dig;
  }, []);

  const reconnect = useCallback(() => {
    const refreshedToken = takeConnectionToken();
    if (refreshedToken !== token && !providedClient) {
      setWorkspace(null);
      setWorkItems([]);
      setApprovals(null);
      setTeams([]);
      setCatalog(null);
      setDigest(null);
      setLastUpdated(null);
      setReadErrors({});
      setCatalogError(null);
    }
    setToken(refreshedToken);
    engineRef.current = providedClient || new LocalEngine(refreshedToken);
    setIsConnecting(true);
    void refresh();
  }, [refresh, token, providedClient]);

  useEffect(() => {
    const connect = () => {
      if (new URLSearchParams(window.location.hash.slice(1)).has('connect')) reconnect();
    };
    window.addEventListener('hashchange', connect);
    return () => window.removeEventListener('hashchange', connect);
  }, [reconnect]);

  // Project engine state to UI models
  const tasks = workspace?.tasks || [];
  const engineAgents = workspace?.agents || [];
  const uiAgents: Agent[] = engineAgents.map((a) =>
    engineAgentToUI(a, [...tasks, ...(workspace?.planning_tasks || [])]),
  );

  return (
    <LocalEngineContext.Provider
      value={{
        isConnected,
        isConnecting,
        error,
        lastUpdated,
        readErrors: catalogError ? { ...readErrors, 'Agent setup': catalogError } : readErrors,
        client: engineRef.current,
        refresh,
        workspace,
        catalog,
        workItems,
        approvals,
        digest,
        teams,
        uiAgents,
        submitTask,
        cancelTask,
        createWorkItem,
        updateWorkItemNotes,
        updateWorkItem,
        resolveApproval,
        refreshDigest,
        reconnect,
      }}
    >
      {children}
    </LocalEngineContext.Provider>
  );
}

export function useLocalEngine() {
  const context = useContext(LocalEngineContext);
  if (!context) {
    throw new Error('useLocalEngine must be used within a LocalEngineProvider');
  }
  return context;
}
