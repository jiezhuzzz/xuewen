/// Shared motion vocabulary. Every animated surface derives its timing from
/// these tokens so the whole UI moves with one accent — never hardcode a
/// duration in a component.
export const DUR = { fast: 150, base: 250, slow: 400 } as const;

/// The one standard ease-out (quart). Mirrors --ease-fluent in app.css.
export const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

/// Presets for `new Spring(value, SPRINGS.x)` from svelte/motion.
export const SPRINGS = {
  pane: { stiffness: 0.18, damping: 0.85 },
} as const;

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

/// Resolve a duration. 0 under reduced motion (accessibility) and under
/// vitest (jsdom runs transitions on rAF; non-zero durations leave outro
/// elements lingering and make DOM assertions flaky).
export function dur(ms: number): number {
  if (import.meta.env.MODE === 'test' || prefersReducedMotion()) return 0;
  return ms;
}
