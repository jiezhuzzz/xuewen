import { tick } from 'svelte';
import { chat, toggleChat } from './chat.svelte';
import { openFind } from './readerState.svelte';
import {
  closeTab,
  identifyState,
  library,
  openTab,
  selection,
  selectPaper,
  setInfoOpen,
  toggleInfo,
  toggleSidebar,
  toggleZen,
  ui,
  viewer,
} from './state.svelte';

export function isEditable(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return !!(
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLSelectElement ||
    t.isContentEditable
  );
}

function anyModalOpen(): boolean {
  return ui.importOpen || ui.projectsOpen || identifyState.open;
}

function moveSelection(delta: number): void {
  const papers = library.papers;
  if (papers.length === 0) return;
  const idx = papers.findIndex((p) => p.id === selection.id);
  const next = idx === -1 ? (delta > 0 ? 0 : papers.length - 1) : Math.min(papers.length - 1, Math.max(0, idx + delta));
  selectPaper(papers[next].id);
}

function openSelected(): void {
  const p = library.papers.find((x) => x.id === selection.id);
  if (p) openTab(p);
}

/// `/` must work even while the pane is collapsed or zen hides it (the
/// pane subtree is inert in both states): leave zen, open the pane, then
/// focus after the DOM update.
function focusSearch(): void {
  ui.zen = false;
  ui.sidebarOpen = true;
  void tick().then(() => {
    document.querySelector<HTMLInputElement>('[data-search-input]')?.focus();
  });
}

/// Global keymap. Modals own their Esc (Modal.svelte stops propagation);
/// everything except ⌘K is inert while a modal is open or focus is in a
/// text control. Spec deviation: close-tab is `x`, not ⌘W — browsers
/// reserve ⌘W/Ctrl+W for closing the browser tab.
export function handleKeydown(e: KeyboardEvent): void {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    ui.paletteOpen = !ui.paletteOpen;
    return;
  }
  if (anyModalOpen()) return;
  // ⌘F finds in the open PDF; on the Library view the browser find is fine.
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
    if (viewer.activeId) {
      e.preventDefault();
      openFind(viewer.activeId);
    }
    return;
  }
  if (e.key === 'Escape') {
    if (ui.paletteOpen) ui.paletteOpen = false;
    else if (chat.open) chat.open = false;
    else if (viewer.infoOpen) setInfoOpen(false);
    else if (ui.zen) ui.zen = false;
    return;
  }
  // Keyboard events from inside a shadow DOM (the PDF viewer) retarget
  // `e.target` to the shadow host, which is never editable — so keys typed
  // into the viewer's find box would leak to these app shortcuts. Check the
  // real deepest target from the composed path instead.
  const realTarget = e.composedPath()[0] ?? e.target;
  if (isEditable(realTarget) || ui.paletteOpen) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  // Match letters case-insensitively so Caps Lock or a held Shift doesn't
  // dead-key a shortcut (`Z`/`X` would otherwise miss `z`/`x`). Named keys
  // (Enter, ArrowUp, …) are longer than one char and keep their exact spelling.
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  switch (key) {
    case '/':
      e.preventDefault();
      focusSearch();
      break;
    case '[':
      toggleSidebar();
      break;
    case 'c':
      toggleChat();
      break;
    case 'i':
      if (viewer.activeId) toggleInfo();
      break;
    case 'z':
      toggleZen();
      break;
    case 'x':
      if (viewer.activeId) closeTab(viewer.activeId);
      break;
    case 'j':
      moveSelection(1);
      break;
    case 'k':
      moveSelection(-1);
      break;
    case 'Enter':
      // Enter on a focused control activates that control — it must not
      // also open the selected paper.
      if (e.target instanceof HTMLElement && e.target.closest('button, a, summary')) break;
      openSelected();
      break;
  }
}
