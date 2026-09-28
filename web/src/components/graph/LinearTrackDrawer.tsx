import * as Dialog from '@radix-ui/react-dialog';
import { X, ArrowUpRight } from 'lucide-react';
import { AgentTrack, GraphNode, GraphEdge } from '../../types';
import { Button } from '../ui/Button';
import { Badge } from '../ui/Badge';
import { sampleTime, type WorkRecord } from '../../lib/workEvidence';
interface Props {
  track: AgentTrack | null;
  node?: GraphNode | null;
  edges: GraphEdge[];
  nodes: GraphNode[];
  onClose: () => void;
  onJumpToHuddle: (id: string) => void;
  activityRecords?: WorkRecord[];
}
export function LinearTrackDrawer({
  track,
  node,
  edges,
  nodes,
  onClose,
  onJumpToHuddle,
  activityRecords,
}: Props) {
  return (
    <Dialog.Root
      open={!!node}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="inspector" onCloseAutoFocus={(e) => e.preventDefault()}>
          <header className="inspector-header">
            <div>
              <p className="eyebrow">Sample {node?.type}</p>
              <Dialog.Title>{node?.name}</Dialog.Title>
              <Dialog.Description>{node?.label}</Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <Button size="icon" aria-label="Close inspector">
                <X size={20} />
              </Button>
            </Dialog.Close>
          </header>
          <div className="inspector-body">
            {activityRecords && (
              <section className="work-summary" aria-label="Current playback calls">
                <h3>Current playback</h3>
                <p>{activityRecords.length} recorded events at this resource · not live</p>
                <ol className="work-evidence-list">
                  {activityRecords.slice(-10).map((record) => (
                    <li key={record.id}>
                      <strong>
                        {record.state} · {sampleTime(record.at)}
                      </strong>
                      <p>{record.interaction.label}</p>
                      <button
                        className="text-action"
                        onClick={() => onJumpToHuddle(record.interaction.agentId)}
                      >
                        Inspect agent work <ArrowUpRight size={14} />
                      </button>
                    </li>
                  ))}
                </ol>
                {!activityRecords.length && (
                  <p>No calls to this resource in the current playback yet.</p>
                )}
              </section>
            )}
            <Badge>{node?.statusLabel || node?.status}</Badge>
            <p>
              {node?.metadata?.details ||
                node?.metadata?.charter ||
                'No additional description provided.'}
            </p>
            <dl className="facts">
              {node?.metadata?.model && (
                <div>
                  <dt>Model</dt>
                  <dd>{node.metadata.model}</dd>
                </div>
              )}
              {node?.metadata?.endpoint && (
                <div>
                  <dt>Endpoint</dt>
                  <dd>{node.metadata.endpoint}</dd>
                </div>
              )}
              {node?.metadata?.latencyMs !== undefined && (
                <div>
                  <dt>Sample latency</dt>
                  <dd>{node.metadata.latencyMs} ms</dd>
                </div>
              )}
              {track && (
                <div>
                  <dt>Sample trace snapshot</dt>
                  <dd>
                    {track.totalTokens.toLocaleString()} tokens · {track.currentStatus}
                  </dd>
                </div>
              )}
            </dl>
            {!!node?.metadata?.capabilities?.length && (
              <section>
                <h3>Listed capabilities</h3>
                <ul className="capability-list">
                  {node.metadata.capabilities.map((c) => (
                    <li key={c}>
                      <code>{c}</code>
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section>
              <h3>Connections ({edges.length})</h3>
              <ul className="relationship-list">
                {edges.map((e) => (
                  <li key={e.id}>
                    <strong>
                      {nodes.find((n) => n.id === (e.source === node?.id ? e.target : e.source))
                        ?.name || (e.source === node?.id ? e.target : e.source)}
                    </strong>
                    <p className="muted">
                      {e.source === node?.id ? 'Outgoing' : 'Incoming'} ·{' '}
                      {e.type.replaceAll('_', ' ')} ·{' '}
                      {e.isActive ? 'Active in snapshot' : 'Inactive in snapshot'}
                    </p>
                    <p>{e.label}</p>
                    {e.activityLabel && <p className="muted">{e.activityLabel}</p>}
                  </li>
                ))}
              </ul>
              {!edges.length && <p className="muted">No connections listed in this snapshot.</p>}
            </section>
            {track ? (
              <section>
                <h3 className="mb-4">Execution trace</h3>
                <ol className="trace-list">
                  {track.steps.map((step) => (
                    <li key={step.id}>
                      <div className="row-between flex-wrap gap-2">
                        <span className="eyebrow">
                          {step.stepNumber} · {step.type.replaceAll('_', ' ')}
                        </span>
                        <Badge variant={step.status === 'failed' ? 'danger' : 'default'}>
                          {step.status}
                        </Badge>
                      </div>
                      <h4>{step.title}</h4>
                      <p className="muted">
                        {step.timestamp}
                        {step.durationMs !== undefined ? ` · ${step.durationMs} ms` : ''}
                        {step.tokens !== undefined ? ` · ${step.tokens} tokens` : ''}
                      </p>
                      {step.targetNodeName && <p>Target: {step.targetNodeName}</p>}
                      {(step.inputPayload || step.outputPayload || step.diffSnippet) && (
                        <details>
                          <summary>Inspect payloads</summary>
                          {step.inputPayload && (
                            <>
                              <h5>Input</h5>
                              <pre className="code-block" tabIndex={0}>
                                {step.inputPayload}
                              </pre>
                            </>
                          )}
                          {step.outputPayload && (
                            <>
                              <h5>Output</h5>
                              <pre className="code-block" tabIndex={0}>
                                {step.outputPayload}
                              </pre>
                            </>
                          )}
                          {step.diffSnippet && (
                            <pre className="code-block" tabIndex={0}>
                              {step.diffSnippet}
                            </pre>
                          )}
                        </details>
                      )}
                    </li>
                  ))}
                </ol>
              </section>
            ) : (
              node?.type === 'agent' && (
                <p className="muted">No execution trace supplied for this agent.</p>
              )
            )}
            {node?.type === 'connector' && (
              <details>
                <summary>Illustrative connector exchange</summary>
                <p className="muted">
                  Example only; scopes and protocol verification are not connected.
                </p>
                <pre className="code-block" tabIndex={0}>
                  {JSON.stringify(
                    {
                      jsonrpc: '2.0',
                      method: 'mcp/connector_handshake',
                      params: {
                        outpost: node.name,
                        origin_castle: 'Alice_Workstation',
                        protocol_version: '2026-v1.0',
                      },
                    },
                    null,
                    2,
                  )}
                </pre>
                <p className="muted">
                  Illustrative scope examples: pull_requests:read, issues:sync, repo:patch,
                  admin:write. Actual granted scopes are unknown.
                </p>
              </details>
            )}
            {node?.type === 'tool' && (
              <p className="scope-note">
                Live approval requirements, network isolation, and permitted callers are not
                supplied by this snapshot. Check the engine’s effective policy before execution.
              </p>
            )}
          </div>
          <footer className="inspector-footer">
            <p className="muted">Sample snapshot · no live audit verification</p>
            {node?.type === 'agent' && (
              <Button
                variant="copper"
                onClick={() => {
                  onClose();
                  onJumpToHuddle(node.id);
                }}
              >
                Open activity <ArrowUpRight size={16} />
              </Button>
            )}
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
