import { ArrowUpRight, Users } from 'lucide-react';

// A still illustration for an empty workspace, never a simulation of agent activity.
const seeds = Array.from({ length: 180 }, (_, i) => {
  const angle = i * 2.399963;
  const spread = Math.sqrt(i / 180);
  return {
    x: 160 + Math.cos(angle) * spread * 112,
    y: 72 + Math.sin(angle) * spread * 30 + Math.sin(i * 0.6) * 5,
    r: 0.7 + (i % 4) * 0.3,
  };
});

export function WorkspaceWelcome({
  onAgents,
  onTeams,
}: {
  onAgents: () => void;
  onTeams: () => void;
}) {
  return (
    <section className="arrival-welcome" aria-label="Welcome to your workspace">
      <svg viewBox="0 0 320 140" className="arrival-seeds" aria-hidden="true">
        {seeds.map((seed, index) => (
          <circle
            key={index}
            cx={seed.x}
            cy={seed.y}
            r={seed.r}
            opacity={0.25 + (index % 7) * 0.1}
          />
        ))}
      </svg>
      <span className="arrival-eyebrow">Room for what’s next</span>
      <h1>
        What would you like
        <br />
        to move forward?
      </h1>
      <p>
        Think it through with your Guide.
        <br />
        Bring in agents when you’re ready to put ideas to work.
      </p>
      <div>
        <button onClick={onAgents}>
          <Users size={15} /> Meet your agents <ArrowUpRight size={15} />
        </button>
        <button onClick={onTeams}>
          Create a team <ArrowUpRight size={15} />
        </button>
      </div>
    </section>
  );
}
