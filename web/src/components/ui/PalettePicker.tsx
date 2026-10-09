import { useEffect, useState, useMemo } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { Palette, Check, ChevronLeft, Sliders, Pipette, RotateCcw } from 'lucide-react';
import './palette-picker.css';
import {
  type PaletteId,
  type CustomColors,
  DEFAULT_CUSTOM_COLORS,
  getLuminance,
  readAppearance,
  savePalette,
} from '../../lib/appearance';
export { type PaletteId, type CustomColors, DEFAULT_CUSTOM_COLORS } from '../../lib/appearance';

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

export function PalettePicker() {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<'list' | 'builder'>('list');

  const [customColors, setCustomColors] = useState<CustomColors>(() => readAppearance().colors);
  const [activePalette, setActivePalette] = useState<PaletteId>(() => readAppearance().palette);
  useEffect(() => {
    savePalette(activePalette, customColors);
  }, [activePalette, customColors]);

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
                <span className="palette-popover-subtitle">Make this space yours</span>
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
