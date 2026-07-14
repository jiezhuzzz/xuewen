import { tick } from 'svelte';

/// Per-open-paper reader UI state, keyed by documentId (= paper id). Lives
/// at module level (not inside PdfPages) so the global keymap can reach the
/// active tab's find bar, while each hidden tab still keeps its own state.
export type PanelTab = 'thumbs' | 'outline';

export const reader = $state<{
  find: Record<string, boolean>;
  panel: Record<string, PanelTab | null>;
}>({ find: {}, panel: {} });

/// Open/close one document's find bar. Omit `open` to toggle.
export function setFind(id: string, open?: boolean): void {
  reader.find[id] = open ?? !reader.find[id];
}

/// Select a side-panel tab; selecting the open tab again closes the panel.
export function togglePanel(id: string, tab: PanelTab): void {
  reader.panel[id] = reader.panel[id] === tab ? null : tab;
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

/// Forget a closed tab's state (called from closeTab).
export function dropReaderState(id: string): void {
  delete reader.find[id];
  delete reader.panel[id];
}
