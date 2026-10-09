import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { PreloadScreen, type PreloadPhase } from '../../src/components/preload/StartupGate';
import { DISPERSAL_MS, READY_HOLD_MS } from '../../src/components/preload/flockMotion';
import { BoidMurmuration } from './BoidMurmuration';
import { DimensionalMurmuration } from './DimensionalMurmuration';
import '@fontsource-variable/inter';
import '@fontsource/plus-jakarta-sans/latin-700.css';
import '@fontsource/jetbrains-mono/latin-400.css';
import '../../src/index.css';
import '../../src/brand.css';
import './preview.css';

function Preview() {
  const [phase, setPhase] = useState<PreloadPhase>('connecting');
  const [playing, setPlaying] = useState(false);
  const [variant, setVariant] = useState('5d');
  const selectPhase = (next: PreloadPhase) => {
    setPlaying(false);
    setPhase(next);
  };
  useEffect(() => {
    if (!playing) return;
    const sequence: [PreloadPhase, number][] = [
      ['connecting', 2200],
      ['waiting', 2200],
      ['unavailable', 2200],
      ['connecting', 2000],
      ['ready', READY_HOLD_MS],
      ['departing', DISPERSAL_MS],
    ];
    let delay = 0;
    const timers = sequence.map(([next, duration]) => {
      const timer = window.setTimeout(() => setPhase(next), delay);
      delay += duration;
      return timer;
    });
    timers.push(window.setTimeout(() => setPlaying(false), delay));
    return () => timers.forEach(window.clearTimeout);
  }, [playing]);
  return (
    <div className="preload-preview">
      <div
        className="preload-preview-map"
        aria-label="Map background preview"
        aria-hidden={phase !== 'departing'}
      >
        <header className="preload-header">
          <span className="preload-brand">
            <span className="brand-symbol" aria-hidden="true" />
            tetonic
          </span>
        </header>
        <p>Map background preview</p>
      </div>
      <PreloadScreen
        phase={phase}
        flock={
          variant === '5d' ? (
            <DimensionalMurmuration phase={phase} />
          ) : variant === 'boids' ? (
            <BoidMurmuration phase={phase} />
          ) : undefined
        }
        error="Open the connection link printed by the local engine."
        onRetry={() => selectPhase('connecting')}
      />
      <div className="preload-preview-controls">
        <span>Preview</span>
        <select
          aria-label="Flock style"
          value={variant}
          onChange={(event) => setVariant(event.target.value)}
        >
          <option value="5d">5D stipple + boids</option>
          <option value="boids">Stipple + boids</option>
          <option value="original">Original strokes</option>
        </select>
        <select
          aria-label="Preview stage"
          value={phase}
          onChange={(event) => selectPhase(event.target.value as PreloadPhase)}
        >
          <option value="connecting">Arriving</option>
          <option value="waiting">Taking a moment</option>
          <option value="unavailable">Connection needs attention</option>
          <option value="ready">Ready</option>
          <option value="departing">Reveal map</option>
        </select>
        <button type="button" onClick={() => setPlaying((value) => !value)}>
          {playing ? 'Stop sequence' : 'Play sequence'}
        </button>
      </div>
    </div>
  );
}

const root = createRoot(document.getElementById('root')!);
root.render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);

if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
