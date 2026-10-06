import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PalettePicker } from '../src/components/ui/PalettePicker';

describe('PalettePicker utility button and popover', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-palette');
    const existingStyle = document.getElementById('tetonic-custom-palette-style');
    if (existingStyle) existingStyle.remove();
  });

  it('renders a compact utility button matching the header icon style', () => {
    render(<PalettePicker />);
    const button = screen.getByRole('button', { name: /Color palette:/i });
    expect(button).toBeTruthy();
    expect(button.classList.contains('utility-button')).toBe(true);
    expect(button.classList.contains('palette-utility-btn')).toBe(true);
  });

  it('expands popover menu on click with all 5 color palettes plus Custom Palette', async () => {
    const user = userEvent.setup();
    render(<PalettePicker />);
    const trigger = screen.getByRole('button', { name: /Color palette:/i });

    // Initially menu is closed
    expect(screen.queryByRole('menu', { name: 'Choose color palette' })).toBeNull();

    // Click trigger opens popover
    await user.click(trigger);
    expect(screen.getByRole('menu', { name: 'Choose color palette' })).toBeTruthy();
    expect(screen.getByText('Classic Tetonic')).toBeTruthy();
    expect(screen.getByText('Shrigley Primaries')).toBeTruthy();
    expect(screen.getByText('Warhol Screenprint')).toBeTruthy();
    expect(screen.getByText('Electric Sunset')).toBeTruthy();
    expect(screen.getByText('Studio Coral & Cobalt')).toBeTruthy();
    expect(screen.getByText('Custom Palette')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Customize your palette' })).toBeTruthy();
  });

  it('switches palette and sets data-palette on root when selecting a palette', async () => {
    const user = userEvent.setup();
    render(<PalettePicker />);
    const trigger = screen.getByRole('button', { name: /Color palette:/i });

    await user.click(trigger);
    const warholBtn = screen.getByText('Warhol Screenprint').closest('button')!;
    await user.click(warholBtn);

    expect(document.documentElement.getAttribute('data-palette')).toBe('warhol');
    expect(localStorage.getItem('tetonic_palette_exp')).toBe('warhol');

    // Reopen and select Classic Tetonic (default)
    await user.click(trigger);
    const defaultBtn = screen.getByText('Classic Tetonic').closest('button')!;
    await user.click(defaultBtn);

    expect(document.documentElement.getAttribute('data-palette')).toBeNull();
    expect(localStorage.getItem('tetonic_palette_exp')).toBe('default');
  });

  it('activates custom palette and dynamically injects CSS custom properties', async () => {
    const user = userEvent.setup();
    render(<PalettePicker />);
    const trigger = screen.getByRole('button', { name: /Color palette:/i });

    await user.click(trigger);
    const customSelectBtn = screen.getByText('Custom Palette').closest('button')!;
    await user.click(customSelectBtn);

    expect(document.documentElement.getAttribute('data-palette')).toBe('custom');
    expect(localStorage.getItem('tetonic_palette_exp')).toBe('custom');

    const styleEl = document.getElementById('tetonic-custom-palette-style');
    expect(styleEl).toBeTruthy();
    expect(styleEl?.textContent).toContain('html[data-palette="custom"]');
    expect(styleEl?.textContent).toContain('--color-accent:');
  });

  it('opens custom palette builder and allows selecting curated swatches or color pickers', async () => {
    const user = userEvent.setup();
    render(<PalettePicker />);
    const trigger = screen.getByRole('button', { name: /Color palette:/i });

    await user.click(trigger);
    const editBtn = screen.getByRole('button', { name: 'Customize your palette' });
    await user.click(editBtn);

    // Should now be in builder view
    expect(screen.getByText('Live Preview')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Back to all palettes' })).toBeTruthy();

    // Curated accent swatches are available
    const emeraldAccent = screen.getByRole('button', { name: 'Accent Emerald' });
    expect(emeraldAccent).toBeTruthy();
    await user.click(emeraldAccent);

    // Native color pickers are available
    const accentPicker = screen.getByLabelText('Custom accent color picker');
    expect(accentPicker).toBeTruthy();
    fireEvent.change(accentPicker, { target: { value: '#123456' } });

    // Custom CSS style tag should be updated with new color
    const styleEl = document.getElementById('tetonic-custom-palette-style');
    expect(styleEl?.textContent).toContain('--color-accent: #123456');

    // Reset restores defaults
    const resetBtn = screen.getByRole('button', { name: 'Reset colors' });
    await user.click(resetBtn);
    expect(styleEl?.textContent).toContain('--color-accent: #0538ff');

    // Click back to return to all palettes
    const backBtn = screen.getByRole('button', { name: 'Back to all palettes' });
    await user.click(backBtn);
    expect(screen.getByText('Shrigley Primaries')).toBeTruthy();
  });
});
