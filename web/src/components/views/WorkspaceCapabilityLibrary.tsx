import { useEffect, useId, useRef, useState } from 'react';
import { ArrowLeft, ArrowUpRight, BookOpen, Plug, Upload, Terminal } from 'lucide-react';
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
  const nameHintId = useId();
  const editor = useRef<HTMLFieldSetElement>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const importButton = useRef<HTMLButtonElement>(null);
  const [paste, setPaste] = useState(false);
  useEffect(() => {
    if (mode !== 'browse') editor.current?.querySelector('input')?.focus();
  }, [mode]);
  function back() {
    const trigger = mode === 'create' ? createButton : importButton;
    setMode('browse');
    setError('');
    requestAnimationFrame(() => trigger.current?.focus());
  }
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
      {mode === 'browse' ? (
        <>
          <h3>Add an ability</h3>
          <p>Available to your workspace. Assigned to agents by you.</p>
          <div className="capability-choices">
            <button
              ref={createButton}
              type="button"
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
              ref={importButton}
              type="button"
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
                <strong>Explore MCP servers</strong>
                <small>Open the official registry. Setup is handled by your engine operator.</small>
              </span>
              <ArrowUpRight size={16} className="capability-choice-arrow" />
            </a>
          </div>
          <div className="ability-basics">
            <Terminal size={18} />
            <p>
              <strong>Need file or terminal tools?</strong>
              <br />
              They’re built in. Select them in the agent editor.
            </p>
          </div>
          {!supported && (
            <p className="capability-status" role="status">
              Skills need a newer engine version. Restart with the updated engine to create or
              import them.
            </p>
          )}
        </>
      ) : (
        <button type="button" className="quiet-back" onClick={back} disabled={busy}>
          <ArrowLeft size={15} /> All abilities
        </button>
      )}
      {mode !== 'browse' && (
        <fieldset ref={editor} disabled={busy} className="capability-editor">
          <legend>{mode === 'create' ? 'Create a skill' : 'Review your skill'}</legend>
          <p>
            {mode === 'create'
              ? 'Teach an approach once. Reuse it across your agents.'
              : 'Choose a SKILL.md file, then review the instructions before adding it.'}
          </p>
          {mode === 'create' ? (
            <>
              <label>
                Skill name
                <input
                  aria-describedby={nameHintId}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="research-sources"
                  maxLength={64}
                />
              </label>
              <p className="capability-name-hint" id={nameHintId}>
                Lowercase words separated by hyphens, like research-sources.
              </p>
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
              <label className="capability-file">
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
              {!content && !paste && (
                <button type="button" className="capability-paste" onClick={() => setPaste(true)}>
                  Or paste instructions
                </button>
              )}
              {(!!content || paste) && (
                <label>
                  Review instructions
                  <textarea
                    autoFocus
                    value={content}
                    onChange={(e) => setContent(e.target.value)}
                    rows={12}
                    placeholder="Or paste the complete SKILL.md, including its name and description frontmatter."
                    maxLength={32768}
                  />
                </label>
              )}
            </>
          )}
          <details className="capability-source">
            <summary>
              Source & attribution <span>Optional</span>
            </summary>
            <label>
              Source
              <input
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder="Author, repository, or source URL"
                maxLength={1024}
              />
            </label>
          </details>
          <p className="capability-scope-note">
            Adds instructions only, not scripts or extra permissions.
            {mode === 'import' && ' Bundled reference files are not imported.'}
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
            <button type="button" onClick={back}>
              Cancel
            </button>
          </div>
        </fieldset>
      )}
      {message && (
        <p className="capability-status" role="status">
          {message}
        </p>
      )}
      {error && (
        <p className="capability-status" role="alert">
          {error}
        </p>
      )}
      {mode === 'browse' && (
        <details className="capability-help">
          <summary>What can I add?</summary>
          <p>
            The MCP registry lists servers; browsing does not install or authorize them. This engine
            currently supports operator-configured local read tools. Remote sign-in and service
            changes are not connected yet.
          </p>
          <p>
            Core file and terminal tools are supplied by your engine. Choose them in the agent
            editor.
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
      )}
    </section>
  );
}
