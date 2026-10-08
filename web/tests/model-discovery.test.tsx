import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { AgentModelSelect } from '../src/components/views/AgentModelSelect';
import type { ProviderModelCatalog } from '../src/lib/localEngine';

function Form({
  discover,
  keySaved = true,
  provider = 'openai',
  connectionRevision = 0,
}: {
  discover: (provider: string, signal?: AbortSignal) => Promise<ProviderModelCatalog>;
  keySaved?: boolean;
  provider?: string;
  connectionRevision?: number;
}) {
  const [model, setModel] = useState('');
  return (
    <AgentModelSelect
      provider={provider}
      connectionRevision={connectionRevision}
      keySaved={keySaved}
      discover={discover}
      choices={[]}
      defaultModel=""
      model={model}
      customModel=""
      onModel={setModel}
      onCustomModel={vi.fn()}
      connected
    />
  );
}
describe('account model discovery', () => {
  it('shows readable names, freshness and searchable current provider results without changing the selection', async () => {
    render(
      <Form
        provider="anthropic"
        discover={vi.fn().mockResolvedValue({
          provider: 'anthropic',
          models: ['future-model', 'other-model'],
          capabilities_verified: false,
          fetched_at: '2026-10-07T16:00:00Z',
          entries: [
            { id: 'future-model', display_name: 'Future research model', created_at: 200 },
            { id: 'other-model', display_name: 'Fast model', created_at: 100 },
          ],
        })}
      />,
    );
    await screen.findByRole('option', { name: 'Future research model · future-model' });
    expect(screen.getByText(/2 models returned by Anthropic · Last checked/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'future-model' } });
    fireEvent.change(screen.getByLabelText('Search models'), { target: { value: 'fast' } });
    expect(screen.getByRole('option', { name: 'Fast model · other-model' })).toBeTruthy();
    expect(screen.getByLabelText('Model')).toHaveProperty('value', 'future-model');
    expect(screen.queryByText(/selected model was not returned/)).toBeNull();
    fireEvent.change(screen.getByLabelText('Search models'), { target: { value: 'unmatched' } });
    expect(screen.getByText(/No models match/)).toBeTruthy();
    expect(screen.getByLabelText('Model')).toHaveProperty('value', 'future-model');
  });

  it('keeps a failed refresh visibly stale and never replaces the chosen model', async () => {
    const discover = vi
      .fn()
      .mockResolvedValueOnce({
        provider: 'openai',
        models: ['chosen-model'],
        capabilities_verified: false,
      })
      .mockRejectedValueOnce(new Error('Provider unavailable.'));
    render(<Form discover={discover} />);
    await screen.findByRole('option', { name: 'chosen-model' });
    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'chosen-model' } });
    fireEvent.click(screen.getByRole('button', { name: 'Refresh models' }));
    await screen.findByRole('alert');
    expect(screen.getByRole('alert').textContent).toContain('Showing the last successful list');
    expect(screen.getByLabelText('Model')).toHaveProperty('value', 'chosen-model');
  });

  it('clears the previous account catalog when credentials are replaced', async () => {
    const discover = vi
      .fn()
      .mockResolvedValueOnce({
        provider: 'openai',
        models: ['old-account-model'],
        capabilities_verified: false,
      })
      .mockRejectedValueOnce(new Error('New key rejected.'));
    const view = render(<Form discover={discover} />);
    await screen.findByRole('option', { name: 'old-account-model' });
    view.rerender(<Form discover={discover} connectionRevision={1} />);
    await screen.findByRole('alert');
    expect(screen.queryByRole('option', { name: 'old-account-model' })).toBeNull();
    expect(screen.queryByText(/Showing the last successful list/)).toBeNull();
  });

  it('offers the official public catalog before connecting, without inventing account models', () => {
    const discover = vi.fn();
    render(<Form provider="google" keySaved={false} discover={discover} />);
    expect(screen.getByRole('link', { name: /Browse Google/ })).toHaveProperty(
      'href',
      'https://ai.google.dev/gemini-api/docs/models',
    );
    expect(screen.getByText(/Your API key determines account access/)).toBeTruthy();
    expect(discover).not.toHaveBeenCalled();
    expect(screen.getAllByRole('option')).toHaveLength(2);
  });

  it('uses the account catalog and retains the selection when refreshed availability changes', async () => {
    const discover = vi
      .fn()
      .mockResolvedValueOnce({
        provider: 'openai',
        models: ['account-model'],
        capabilities_verified: false,
      })
      .mockResolvedValueOnce({
        provider: 'openai',
        models: ['another-model'],
        capabilities_verified: false,
      });
    render(<Form discover={discover} />);
    await screen.findByRole('option', { name: 'account-model' });
    expect(screen.queryByRole('option', { name: 'gpt-4.1' })).toBeNull();
    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'account-model' } });
    fireEvent.click(screen.getByRole('button', { name: 'Refresh models' }));
    await screen.findByRole('option', { name: 'another-model' });
    expect(screen.getByLabelText('Model')).toHaveProperty('value', 'account-model');
    expect(screen.getByText(/selected model was not returned/)).toBeTruthy();
  });
  it('shows discovery failures and keeps manual model entry available', async () => {
    render(
      <Form discover={vi.fn().mockRejectedValue(new Error('Check provider account access.'))} />,
    );
    await screen.findByText('Check provider account access.');
    fireEvent.change(screen.getByLabelText('Model'), { target: { value: 'custom' } });
    expect(screen.getByLabelText('Model identifier')).toBeTruthy();
  });
  it('does not discover without credentials and ignores results after credentials are removed', async () => {
    let finish!: (catalog: ProviderModelCatalog) => void;
    const discover = vi.fn().mockImplementation(
      () =>
        new Promise<ProviderModelCatalog>((resolve) => {
          finish = resolve;
        }),
    );
    const view = render(<Form discover={discover} keySaved={false} />);
    expect(discover).not.toHaveBeenCalled();
    view.rerender(<Form discover={discover} keySaved />);
    await waitFor(() => expect(discover).toHaveBeenCalledOnce());
    const signal = discover.mock.calls[0][1] as AbortSignal;
    view.rerender(<Form discover={discover} keySaved={false} />);
    expect(signal.aborted).toBe(true);
    await act(async () => {
      finish({ provider: 'openai', models: ['stale-model'], capabilities_verified: false });
    });
    expect(screen.queryByRole('option', { name: 'stale-model' })).toBeNull();
  });
});
