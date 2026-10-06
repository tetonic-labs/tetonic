import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { AgentModelSelect } from '../src/components/views/AgentModelSelect';
import type { ProviderModelCatalog } from '../src/lib/localEngine';

function Form({
  discover,
  keySaved = true,
}: {
  discover: (provider: string, signal?: AbortSignal) => Promise<ProviderModelCatalog>;
  keySaved?: boolean;
}) {
  const [model, setModel] = useState('');
  return (
    <AgentModelSelect
      provider="openai"
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
