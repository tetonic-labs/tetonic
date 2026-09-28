import { useRef, useState } from 'react';
import { prepareAgentImage, saveAgentImage, useAgentImage } from '../../lib/agentImages';

export function AgentImagePicker({ agentId }: { agentId: string }) {
  const input = useRef<HTMLInputElement>(null);
  const image = useAgentImage(agentId);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  return (
    <div>
      <div className="agent-focus-actions">
        <button type="button" disabled={busy} onClick={() => input.current?.click()}>
          {busy ? 'Saving image…' : image ? 'Change image' : 'Add image'}
        </button>
        {image && (
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              try {
                saveAgentImage(agentId, null);
                setError('');
              } catch {
                setError('Could not remove the image. Browser storage may be unavailable.');
              }
            }}
          >
            Reset image
          </button>
        )}
      </div>
      <input
        ref={input}
        type="file"
        hidden
        accept="image/png,image/jpeg,image/webp"
        aria-label="Agent image"
        onChange={async (event) => {
          const file = event.target.files?.[0];
          event.target.value = '';
          if (!file) return;
          setBusy(true);
          setError('');
          try {
            saveAgentImage(agentId, await prepareAgentImage(file));
          } catch (error) {
            setError(error instanceof Error ? error.message : 'Unable to save this image.');
          } finally {
            setBusy(false);
          }
        }}
      />
      <p className="preview-footnote">
        PNG, JPEG, or WebP · Center cropped · Saved in this browser
      </p>
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
