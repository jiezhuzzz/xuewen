import { afterEach, describe, expect, it, vi } from 'vitest';
import { DUR, EASE, SPRINGS, dur, prefersReducedMotion } from './motion';

afterEach(() => vi.unstubAllGlobals());

function stubReducedMotion(matches: boolean): void {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('prefers-reduced-motion') ? matches : false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe('motion tokens', () => {
  it('exposes the shared vocabulary', () => {
    expect(DUR).toEqual({ fast: 150, base: 250, slow: 400 });
    expect(EASE).toBe('cubic-bezier(0.22, 1, 0.36, 1)');
    expect(SPRINGS.pane.stiffness).toBeGreaterThan(0);
  });

  it('prefersReducedMotion reads the media query', () => {
    stubReducedMotion(true);
    expect(prefersReducedMotion()).toBe(true);
    stubReducedMotion(false);
    expect(prefersReducedMotion()).toBe(false);
  });

  it('prefersReducedMotion is false when matchMedia is unavailable (jsdom default)', () => {
    expect(prefersReducedMotion()).toBe(false);
  });

  it('dur is 0 under vitest so transitions never linger in DOM tests', () => {
    expect(dur(250)).toBe(0);
  });
});
