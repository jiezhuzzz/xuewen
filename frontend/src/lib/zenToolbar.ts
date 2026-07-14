/// Zen-mode auto-hide policy for the floating PDF toolbar. Pure decision
/// logic — the component owns the actual timer; this owns when it may run
/// and what visibility results.
export const HIDE_DELAY_MS = 1500;
/// Pointer within this many px of the reader's top edge re-reveals the pill.
export const HOT_ZONE_PX = 96;

export interface ToolbarHold {
  zen: boolean;
  hotZone: boolean; //     pointer inside the top hot zone
  pointerOver: boolean; // pointer over the pill itself
  focusWithin: boolean; // keyboard focus inside the pill
  findOpen: boolean;
  pageEditing: boolean; // the page-number input is focused
}

/// While any hold is active the toolbar stays visible and the hide timer
/// must be cancelled. Outside zen the toolbar is unconditionally visible.
export function holdVisible(s: ToolbarHold): boolean {
  return !s.zen || s.hotZone || s.pointerOver || s.focusWithin || s.findOpen || s.pageEditing;
}

/// Final visibility: held, or the hide timer hasn't fired yet.
export function toolbarVisible(s: ToolbarHold, idleExpired: boolean): boolean {
  return holdVisible(s) || !idleExpired;
}
