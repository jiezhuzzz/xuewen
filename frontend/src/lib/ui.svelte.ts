import { getSettings } from './api';
import { readLocalJson, writeLocal } from './persist';
import { viewer } from './tabs.svelte';
import type { TranslateSettings } from './types';

export const ui = $state<{
  sidebarOpen: boolean;
  importOpen: boolean;
  zen: boolean;
  filePickerOpen: boolean;
  helpOpen: boolean;
}>({
  sidebarOpen: true,
  importOpen: false,
  zen: false,
  filePickerOpen: false,
  helpOpen: false,
});
export function toggleSidebar(): void {
  ui.sidebarOpen = !ui.sidebarOpen;
}

/// Below ~lg the fixed 304px list pane crushes the reader, so it starts
/// collapsed there and follows live crossings of the breakpoint. `[`, the
/// edge-peek button, and the TopBar toggle still override at any width —
/// this only sets the default on load/resize, it doesn't lock anything.
export function initResponsiveSidebar(): void {
  const q = window.matchMedia('(max-width: 1023px)');
  if (q.matches) ui.sidebarOpen = false;
  q.addEventListener('change', (e) => {
    ui.sidebarOpen = !e.matches;
  });
}

/// Where the panel should land when it opens. Not a mode: the dock is one
/// scroll (the record, then the conversation) with the composer pinned to its
/// foot, so this only picks what gets focus or scrolled on the way in.
export type DockEntry = 'record' | 'ask';

/// The reader dock: one right-docked panel carrying the paper's record and
/// its Ask thread on a single surface. `entry` is a one-shot request that
/// ReaderDock consumes and clears; only `open` is remembered across sessions.
export const dock = $state<{ open: boolean; entry: DockEntry | null }>({ open: false, entry: null });

const DOCK_KEY = 'xuewen-dock';

/// Load the remembered dock state (default: closed). Call once at startup.
export function initDock(): void {
  const v = readLocalJson(DOCK_KEY);
  if (!v || typeof v !== 'object') return; // absent or corrupted — keep defaults
  const { open } = v as { open?: unknown };
  dock.open = open === true;
}

function saveDock(): void {
  writeLocal(DOCK_KEY, JSON.stringify({ open: dock.open }));
}

export function openDock(entry: DockEntry = 'record'): void {
  dock.open = true;
  dock.entry = entry;
  saveDock();
}

export function closeDock(): void {
  dock.open = false;
  dock.entry = null;
  saveDock();
}

/// The `i`/`c` shortcut behavior. `i` is the panel's own toggle; `c` asks for
/// the composer, so with the panel already open it moves focus there rather
/// than closing the thing it was asked to type into. The dock only exists
/// over a PDF.
export function toggleDock(entry: DockEntry = 'record'): void {
  if (viewer.activeId === null) return;
  if (!dock.open) openDock(entry);
  else if (entry === 'ask') dock.entry = 'ask';
  else closeDock();
}

/// UI preferences and server-held state from `/api/settings` — the single
/// loader/store for that endpoint (the import modal re-calls loadSettings
/// after a proxy-cookie change rather than keeping its own copy). Loaded
/// once at startup.
export const appSettings = $state<{
  foldAbstract: boolean;
  translate: TranslateSettings;
  proxyHost: string | null;
  proxyCookieSet: boolean;
  proxyCookieUpdatedAt: string | null;
}>({
  foldAbstract: true,
  translate: { enabled: false },
  proxyHost: null,
  proxyCookieSet: false,
  proxyCookieUpdatedAt: null,
});

export async function loadSettings(): Promise<void> {
  try {
    const s = await getSettings();
    appSettings.foldAbstract = s.fold_abstract;
    appSettings.translate = s.translate ?? { enabled: false };
    appSettings.proxyHost = s.proxy?.host ?? null;
    appSettings.proxyCookieSet = s.proxy_cookie_set;
    appSettings.proxyCookieUpdatedAt = s.proxy_cookie_updated_at;
  } catch (e) {
    console.error(e); // keep the last known values on failure
  }
}
