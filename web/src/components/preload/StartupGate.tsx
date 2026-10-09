import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { Murmuration } from './Murmuration';
import { DISPERSAL_MS, READY_HOLD_MS, type PreloadPhase } from './flockMotion';
export type { PreloadPhase } from './flockMotion';
import './preload.css';

/** Only the first connection owns the entry screen. Later outages stay in the workspace. */
export function StartupGate({ children }: { children: ReactNode }) {
  const { workspace, isConnected, isConnecting, error, reconnect } = useLocalEngine();
  const [entered, setEntered] = useState(false);
  const [departing, setDeparting] = useState(false);
  const [takingLonger, setTakingLonger] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);
  const ready = isConnected && !!workspace;
  const unavailable = !ready && !isConnecting && !!error;

  useEffect(() => {
    if (entered) return;
    setDeparting(false);
    if (!ready) return;
    // The engine is already ready: settle into an orbit, then disperse over the map.
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const departure = window.setTimeout(() => setDeparting(true), reduced ? 0 : READY_HOLD_MS);
    const arrival = window.setTimeout(
      () => setEntered(true),
      reduced ? 0 : READY_HOLD_MS + DISPERSAL_MS,
    );
    return () => {
      window.clearTimeout(departure);
      window.clearTimeout(arrival);
    };
  }, [ready, entered]);

  useEffect(() => {
    if (entered || ready || unavailable) {
      setTakingLonger(false);
      return;
    }
    const timer = window.setTimeout(() => setTakingLonger(true), 8000);
    return () => window.clearTimeout(timer);
  }, [entered, ready, unavailable]);

  useEffect(() => {
    if (entered) contentRef.current?.querySelector('main')?.focus({ preventScroll: true });
  }, [entered]);

  return (
    <>
      {(ready || entered) && (
        <div ref={contentRef} inert={!entered} aria-hidden={!entered || undefined}>
          {children}
        </div>
      )}
      {!entered && (
        <PreloadScreen
          phase={
            ready
              ? departing
                ? 'departing'
                : 'ready'
              : unavailable
                ? 'unavailable'
                : takingLonger
                  ? 'waiting'
                  : 'connecting'
          }
          error={error}
          onRetry={reconnect}
        />
      )}
    </>
  );
}

/** Shared by the real entry and the isolated design preview; never invents engine state. */
export function PreloadScreen({
  phase,
  error,
  onRetry,
  flock,
}: {
  phase: PreloadPhase;
  error?: string | null;
  onRetry: () => void;
  flock?: ReactNode;
}) {
  const ready = phase === 'ready' || phase === 'departing';
  const unavailable = phase === 'unavailable';
  const takingLonger = phase === 'waiting';
  const heading = ready
    ? 'Ready when you are.'
    : unavailable
      ? 'Let’s find our way back.'
      : takingLonger
        ? 'Still gathering the threads.'
        : 'Gathering the threads.';
  const message = ready
    ? 'Your workspace is ready.'
    : unavailable
      ? 'We couldn’t reach your workspace just yet. Try again, or open a fresh connection link.'
      : takingLonger
        ? 'Your workspace is taking a little longer to answer. We’re still here.'
        : 'A moment to bring your world of work together.';

  return (
    <main
      className="preload-screen"
      data-ready={ready}
      data-phase={phase}
      style={{ '--preload-exit-duration': `${DISPERSAL_MS}ms` } as CSSProperties}
      data-unavailable={unavailable}
      aria-label="Opening your workspace"
    >
      <header className="preload-header">
        <span className="preload-brand">
          <span className="brand-symbol" aria-hidden="true" />
          tetonic
        </span>
        <span className="preload-header-note">A little space for what’s next.</span>
      </header>
      <div className="preload-center">
        {flock ?? <Murmuration phase={phase} />}
        <div className="preload-message" role="status" aria-live="polite" aria-atomic="true">
          <span className="preload-eyebrow">{ready ? 'Welcome in' : 'Coming together'}</span>
          <h1 key={heading}>{heading}</h1>
          <p>{message}</p>
        </div>
        {unavailable && (
          <div className="preload-recovery">
            <button className="preload-retry" onClick={onRetry}>
              Try again <span aria-hidden="true">↗</span>
            </button>
            <details>
              <summary>Connection details</summary>
              <p>{error}</p>
            </details>
          </div>
        )}
      </div>
      <footer className="preload-footer">
        <span className="preload-breath" aria-hidden="true" />
        <span>{unavailable ? 'Here when you’re ready.' : 'Your work, coming to life.'}</span>
      </footer>
    </main>
  );
}
