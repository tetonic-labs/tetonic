import { useRef, useState } from 'react';
import { ZoomIn, ZoomOut, Network } from 'lucide-react';
import { GraphNode, GraphEdge, AgentTrack } from '../../types';
import { LinearTrackDrawer } from '../graph/LinearTrackDrawer';
import { Button } from '../ui/Button';
import { Badge } from '../ui/Badge';
import { ScreenHeading, EmptyState } from '../ui/Screen';
interface Props {
  nodes: GraphNode[];
  edges: GraphEdge[];
  tracks: Record<string, AgentTrack>;
  onJumpToHuddle: (id: string) => void;
}
const categories = {
  castle: 'Local workstation',
  team: 'Shared team',
  external: 'External services',
};
export function GraphMapView({ nodes, edges, tracks, onJumpToHuddle }: Props) {
  const [mode, setMode] = useState<'map' | 'list'>('list');
  const [filter, setFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const [scale, setScale] = useState(0.75);
  const [pan, setPan] = useState({ x: 20, y: 40 });
  const canvas = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const filtered = nodes.filter(
    (n) =>
      (filter === 'all' ||
        (filter === 'active'
          ? n.status === 'active' || n.status === 'executing' || n.status === 'thinking'
          : n.type === filter)) &&
      `${n.name} ${n.label}`.toLowerCase().includes(search.toLowerCase()),
  );
  const columns = [
    nodes.filter((n) => n.type === 'agent'),
    nodes.filter((n) => ['tool', 'storage', 'inference'].includes(n.type)),
    nodes.filter((n) => ['connector', 'human'].includes(n.type)),
  ];
  const positions = new Map(
    columns.flatMap((col, i) =>
      col.map((n, j) => [n.id, { x: i * 340 + 20, y: j * 132 + 60 }] as const),
    ),
  );
  const mapHeight = Math.max(400, ...columns.map((c) => c.length * 132 + 90));
  function inspect(id: string) {
    opener.current = document.activeElement as HTMLElement;
    setSelected(id);
  }
  function close() {
    setSelected(null);
    requestAnimationFrame(() => opener.current?.focus());
  }
  function fit() {
    if (canvas.current) {
      setScale(
        Math.min(
          1,
          (canvas.current.clientWidth - 32) / 1020,
          (canvas.current.clientHeight - 32) / mapHeight,
        ),
      );
      setPan({ x: 16, y: 16 });
    }
  }
  const node = nodes.find((n) => n.id === selected);
  return (
    <div className="screen connections-screen">
      <ScreenHeading
        title="Connections"
        description="Inspect the agents, tools, and services in this sample environment."
      >
        <div className="segmented" aria-label="Connection view">
          <button aria-pressed={mode === 'list'} onClick={() => setMode('list')}>
            List
          </button>
          <button
            aria-pressed={mode === 'map'}
            onClick={() => {
              setMode('map');
              requestAnimationFrame(fit);
            }}
          >
            Map
          </button>
        </div>
      </ScreenHeading>
      <div className="connection-filters">
        <label className="field-label grow">
          Find a connection
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search agents, tools, services…"
          />
        </label>
        <label className="field-label">
          Show
          <select value={filter} onChange={(e) => setFilter(e.target.value)}>
            {[
              ['all', 'All connections'],
              ['agent', 'Agents'],
              ['tool', 'Tools'],
              ['connector', 'Connectors'],
              ['active', 'Active in sample'],
            ].map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <span className="muted">
          {filtered.length} of {nodes.length}
        </span>
      </div>
      <p className="muted">
        Agent state matches the workspace. Resource connections and traces are sample snapshots.
      </p>
      {!filtered.length ? (
        <EmptyState title="No matching connections">
          Try a different search or choose All connections.
        </EmptyState>
      ) : mode === 'list' ? (
        <div className="connection-grid">
          {filtered.map((n) => (
            <button
              className="panel connection-card"
              key={n.id}
              onClick={() => inspect(n.id)}
              aria-label={`Inspect ${n.name}`}
            >
              <span className="row-between">
                <span className="eyebrow">{n.type}</span>
                <Badge>{n.statusLabel || n.status}</Badge>
              </span>
              <strong>{n.name}</strong>
              <span className="muted">{n.label}</span>
              <span className="connection-location">
                {categories[n.category]} <span aria-hidden="true">↗</span>
              </span>
            </button>
          ))}
        </div>
      ) : (
        <>
          <div className="map-toolbar toolbar">
            <Button
              size="icon"
              aria-label="Zoom in"
              onClick={() => setScale((s) => Math.min(1.8, s + 0.1))}
            >
              <ZoomIn size={18} />
            </Button>
            <Button
              size="icon"
              aria-label="Zoom out"
              onClick={() => setScale((s) => Math.max(0.2, s - 0.1))}
            >
              <ZoomOut size={18} />
            </Button>
            <Button onClick={fit}>Fit map</Button>
            <Button
              onClick={() => {
                setScale(0.8);
                setPan({ x: 20, y: 30 });
              }}
            >
              Local agents
            </Button>
            <Button
              onClick={() => {
                setScale(0.8);
                setPan({ x: -500, y: 30 });
              }}
            >
              External services
            </Button>
            <span className="muted">{Math.round(scale * 100)}%</span>
          </div>
          <div
            ref={canvas}
            className="graph-canvas"
            tabIndex={0}
            role="region"
            aria-label="Connection map. Use arrow keys to pan; plus and minus to zoom. List view contains the same connections."
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return;
              const directions: Record<string, { x: number; y: number }> = {
                ArrowLeft: { x: 40, y: 0 },
                ArrowRight: { x: -40, y: 0 },
                ArrowUp: { x: 0, y: 40 },
                ArrowDown: { x: 0, y: -40 },
              };
              const d = directions[e.key];
              if (d) {
                e.preventDefault();
                setPan((p) => ({ x: p.x + d.x, y: p.y + d.y }));
              }
              if (e.key === '+' || e.key === '-') {
                e.preventDefault();
                setScale((s) => Math.max(0.2, Math.min(1.8, s + (e.key === '+' ? 0.1 : -0.1))));
              }
            }}
            onPointerDown={(e) => {
              if ((e.target as HTMLElement).closest('button')) return;
              drag.current = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
              e.currentTarget.setPointerCapture(e.pointerId);
            }}
            onPointerMove={(e) => {
              const d = drag.current;
              if (d) setPan({ x: d.px + e.clientX - d.x, y: d.py + e.clientY - d.y });
            }}
            onPointerUp={() => {
              drag.current = null;
            }}
            onPointerCancel={() => {
              drag.current = null;
            }}
          >
            <div
              className="graph-stage"
              style={{
                width: 1020,
                height: mapHeight,
                transform: `translate(${pan.x}px,${pan.y}px) scale(${scale})`,
              }}
            >
              {['Agents', 'Execution capabilities', 'People & external services'].map(
                (label, i) => (
                  <span className="map-column-title" style={{ left: i * 340 + 20 }} key={label}>
                    {label}
                  </span>
                ),
              )}
              <svg width="1020" height={mapHeight} aria-hidden="true">
                {edges.map((e) => {
                  const s = positions.get(e.source),
                    t = positions.get(e.target);
                  if (
                    !s ||
                    !t ||
                    !filtered.some((n) => n.id === e.source) ||
                    !filtered.some((n) => n.id === e.target)
                  )
                    return null;
                  return (
                    <path
                      key={e.id}
                      d={`M ${s.x + 140} ${s.y + 52} C ${s.x + 320} ${s.y + 52}, ${t.x - 40} ${t.y + 52}, ${t.x + 140} ${t.y + 52}`}
                      stroke={e.isActive ? 'var(--color-accent)' : 'var(--text-muted)'}
                      opacity=".5"
                      strokeWidth="2"
                      strokeDasharray={e.isActive ? undefined : '5 5'}
                      fill="none"
                    />
                  );
                })}
              </svg>
              {filtered.map((n) => (
                <button
                  key={n.id}
                  className="map-node"
                  style={{ left: positions.get(n.id)?.x, top: positions.get(n.id)?.y }}
                  onClick={() => inspect(n.id)}
                  aria-label={`Inspect ${n.name}`}
                >
                  <span className="eyebrow">
                    {n.type} · {n.statusLabel || n.status}
                  </span>
                  <strong>{n.name}</strong>
                  <span className="muted">{categories[n.category]}</span>
                </button>
              ))}
            </div>
          </div>
          <p className="muted flex items-center gap-2">
            <Network size={16} />
            Drag or use arrow keys to pan. Solid lines: active in sample; dashed: inactive.
          </p>
        </>
      )}
      <LinearTrackDrawer
        node={node}
        track={selected ? tracks[selected] || null : null}
        edges={edges.filter((e) => e.source === selected || e.target === selected)}
        nodes={nodes}
        onClose={close}
        onJumpToHuddle={onJumpToHuddle}
      />
    </div>
  );
}
