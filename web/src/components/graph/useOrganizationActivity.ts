import { useMemo, useState } from 'react';
import type { Agent, AgentTrack, GraphEdge, GraphNode } from '../../types';
import { destinationsFor, actionsFor } from '../../lib/mapActivity';
import { examplesFor } from '../../lib/motionPlayback';
import { evidenceFor, evidenceEvents } from '../../lib/workEvidence';
import { useWorkPlayback } from './useWorkPlayback';

export function useOrganizationActivity(
  agents: Agent[],
  nodes: GraphNode[],
  edges: GraphEdge[],
  tracks: Record<string, AgentTrack>,
) {
  // A fixture is one organization-wide timeline. Filtering teams never regenerates it.
  const [examples] = useState(() => {
    const places = destinationsFor(agents, nodes, edges);
    return examplesFor(agents, places, actionsFor(agents, places, tracks));
  });
  const [exampleId, setExampleId] = useState('trace');
  const [reading, setReading] = useState(false);
  const example = examples.find((e) => e.id === exampleId) || examples[0];
  const motion = useWorkPlayback(example, reading);
  const allRecords = useMemo(() => evidenceFor(example), [example]);
  const count = motion.started ? allRecords.filter((r) => r.at <= motion.elapsed).length : 0;
  const records = useMemo(() => allRecords.slice(0, count), [allRecords, count]);
  const events = useMemo(
    () =>
      evidenceEvents(records).map((event) => ({
        ...event,
        agentName: agents.find((a) => a.id === event.agentId)?.name || event.agentId,
      })),
    [records, agents],
  );
  return {
    ...motion,
    agents,
    example,
    examples,
    setExampleId,
    records,
    events,
    setReading,
    reading,
  };
}
export type OrganizationActivity = ReturnType<typeof useOrganizationActivity>;
