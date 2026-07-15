import { tick } from 'svelte';

/// Reader UI state. The find bar is per-open-paper (keyed by documentId) so the
/// global keymap reaches the active tab's bar while hidden tabs keep their own.
/// The side panel, by contrast, is a single GLOBAL setting shared across every
/// open paper: opening/closing it or switching thumbnails↔outline in one paper
/// applies to all open tabs and to every paper opened afterwards.
export type PanelTab = 'thumbs' | 'outline';

export const reader = $state<{
  find: Record<string, boolean>;
  panel: PanelTab | null;
  lastPanel: PanelTab;
}>({ find: {}, panel: null, lastPanel: 'thumbs' });

/// Open/close one document's find bar. Omit `open` to toggle.
export function setFind(id: string, open?: boolean): void {
  reader.find[id] = open ?? !reader.find[id];
}

/// The toolbar's single sidebar button (global): closed → reopen at the
/// last-used view (thumbnails on first open); open → close.
export function toggleSidebar(): void {
  reader.panel = reader.panel ? null : reader.lastPanel;
}

/// The panel's segmented control: switch the (global) open view and remember it.
export function setPanelView(tab: PanelTab): void {
  reader.panel = tab;
  reader.lastPanel = tab;
}

/// ⌘F: open (or refocus) a document's find bar. Focus waits a tick so a
/// just-mounted bar exists; `select()` keeps a previous query editable.
export function openFind(id: string): void {
  reader.find[id] = true;
  void tick().then(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-find-input="${id}"]`);
    if (input) {
      input.focus();
      input.select();
    }
  });
}

/// Forget a closed tab's find state (called from closeTab). The side panel is
/// global, so there is nothing panel-related to drop per tab.
export function dropReaderState(id: string): void {
  delete reader.find[id];
}
