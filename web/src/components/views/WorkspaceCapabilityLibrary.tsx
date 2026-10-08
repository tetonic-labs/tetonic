import { useRef, useState } from 'react';
import { ArrowUpRight, BookOpen, Plug, Upload } from 'lucide-react';
import type { LocalEngine } from '../../lib/localEngine';
import '../../tools.css';

/** Embedded in both the toolkit and the agent editor; never replaces the agent draft. */
export function WorkspaceCapabilityLibrary({
  client,
  supported,
  onChanged,
}: {
  client: LocalEngine;
  supported: boolean;
  onChanged: () => Promise<void>;
}) {
  const [mode, setMode] = useState<'browse' | 'create' | 'import'>('browse');
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [instructions, setInstructions] = useState('');
  const [content, setContent] = useState('');
  const [source, setSource] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const locked = useRef(false);
  const fileRevision = useRef(0);
  async function save() {
    if (locked.current || !supported) return;
    locked.current = true;
    setBusy(true);
    setError('');
    setMessage('');
    try {
      const markdown =
        mode === 'create'
          ? `---\nname: ${JSON.stringify(name.trim())}\ndescription: ${JSON.stringify(description.trim())}\n---\n\n${instructions.trim()}\n`
          : content;
      const saved = await client.importSkill({
        content: markdown,
        source:
          source.trim() || (mode === 'create' ? 'Written in this workspace' : 'Imported SKILL.md'),
      });
      await onChanged();
      setMessage(
        saved && !saved.enabled
          ? 'This version was previously revoked. Importing it again does not restore access.'
          : 'Saved to the workspace. Select the skill in an agent’s settings to give them access.',
      );
      setMode('browse');
      setContent('');
      setInstructions('');
      setName('');
      setDescription('');
      setSource('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not add skill.');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  return (
    <section
      className="capability-library"
      aria-label="Add workspace capabilities"
      onKeyDown={(event) => {
        // This editor also lives inside the agent form. Enter in a skill field
        // must not submit that surrounding form and discard the in-progress skill.
        if (
          event.key === 'Enter' &&
          event.target instanceof HTMLInputElement &&
          event.target.type !== 'file'
        ) {
          event.preventDefault();
        }
      }}
    >
      <h3>Add to your workspace</h3>
      <p>Bring in a skill or connect a service. Then choose which agents can use it.</p>
      <div className="capability-choices">
        <button
          type="button"
          aria-pressed={mode === 'create'}
          disabled={!supported || busy}
          onClick={() => {
            setMode('create');
            setMessage('');
            setError('');
          }}
        >
          <BookOpen size={19} />
          <span>
            <strong>Write a skill</strong>
            <small>Give agents a reusable way to work.</small>
          </span>
        </button>
        <button
          type="button"
          aria-pressed={mode === 'import'}
          disabled={!supported || busy}
          onClick={() => {
            setMode('import');
            setMessage('');
            setError('');
          }}
        >
          <Upload size={19} />
          <span>
            <strong>Import a skill</strong>
            <small>Review and add a SKILL.md file.</small>
          </span>
        </button>
        <a href="https://registry.modelcontextprotocol.io/" target="_blank" rel="noreferrer">
          <Plug size={19} />
          <span>
            <strong>
              Explore MCP servers <ArrowUpRight size={13} />
            </strong>
            <small>Browse the official registry.</small>
          </span>
        </a>
      </div>
      {!supported && (
        <p role="status">Restart with the updated engine to enable workspace skills.</p>
      )}
      {mode !== 'browse' && (
        <fieldset disabled={busy} className="capability-editor">
          <legend>{mode === 'create' ? 'Create a skill' : 'Review your skill'}</legend>
          {mode === 'create' ? (
            <>
              <label>
                Skill name
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="research-sources"
                  maxLength={64}
                />
              </label>
              <label>
                When should an agent use it?
                <textarea
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="When researching a question that needs reliable sources."
                  maxLength={1024}
                  rows={2}
                />
              </label>
              <label>
                Instructions
                <textarea
                  value={instructions}
                  onChange={(e) => setInstructions(e.target.value)}
                  placeholder="Describe the approach, what a good result looks like, and when to ask for help."
                  rows={7}
                  maxLength={28000}
                />
              </label>
            </>
          ) : (
            <>
              <label>
                SKILL.md file
                <input
                  type="file"
                  accept=".md,text/markdown,text/plain"
                  onChange={async (e) => {
                    const revision = ++fileRevision.current;
                    const file = e.target.files?.[0];
                    setContent('');
                    setError('');
                    if (!file) return;
                    if (file.size > 32768) {
                      setError('Choose a SKILL.md file smaller than 32 KiB.');
                      return;
                    }
                    try {
                      const text = await file.text();
                      if (revision === fileRevision.current) {
                        setContent(text);
                        setSource(file.name);
                      }
                    } catch {
                      if (revision === fileRevision.current) setError('Could not read this file.');
                    }
                  }}
                />
              </label>
              <label>
                Review instructions
                <textarea
                  value={content}
                  onChange={(e) => setContent(e.target.value)}
                  rows={12}
                  placeholder="Or paste the complete SKILL.md, including its name and description frontmatter."
                  maxLength={32768}
                />
              </label>
            </>
          )}
          <label>
            Source <span>(optional)</span>
            <input
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder="Author, repository, or source URL"
              maxLength={1024}
            />
          </label>
          <p>
            Instructions only. Bundled scripts and reference files are not imported. A skill uses
            the tools and permissions you separately grant to the agent.
          </p>
          <div className="capability-actions">
            <button
              type="button"
              className="canvas-primary"
              disabled={
                busy ||
                (mode === 'create'
                  ? !name.trim() || !description.trim() || !instructions.trim()
                  : !content.trim())
              }
              onClick={() => void save()}
            >
              {busy ? 'Adding…' : 'Add skill to workspace'}
            </button>
            <button type="button" onClick={() => setMode('browse')}>
              Cancel
            </button>
          </div>
        </fieldset>
      )}
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
      <details>
        <summary>Connecting services and finding skills</summary>
        <p>
          The MCP registry lists servers; browsing does not install or authorize them. This engine
          currently supports operator-configured local read tools. Remote sign-in and service
          changes are not connected yet.
        </p>
        <p>
          Core file and terminal tools are supplied by your engine. Choose them in the agent editor.
        </p>
        <p>
          <a href="https://github.com/anthropics/skills" target="_blank" rel="noreferrer">
            Browse Anthropic’s skill examples <ArrowUpRight size={13} />
          </a>{' '}
          ·{' '}
          <a href="https://agentskills.io/specification" target="_blank" rel="noreferrer">
            SKILL.md format
          </a>
        </p>
      </details>
    </section>
  );
}
