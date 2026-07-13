import type { Reference } from './citations';
import type { PaperSummary } from './types';

export interface HoveredCitation {
  reference: Reference;
  matchedPaper: PaperSummary | null;
  screenX: number;
  screenY: number;
}

// Single global hovered-citation slot (only one popover at a time).
export const citationHover = $state<{ current: HoveredCitation | null }>({ current: null });

let hideTimer: ReturnType<typeof setTimeout> | null = null;

export function showCitation(c: HoveredCitation): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
  citationHover.current = c;
}

/** Hide after a short grace delay so the pointer can travel into the popover. */
export function hideCitationSoon(): void {
  if (hideTimer) clearTimeout(hideTimer);
  hideTimer = setTimeout(() => {
    citationHover.current = null;
    hideTimer = null;
  }, 120);
}

export function cancelHideCitation(): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
}
