import { identifyState } from './identify.svelte';
import { openSelected, SINGLE_KEYS } from './keymap';
import { copyPdfSelection, pdfSelectionHasText } from './pdfCopy';
import { openFind } from './readerState.svelte';
import { viewer } from './tabs.svelte';
import { closeDock, dock, ui } from './ui.svelte';

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

/// Global keymap driver. The single-key bindings live as data in keymap.ts
/// (SINGLE_KEYS); this dispatches them after the gating below. Modals own
/// their Esc (Modal.svelte stops propagation); everything except ⌘K is inert
/// while a modal is open or focus is in a text control. Spec deviation:
/// close-tab is `x`, not ⌘W — browsers reserve ⌘W/Ctrl+W for closing the
/// browser tab.
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
  const binding = SINGLE_KEYS.find((b) => b.key === key);
  if (binding) {
    if (binding.when && !binding.when()) return;
    if (binding.preventDefault) e.preventDefault();
    binding.run();
    return;
  }
  if (key === 'Enter') {
    // Enter on a focused control activates that control — it must not also
    // open the selected paper. Checked against the composed-path target, so
    // a button inside the viewer's shadow DOM counts too (raw e.target would
    // be its never-matching host).
    if (realTarget instanceof HTMLElement && realTarget.closest('button, a, summary')) return;
    openSelected();
  }
}
