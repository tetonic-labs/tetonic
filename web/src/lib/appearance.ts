// Shared by the picker and the synchronous HTML startup script. Keep this module import-free.
export type PaletteId = 'default' | 'shrigley' | 'warhol' | 'electric' | 'coral' | 'custom';

export interface CustomColors {
  accent: string;
  highlight: string;
  ground: string;
}

export const DEFAULT_CUSTOM_COLORS: CustomColors = {
  accent: '#0538ff',
  highlight: '#ffd100',
  ground: '#faf8f3',
};

const STORAGE_KEY = 'tetonic_palette_exp';
const CUSTOM_STORAGE_KEY = 'tetonic_palette_custom_colors';

export function getLuminance(hex: string): number {
  const cleanHex = hex.replace('#', '');
  if (cleanHex.length !== 6) return 0.5;
  const r = parseInt(cleanHex.substring(0, 2), 16) / 255;
  const g = parseInt(cleanHex.substring(2, 4), 16) / 255;
  const b = parseInt(cleanHex.substring(4, 6), 16) / 255;
  const a = [r, g, b].map((v) => (v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)));
  return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
}

function adjustHex(hex: string, amount: number): string {
  const cleanHex = hex.replace('#', '');
  if (cleanHex.length !== 6) return hex;
  const num = parseInt(cleanHex, 16);
  let r = (num >> 16) + amount;
  let g = ((num >> 8) & 0x00ff) + amount;
  let b = (num & 0x0000ff) + amount;
  r = Math.min(255, Math.max(0, r));
  g = Math.min(255, Math.max(0, g));
  b = Math.min(255, Math.max(0, b));
  return `#${((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1)}`;
}

export function applyCustomCss(colors: CustomColors) {
  if (typeof document === 'undefined') return;
  let styleEl = document.getElementById('tetonic-custom-palette-style') as HTMLStyleElement | null;
  if (!styleEl) {
    styleEl = document.createElement('style');
    styleEl.id = 'tetonic-custom-palette-style';
    document.head.appendChild(styleEl);
  }

  const isLight = getLuminance(colors.ground) > 0.4;
  const textPrimary = isLight ? '#121212' : '#faf8f3';
  const textSecondary = isLight ? '#4a4a4a' : '#b8b8b8';
  const textMuted = isLight ? '#6e6e6e' : '#8e8e8e';
  const bgSurface = isLight ? '#ffffff' : adjustHex(colors.ground, 16);
  const bgSidebar = isLight ? adjustHex(colors.ground, -6) : adjustHex(colors.ground, -8);
  const bgElevated = isLight ? adjustHex(colors.ground, -12) : adjustHex(colors.ground, 24);
  const bgHover = isLight ? adjustHex(colors.ground, -8) : adjustHex(colors.ground, 32);
  const bgSunken = isLight ? adjustHex(colors.ground, -22) : adjustHex(colors.ground, -14);
  const borderColor = isLight ? 'rgba(18, 18, 18, 0.16)' : 'rgba(250, 248, 243, 0.16)';
  const borderSubtle = isLight ? 'rgba(18, 18, 18, 0.08)' : 'rgba(250, 248, 243, 0.08)';
  const accentHover = adjustHex(colors.accent, isLight ? -25 : 25);

  styleEl.textContent = `
    html[data-palette="custom"] {
      --brand-field: ${colors.highlight};
      --color-highlight: ${colors.highlight};
      --color-accent: ${colors.accent};
      --color-accent-hover: ${accentHover};
      --color-ground: ${textPrimary};
      --color-off-white: ${colors.ground};

      --bg-ground: ${colors.ground};
      --bg-surface: ${bgSurface};
      --bg-sidebar: ${bgSidebar};
      --bg-surface-elevated: ${bgElevated};
      --bg-surface-hover: ${bgHover};
      --bg-sunken: ${bgSunken};

      --text-primary: ${textPrimary};
      --text-secondary: ${textSecondary};
      --text-muted: ${textMuted};
      --color-mid-text: ${textSecondary};

      --color-border: ${borderColor};
      --color-border-subtle: ${borderSubtle};
      --brand-rule: ${borderColor};
      --control-border: ${isLight ? '#808080' : '#666666'};

      --canvas-paper: ${colors.ground};
      --canvas-panel: ${bgSurface};
      --canvas-ink: ${textPrimary};
      --canvas-muted: ${textSecondary};
      --canvas-line: ${borderColor};
      --canvas-copper: ${colors.accent};
      --room-slab: ${bgElevated};
      --surface-ink: ${isLight ? '#121212' : '#0a0a0a'};
      --on-ink: #faf8f3;
    }
  `;
}

const MODE_STORAGE_KEY = 'tetonic_appearance_dark';
const PALETTES: PaletteId[] = ['default', 'shrigley', 'warhol', 'electric', 'coral', 'custom'];

export function readAppearance(): { palette: PaletteId; colors: CustomColors; dark: boolean } {
  let palette: PaletteId = 'default';
  let colors = DEFAULT_CUSTOM_COLORS;
  let dark = false;
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (PALETTES.includes(saved as PaletteId)) palette = saved as PaletteId;
    dark = localStorage.getItem(MODE_STORAGE_KEY) === 'true';
    const parsed = JSON.parse(localStorage.getItem(CUSTOM_STORAGE_KEY) || 'null');
    if (
      parsed &&
      [parsed.accent, parsed.highlight, parsed.ground].every(
        (value) => typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value),
      )
    ) {
      colors = { accent: parsed.accent, highlight: parsed.highlight, ground: parsed.ground };
    }
  } catch {
    /* Storage can be unavailable; opening the app must still work. */
  }
  return { palette, colors, dark };
}

export function applyPalette(palette: PaletteId, colors: CustomColors) {
  applyCustomCss(colors);
  if (palette === 'default') document.documentElement.removeAttribute('data-palette');
  else document.documentElement.setAttribute('data-palette', palette);
  document.dispatchEvent(new CustomEvent('tetonic:palettechange'));
}

export function savePalette(palette: PaletteId, colors: CustomColors) {
  applyPalette(palette, colors);
  try {
    localStorage.setItem(CUSTOM_STORAGE_KEY, JSON.stringify(colors));
    localStorage.setItem(STORAGE_KEY, palette);
  } catch {
    /* Keep the current page usable if persistence is unavailable. */
  }
}

export function saveDarkAppearance(dark: boolean) {
  document.documentElement.classList.toggle('dark', dark);
  try {
    localStorage.setItem(MODE_STORAGE_KEY, String(dark));
  } catch {
    /* Optional persistence. */
  }
  document.dispatchEvent(new CustomEvent('tetonic:palettechange'));
}

export function restoreSavedAppearance() {
  const saved = readAppearance();
  document.documentElement.classList.toggle('dark', saved.dark);
  applyPalette(saved.palette, saved.colors);
}
