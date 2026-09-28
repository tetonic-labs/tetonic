import { useId } from 'react';
import { Agent } from '../../types';
import { teammate } from '../../lib/teammates';
import { useAgentImage } from '../../lib/agentImages';

export function Portrait({ agent, size = 40 }: { agent: Agent; size?: number }) {
  const clip = useId();
  const image = useAgentImage(agent.id);
  if (image)
    return (
      <img
        src={image}
        alt=""
        width={size}
        height={size}
        className="teammate-portrait"
        style={{ borderRadius: '50%', objectFit: 'cover' }}
      />
    );
  const hash = [...agent.id].reduce((sum, char) => sum + char.charCodeAt(0), 0);
  const known: Record<string, number> = {
    'agt-builder': 0,
    'agt-doc': 1,
    'agt-remote-guard': 2,
    'agt-sentinel': 3,
  };
  const variant = known[agent.id] ?? hash % 4;
  const skin = ['#EDC6A0', '#F1C9A9', '#BA7E58', '#DEB697'][variant];
  const hair = ['#50392D', '#45342D', '#332A27', '#6D655B'][variant];
  const shirt = ['#49645C', '#6D6551', '#655572', '#455A73'][variant];
  const background =
    known[agent.id] === undefined
      ? ['#C2D0B8', '#D8B39F', '#C1C4D8', '#D4C395'][hash % 4]
      : teammate(agent).color;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 48 48"
      aria-hidden="true"
      className="teammate-portrait"
    >
      <defs>
        <clipPath id={clip}>
          <circle cx="24" cy="24" r="24" />
        </clipPath>
      </defs>
      <g clipPath={`url(#${clip})`}>
        <circle cx="24" cy="24" r="24" fill={background} />
        {variant === 1 && <path d="M10 24C5 43 10 48 17 48H32C40 42 44 29 37 16Z" fill={hair} />}
        <path d="M5 49c1-11 7-16 19-16s18 5 19 16" fill={shirt} />
        <path d="M19 32v7q5 5 10 0v-7" fill={skin} />
        <ellipse cx="24" cy="22" rx="12" ry="14" fill={skin} />
        <path
          d={
            variant === 1
              ? 'M11 27V19C11 3 39 2 37 24L32 16 22 13 14 25Z'
              : 'M12 20C9 7 22 4 32 10L37 18 28 15 18 16Z'
          }
          fill={hair}
        />
        {variant === 2 && (
          <path
            d="M11 18Q6 12 13 10Q10 3 20 6Q25 0 31 7Q40 6 38 15L35 20L29 15 23 16 16 15Z"
            fill={hair}
          />
        )}
        {variant === 0 && (
          <path d="M13 24Q15 36 24 36Q34 35 35 24L31 29Q24 34 18 28Z" fill={hair} />
        )}
        <circle cx="20" cy="23" r="1.2" fill="#33251E" />
        <circle cx="29" cy="23" r="1.2" fill="#33251E" />
        <path
          d="M21 29q4 3 7-1"
          fill="none"
          stroke="#80543D"
          strokeWidth="1.4"
          strokeLinecap="round"
        />
        {(variant === 2 || variant === 3) && (
          <g fill="none" stroke={variant === 2 ? '#E1D3B8' : '#33251E'} strokeWidth="1.2">
            <rect x="15" y="20" width="9" height="7" rx="2" />
            <rect x="25" y="20" width="9" height="7" rx="2" />
            <path d="M24 22h1" />
          </g>
        )}
      </g>
    </svg>
  );
}
