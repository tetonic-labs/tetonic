import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';
window.scrollTo = vi.fn();
// Node exposes an unavailable experimental localStorage through Vitest's window proxy.
// Bind deterministic browser storage for this isolated test environment.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, String(value)),
    removeItem: (key: string) => stored.delete(key),
    clear: () => stored.clear(),
    key: (index: number) => [...stored.keys()][index] ?? null,
    get length() {
      return stored.size;
    },
  },
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.classList.remove('dark');
});
