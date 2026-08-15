import { getSettings } from './api';
import { readLocalJson, writeLocal } from './persist';
import { viewer } from './tabs.svelte';
import type { TranslateSettings } from './types';

export const ui = $state<{
  sidebarOpen: boolean;
  importOpen: boolean;
  zen: boolean;
  paletteOpen: boolean;
  helpOpen: boolean;
}>({
  sidebarOpen: true,
  importOpen: false,
  zen: false,
  paletteOpen: false,
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

export type DockTab = 'details' | 'ask';

/// The reader dock: one right-docked panel hosting the Details and Ask tabs
/// (replaces the old separate info panel + chat float). Open state and tab
/// are remembered across sessions.
export const dock = $state<{ open: boolean; tab: DockTab }>({ open: false, tab: 'details' });

const DOCK_KEY = 'xuewen-dock';

/// Load the remembered dock state (default: closed, Details). Call once at startup.
export function initDock(): void {
  const v = readLocalJson(DOCK_KEY);
  if (!v || typeof v !== 'object') return; // absent or corrupted — keep defaults
  const { open, tab } = v as { open?: unknown; tab?: unknown };
  dock.open = open === true;
  dock.tab = tab === 'ask' ? 'ask' : 'details';
}

function saveDock(): void {
  writeLocal(DOCK_KEY, JSON.stringify({ open: dock.open, tab: dock.tab }));
}

export function openDock(tab: DockTab): void {
  dock.open = true;
  dock.tab = tab;
  saveDock();
}

export function closeDock(): void {
  dock.open = false;
  saveDock();
}

/// The `i`/`c` shortcut behavior: close if already open on that tab,
/// otherwise open on (or switch to) it. The dock only exists over a PDF.
export function toggleDock(tab: DockTab): void {
  if (viewer.activeId === null) return;
  if (dock.open && dock.tab === tab) closeDock();
  else openDock(tab);
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
