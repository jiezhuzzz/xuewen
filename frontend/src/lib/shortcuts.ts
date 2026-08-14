import { tick } from 'svelte';
import { chat } from './chat.svelte';
import { copyPdfSelection, pdfSelectionHasText } from './pdfCopy';
import { openFind, toggleAnnotationsPanel } from './readerState.svelte';
import {
  closeDock,
  closeTab,
  dock,
  identifyState,
  library,
  openTab,
  selection,
  selectPaper,
  toggleDock,
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
  return ui.importOpen || identifyState.open || ui.helpOpen;
}

/// True when the browser has a real text selection of its own. Every surface
/// outside the PDF page area — library rows, the details dock, the Ask
/// transcript, popovers, the reader's own toolbar — is ordinary DOM text where
/// native copy already works, so ⌘C must stand aside there. The page area is
/// the sole exception, and it can never produce a selection to confuse this:
/// its pages are <img>, its overlays are empty <div>s, the Viewport carries
/// `select-none`, and pdfCopy clears any stale selection when a PDF selection
/// begins.
function hasDomSelection(): boolean {
  return (document.getSelection()?.toString() ?? '').trim() !== '';
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
  // Keyboard events from inside a shadow DOM (the PDF viewer) retarget
  // `e.target` to the shadow host, which is never editable — so keys typed
  // into the viewer's find box would leak to these app shortcuts. Check the
  // real deepest target from the composed path instead.
  const realTarget = e.composedPath()[0] ?? e.target;
  // ⌘C copies the reader's text selection. The reader is the one place in the
  // app where the browser cannot do this itself — its pages are <img> and its
  // selection overlay is empty <div>s, so the document selection there is
  // always collapsed and no `copy` event is ever dispatched (see pdfCopy.ts).
  // Everywhere else IS real DOM text, so this branch stands aside — no
  // preventDefault, no call — on a live DOM selection, in a text control, on
  // the Library view, or with nothing selected in the PDF, and native copy
  // proceeds untouched. ⌥/⇧ are excluded so ⌘⌥C and ⌘⇧C (DevTools inspect)
  // still reach the browser. copyPdfSelection() must not be awaited or
  // deferred: it writes the clipboard synchronously, while this keystroke's
  // user gesture still authorizes the write.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'c') {
    if (viewer.activeId && !isEditable(realTarget) && !hasDomSelection() && pdfSelectionHasText()) {
      e.preventDefault();
      copyPdfSelection();
    }
    return;
  }
  if (e.key === 'Escape') {
    if (ui.paletteOpen) ui.paletteOpen = false;
    else if (dock.open && viewer.activeId !== null) closeDock();
    else if (ui.zen) ui.zen = false;
    return;
  }
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
    case '?':
      e.preventDefault();
      ui.helpOpen = true;
      break;
    case '[':
      toggleSidebar();
      break;
    case 'c':
      if (chat.available) toggleDock('ask');
      break;
    case 'i':
      toggleDock('details');
      break;
    case 'a':
      // Reader-only: the panel lives inside the PDF view, and on the library
      // this would open a panel nobody can see.
      if (viewer.activeId) toggleAnnotationsPanel();
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
