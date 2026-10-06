import { useEffect, useState, useMemo } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { Palette, Check, ChevronLeft, Sliders, Pipette, RotateCcw } from 'lucide-react';

export type PaletteId = 'default' | 'shrigley' | 'warhol' | 'electric' | 'coral' | 'custom';

export interface CustomColors {
  accent: string;
  highlight: string;
  ground: string;
}

interface PaletteOption {
  id: PaletteId;
  name: string;
  tagline: string;
}

export const PRESET_PALETTES: PaletteOption[] = [
  {
    id: 'default',
    name: 'Classic Tetonic',
    tagline: 'Warm Rust, Clay & Paper',
  },
  {
    id: 'shrigley',
    name: 'Shrigley Primaries',
    tagline: 'Canary Yellow, Poppy & Cobalt',
  },
  {
    id: 'warhol',
    name: 'Warhol Screenprint',
    tagline: 'Turquoise, Sun & Magenta',
  },
  {
    id: 'electric',
    name: 'Electric Sunset',
    tagline: 'Coral-Pink, Banana & Ultraviolet',
  },
  {
    id: 'coral',
    name: 'Studio Coral & Cobalt',
    tagline: 'Tangerine, Cobalt & Cream',
  },
];

export const DEFAULT_CUSTOM_COLORS: CustomColors = {
  accent: '#0538ff',
  highlight: '#ffd100',
  ground: '#faf8f3',
};

export const CURATED_ACCENTS = [
  { name: 'Cobalt', hex: '#0538ff' },
  { name: 'Magenta', hex: '#ff007f' },
  { name: 'Coral', hex: '#ff4d6d' },
  { name: 'Tangerine', hex: '#f26444' },
  { name: 'Ultraviolet', hex: '#590de5' },
  { name: 'Emerald', hex: '#0a8f5c' },
  { name: 'Rust', hex: '#984629' },
  { name: 'Turquoise', hex: '#00b4d8' },
];

export const CURATED_HIGHLIGHTS = [
  { name: 'Sun Yellow', hex: '#ffd100' },
  { name: 'Hot Poppy', hex: '#ff2a6d' },
  { name: 'Warm Amber', hex: '#ff9f1c' },
  { name: 'Neon Mint', hex: '#2ec4b6' },
  { name: 'Lilac', hex: '#b185db' },
  { name: 'Sky Cerulean', hex: '#38bdf8' },
  { name: 'Banana', hex: '#ffd166' },
];

export const CURATED_SURFACES = [
  { name: 'Warm Paper', hex: '#faf8f3' },
  { name: 'Crisp White', hex: '#ffffff' },
  { name: 'Cream Linen', hex: '#f7efe2' },
  { name: 'Inky Slate', hex: '#121212' },
  { name: 'Velvet Violet', hex: '#1a092a' },
  { name: 'Deep Midnight', hex: '#0e1726' },
];

const STORAGE_KEY = 'tetonic_palette_exp';
const CUSTOM_STORAGE_KEY = 'tetonic_palette_custom_colors';

function getLuminance(hex: string): number {
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

function applyCustomCss(colors: CustomColors) {
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

export function PalettePicker() {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<'list' | 'builder'>('list');

  const [customColors, setCustomColors] = useState<CustomColors>(() => {
    if (typeof window === 'undefined') return DEFAULT_CUSTOM_COLORS;
    try {
      const saved = localStorage.getItem(CUSTOM_STORAGE_KEY);
      if (saved) {
        const parsed = JSON.parse(saved);
        if (parsed.accent && parsed.highlight && parsed.ground) return parsed;
      }
    } catch {
      // fallback
    }
    return DEFAULT_CUSTOM_COLORS;
  });

  const [activePalette, setActivePalette] = useState<PaletteId>(() => {
    if (typeof window === 'undefined') return 'shrigley';
    const saved = localStorage.getItem(STORAGE_KEY) as PaletteId | null;
    return saved && (PRESET_PALETTES.some((p) => p.id === saved) || saved === 'custom')
      ? saved
      : 'shrigley';
  });

  // Keep dynamic custom CSS updated
  useEffect(() => {
    applyCustomCss(customColors);
    try {
      localStorage.setItem(CUSTOM_STORAGE_KEY, JSON.stringify(customColors));
    } catch {
      // Ignore quota
    }
  }, [customColors]);

  // Apply active palette attribute
  useEffect(() => {
    const root = document.documentElement;
    if (activePalette === 'default') {
      root.removeAttribute('data-palette');
    } else {
      root.setAttribute('data-palette', activePalette);
    }
    try {
      localStorage.setItem(STORAGE_KEY, activePalette);
    } catch {
      // Ignore storage errors
    }
    document.dispatchEvent(
      new CustomEvent('tetonic:palettechange', { detail: { palette: activePalette } }),
    );
  }, [activePalette]);

  const currentOption = useMemo(() => {
    if (activePalette === 'custom') {
      return { id: 'custom' as PaletteId, name: 'Custom Palette', tagline: 'Personal Mix' };
    }
    return PRESET_PALETTES.find((p) => p.id === activePalette) || PRESET_PALETTES[0];
  }, [activePalette]);

  const customSwatchStyle = useMemo(
    () => ({
      background: `conic-gradient(${customColors.accent} 0deg 120deg, ${customColors.highlight} 120deg 240deg, ${customColors.ground} 240deg 360deg)`,
    }),
    [customColors],
  );

  const isGroundLight = getLuminance(customColors.ground) > 0.4;
  const isAccentLight = getLuminance(customColors.accent) > 0.4;
  const isHighlightLight = getLuminance(customColors.highlight) > 0.4;

  const handleColorChange = (field: keyof CustomColors, value: string) => {
    setCustomColors((prev) => ({ ...prev, [field]: value }));
    if (activePalette !== 'custom') {
      setActivePalette('custom');
    }
  };

  const handleResetCustom = () => {
    setCustomColors(DEFAULT_CUSTOM_COLORS);
    if (activePalette !== 'custom') {
      setActivePalette('custom');
    }
  };

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          // Reset view mode on close
          setTimeout(() => setView('list'), 150);
        }
      }}
    >
      <Popover.Trigger asChild>
        <button
          className="utility-button palette-utility-btn"
          aria-label={`Color palette: ${currentOption.name}. Click to change.`}
          title={`Palette: ${currentOption.name}`}
          data-active-palette={activePalette}
        >
          <Palette size={17} />
          <span
            className="palette-pip"
            data-palette={activePalette}
            style={activePalette === 'custom' ? { background: customColors.accent } : undefined}
            aria-hidden="true"
          />
        </button>
      </Popover.Trigger>

      <Popover.Portal>
        <Popover.Content
          className="palette-popover-content"
          side="bottom"
          align="end"
          sideOffset={8}
          collisionPadding={12}
        >
          {view === 'list' ? (
            <>
              <div className="palette-popover-header">
                <span className="palette-popover-title">Color Palette</span>
                <span className="palette-popover-subtitle">Theme experiment</span>
              </div>

              <div className="palette-menu-list" role="menu" aria-label="Choose color palette">
                {PRESET_PALETTES.map((palette) => {
                  const isActive = activePalette === palette.id;
                  return (
                    <button
                      key={palette.id}
                      type="button"
                      role="menuitem"
                      className={`palette-menu-item ${isActive ? 'is-active' : ''}`}
                      onClick={() => {
                        setActivePalette(palette.id);
                        setOpen(false);
                      }}
                      aria-pressed={isActive}
                    >
                      <span
                        className="palette-swatch-circle"
                        data-palette={palette.id}
                        aria-hidden="true"
                      />
                      <div className="palette-menu-text">
                        <span className="palette-item-name">{palette.name}</span>
                        <span className="palette-item-tagline">{palette.tagline}</span>
                      </div>
                      {isActive && (
                        <Check size={14} className="palette-item-check" aria-hidden="true" />
                      )}
                    </button>
                  );
                })}

                <div className="palette-menu-divider" role="separator" />

                {/* Custom Palette Option */}
                <div
                  className={`palette-menu-item palette-custom-item ${activePalette === 'custom' ? 'is-active' : ''}`}
                  role="menuitem"
                >
                  <button
                    type="button"
                    className="palette-custom-select-area"
                    onClick={() => {
                      setActivePalette('custom');
                      setOpen(false);
                    }}
                    aria-pressed={activePalette === 'custom'}
                  >
                    <span
                      className="palette-swatch-circle palette-custom-swatch"
                      style={customSwatchStyle}
                      aria-hidden="true"
                    />
                    <div className="palette-menu-text">
                      <span className="palette-item-name">Custom Palette</span>
                      <span className="palette-item-tagline">Design your own</span>
                    </div>
                  </button>

                  <div className="palette-custom-actions">
                    {activePalette === 'custom' && (
                      <Check size={14} className="palette-item-check" aria-hidden="true" />
                    )}
                    <button
                      type="button"
                      className="palette-customize-btn"
                      onClick={() => {
                        setActivePalette('custom');
                        setView('builder');
                      }}
                      title="Open Palette Builder"
                      aria-label="Customize your palette"
                    >
                      <Sliders size={13} />
                      <span>Edit</span>
                    </button>
                  </div>
                </div>
              </div>
            </>
          ) : (
            /* Custom Palette Builder View */
            <div className="palette-builder-view">
              <div className="palette-builder-header">
                <button
                  type="button"
                  className="palette-builder-back-btn"
                  onClick={() => setView('list')}
                  aria-label="Back to all palettes"
                >
                  <ChevronLeft size={14} />
                  <span>All Palettes</span>
                </button>
                <button
                  type="button"
                  className="palette-builder-reset-btn"
                  onClick={handleResetCustom}
                  title="Reset to default custom colors"
                  aria-label="Reset colors"
                >
                  <RotateCcw size={12} />
                  <span>Reset</span>
                </button>
              </div>

              {/* Interactive Live Mini-Preview */}
              <div
                className="palette-builder-preview-card"
                style={{
                  backgroundColor: customColors.ground,
                  color: isGroundLight ? '#121212' : '#faf8f3',
                  borderColor: isGroundLight ? 'rgba(0,0,0,0.12)' : 'rgba(255,255,255,0.15)',
                }}
              >
                <div className="preview-card-header">
                  <span
                    className="preview-highlight-pill"
                    style={{
                      backgroundColor: customColors.highlight,
                      color: isHighlightLight ? '#121212' : '#ffffff',
                    }}
                  >
                    Highlight
                  </span>
                  <span className="preview-label">Live Preview</span>
                </div>
                <div className="preview-card-body">
                  <button
                    type="button"
                    className="preview-accent-btn"
                    style={{
                      backgroundColor: customColors.accent,
                      color: isAccentLight ? '#121212' : '#ffffff',
                    }}
                  >
                    Accent Button
                  </button>
                </div>
              </div>

              {/* Color Configuration Sections */}
              <div className="palette-builder-sections">
                {/* 1. Interactive Accent */}
                <div className="palette-builder-row">
                  <div className="builder-row-label">
                    <span>Accent (Buttons & Focus)</span>
                    <span className="builder-color-val">{customColors.accent}</span>
                  </div>
                  <div className="builder-swatches-grid">
                    {CURATED_ACCENTS.map((c) => {
                      const isSelected = customColors.accent.toLowerCase() === c.hex.toLowerCase();
                      return (
                        <button
                          key={c.hex}
                          type="button"
                          className={`builder-swatch-dot ${isSelected ? 'is-selected' : ''}`}
                          style={{ backgroundColor: c.hex }}
                          onClick={() => handleColorChange('accent', c.hex)}
                          title={`${c.name} (${c.hex})`}
                          aria-label={`Accent ${c.name}`}
                        />
                      );
                    })}
                    {/* Custom Native Color Picker */}
                    <label
                      className={`builder-color-picker-label ${!CURATED_ACCENTS.some((c) => c.hex.toLowerCase() === customColors.accent.toLowerCase()) ? 'is-selected' : ''}`}
                      title="Pick custom accent color"
                    >
                      <input
                        type="color"
                        className="builder-native-picker"
                        value={customColors.accent}
                        onChange={(e) => handleColorChange('accent', e.target.value)}
                        aria-label="Custom accent color picker"
                      />
                      <Pipette size={11} />
                      <span>Custom</span>
                    </label>
                  </div>
                </div>

                {/* 2. Highlight Pop */}
                <div className="palette-builder-row">
                  <div className="builder-row-label">
                    <span>Highlight (Badges & Tags)</span>
                    <span className="builder-color-val">{customColors.highlight}</span>
                  </div>
                  <div className="builder-swatches-grid">
                    {CURATED_HIGHLIGHTS.map((c) => {
                      const isSelected =
                        customColors.highlight.toLowerCase() === c.hex.toLowerCase();
                      return (
                        <button
                          key={c.hex}
                          type="button"
                          className={`builder-swatch-dot ${isSelected ? 'is-selected' : ''}`}
                          style={{ backgroundColor: c.hex }}
                          onClick={() => handleColorChange('highlight', c.hex)}
                          title={`${c.name} (${c.hex})`}
                          aria-label={`Highlight ${c.name}`}
                        />
                      );
                    })}
                    {/* Custom Native Color Picker */}
                    <label
                      className={`builder-color-picker-label ${!CURATED_HIGHLIGHTS.some((c) => c.hex.toLowerCase() === customColors.highlight.toLowerCase()) ? 'is-selected' : ''}`}
                      title="Pick custom highlight color"
                    >
                      <input
                        type="color"
                        className="builder-native-picker"
                        value={customColors.highlight}
                        onChange={(e) => handleColorChange('highlight', e.target.value)}
                        aria-label="Custom highlight color picker"
                      />
                      <Pipette size={11} />
                      <span>Custom</span>
                    </label>
                  </div>
                </div>

                {/* 3. Surface Tone */}
                <div className="palette-builder-row">
                  <div className="builder-row-label">
                    <span>Surface (Ground Tone)</span>
                    <span className="builder-color-val">{customColors.ground}</span>
                  </div>
                  <div className="builder-swatches-grid">
                    {CURATED_SURFACES.map((c) => {
                      const isSelected = customColors.ground.toLowerCase() === c.hex.toLowerCase();
                      return (
                        <button
                          key={c.hex}
                          type="button"
                          className={`builder-swatch-dot ${isSelected ? 'is-selected' : ''}`}
                          style={{ backgroundColor: c.hex }}
                          onClick={() => handleColorChange('ground', c.hex)}
                          title={`${c.name} (${c.hex})`}
                          aria-label={`Surface ${c.name}`}
                        />
                      );
                    })}
                    {/* Custom Native Color Picker */}
                    <label
                      className={`builder-color-picker-label ${!CURATED_SURFACES.some((c) => c.hex.toLowerCase() === customColors.ground.toLowerCase()) ? 'is-selected' : ''}`}
                      title="Pick custom surface color"
                    >
                      <input
                        type="color"
                        className="builder-native-picker"
                        value={customColors.ground}
                        onChange={(e) => handleColorChange('ground', e.target.value)}
                        aria-label="Custom surface color picker"
                      />
                      <Pipette size={11} />
                      <span>Custom</span>
                    </label>
                  </div>
                </div>
              </div>

              <div className="palette-builder-footer">
                <button
                  type="button"
                  className="palette-builder-done-btn"
                  onClick={() => setOpen(false)}
                >
                  Done
                </button>
              </div>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
