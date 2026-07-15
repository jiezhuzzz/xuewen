import { reader } from './readerState.svelte';
import { ui, viewer } from './state.svelte';
import { HIDE_DELAY_MS, HOT_ZONE_PX, holdVisible, toolbarVisible, type ToolbarHold } from './zenToolbar';

/// Shared zen auto-hide for the reader's floating pills (center toolbar +
/// top-right quick actions). One instance per PdfPages tab: it owns the DOM
/// signals and the hide timer; the visibility decision stays in the pure
/// lib/zenToolbar.ts. Both pills bind the same handlers, so hovering or
/// focusing either one holds both visible and they fade together.
export interface PillHide {
  readonly visible: boolean;
  setHost(el: HTMLElement | null): void;
  setExtraHold(fn: () => boolean): void;
  onWindowMove(e: PointerEvent): void;
  pillEnter(): void;
  pillLeave(): void;
  focusIn(): void;
  focusOut(): void;
}

/// MUST be called during component init — it registers $effects.
export function createPillHide(getDocumentId: () => string): PillHide {
  let hotZone = $state(false);
  let pointerOver = $state(false);
  let focusWithin = $state(false);
  let idleExpired = $state(false);
  // The toolbar registers its local interaction holds (page editing, zoom
  // menu) here; reads inside the $derived track their reactive sources.
  let extraHold = $state<() => boolean>(() => false);
  let host: HTMLElement | null = null;

  const hold = $derived<ToolbarHold>({
    zen: ui.zen,
    hotZone,
    pointerOver,
    focusWithin,
    findOpen: !!reader.find[getDocumentId()],
    // Historical field name; carries every toolbar-local interaction hold.
    pageEditing: extraHold(),
  });
  const visible = $derived(toolbarVisible(hold, idleExpired));

  // A stale hot-zone from a previous zen session must not hold the pills
  // visible on re-entry — onWindowMove only updates hotZone while zen is
  // active, so without this it freezes at its last value across exit/re-entry.
  $effect(() => {
    if (!ui.zen) hotZone = false;
  });

  // Any hold cancels the countdown and re-arms visibility; once every hold
  // drops in zen, the countdown starts.
  $effect(() => {
    if (holdVisible(hold)) {
      idleExpired = false;
      return;
    }
    const t = setTimeout(() => (idleExpired = true), HIDE_DELAY_MS);
    return () => clearTimeout(t);
  });

  return {
    get visible() {
      return visible;
    },
    setHost(el) {
      host = el;
    },
    setExtraHold(fn) {
      extraHold = fn;
    },
    // Window-level so it works while the pills are faded out; only the
    // active tab's controller reacts (hidden tabs stay mounted).
    onWindowMove(e) {
      if (!ui.zen || viewer.activeId !== getDocumentId()) return;
      if (!host) return;
      const top = host.getBoundingClientRect().top;
      hotZone = e.clientY >= top && e.clientY - top < HOT_ZONE_PX;
    },
    pillEnter() {
      pointerOver = true;
    },
    pillLeave() {
      pointerOver = false;
    },
    focusIn() {
      focusWithin = true;
    },
    focusOut() {
      focusWithin = false;
    },
  };
}
