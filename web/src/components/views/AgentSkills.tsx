import type { WorkspaceSkill } from '../../lib/localEngine';

export function AgentSkills({
  skills,
  selected,
  supported,
  onSelect,
  disabled,
}: {
  skills: WorkspaceSkill[];
  selected: string[];
  supported: string[];
  onSelect: (ids: string[]) => void;
  disabled: boolean;
}) {
  const missing = selected.filter(
    (id) => id.startsWith('skill_') && !skills.some((s) => s.id === id),
  );
  return (
    <section
      className="agent-skills"
      aria-label="Agent skills"
      data-empty={!skills.length && !missing.length}
    >
      <h4>Skills</h4>
      <p>
        {skills.length || missing.length
          ? 'Reusable instructions for how to work.'
          : 'No skills added yet. Add reusable instructions below.'}
      </p>
      {skills.map((skill) => (
        <label
          key={skill.id}
          className="capability-skill-option"
          data-selected={selected.includes(skill.id)}
        >
          <input
            type="checkbox"
            checked={selected.includes(skill.id)}
            disabled={
              disabled ||
              (!selected.includes(skill.id) && (!skill.enabled || !supported.includes(skill.id)))
            }
            onChange={(e) =>
              onSelect(
                e.target.checked
                  ? [...selected, skill.id]
                  : selected.filter((id) => id !== skill.id),
              )
            }
          />
          <span>
            <strong>{skill.name}</strong>
            <small>
              {skill.enabled
                ? skill.description
                : 'Access revoked. Remove this skill to resume work.'}
            </small>
            <small>Version {skill.id.slice(6, 14)}</small>
          </span>
        </label>
      ))}
      {missing.map((id) => (
        <label key={id} className="capability-skill-option">
          <input
            type="checkbox"
            checked
            disabled={disabled}
            onChange={() => onSelect(selected.filter((s) => s !== id))}
          />
          <span>
            Unavailable skill <small>{id.slice(6, 14)} · Remove to update this agent.</small>
          </span>
        </label>
      ))}
    </section>
  );
}
