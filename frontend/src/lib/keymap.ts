import { tick } from 'svelte';
import { chat } from './chat.svelte';
import { library } from './library.svelte';
import { toggleAnnotationsPanel } from './readerState.svelte';
import { closeTab, openTab, selectPaper, selection, toggleZen, viewer } from './tabs.svelte';
import { toggleDock, toggleSidebar, ui } from './ui.svelte';

function moveSelection(delta: number): void {
  const papers = library.papers;
  if (papers.length === 0) return;
  const idx = papers.findIndex((p) => p.id === selection.id);
  const next = idx === -1 ? (delta > 0 ? 0 : papers.length - 1) : Math.min(papers.length - 1, Math.max(0, idx + delta));
  selectPaper(papers[next].id);
}

/// Enter's action — the binding itself stays bespoke in shortcuts.ts because
/// its focused-control guard needs the event's composed-path target.
export function openSelected(): void {
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

export interface KeyBinding {
  key: string;
  label: string;
  /// Gate: the key is inert (dispatch skipped entirely) when this is false.
  when?: () => boolean;
  /// Only keys the browser has its own use for (quick-find) suppress it.
  preventDefault?: boolean;
  run: () => void;
}

/// The single-key map as data — the one source shared by the dispatcher
/// (shortcuts.ts), the `?` help overlay (SHORTCUT_GROUPS below), and the
/// command palette's key hints, so a binding can't drift between behavior
/// and display. Chords (⌘K/⌘F/⌘C), Esc, and Enter stay bespoke in
/// shortcuts.ts — each needs the event itself — and appear below as static
/// display rows only.
export const SINGLE_KEYS: readonly KeyBinding[] = [
  { key: '/', label: 'Search library', preventDefault: true, run: focusSearch },
  { key: '?', label: 'Keyboard shortcuts', preventDefault: true, run: () => (ui.helpOpen = true) },
  { key: '[', label: 'Toggle list pane', run: toggleSidebar },
  { key: 'c', label: 'Ask panel', when: () => chat.available, run: () => toggleDock('ask') },
  { key: 'i', label: 'Details panel', run: () => toggleDock('details') },
  {
    key: 'a',
    label: 'Annotations panel',
    // Reader-only: the panel lives inside the PDF view, and on the library
    // this would open a panel nobody can see.
    when: () => viewer.activeId !== null,
    run: toggleAnnotationsPanel,
  },
  { key: 'z', label: 'Zen mode', run: toggleZen },
  { key: 'x', label: 'Close tab', when: () => viewer.activeId !== null, run: () => closeTab(viewer.activeId!) },
  { key: 'j', label: 'Next paper', run: () => moveSelection(1) },
  { key: 'k', label: 'Previous paper', run: () => moveSelection(-1) },
];

export interface ShortcutItem {
  keys: string;
  label: string;
}

/// A single-key binding's display row, pulled from the table above.
function fromKey(key: string): ShortcutItem {
  const b = SINGLE_KEYS.find((x) => x.key === key)!;
  return { keys: b.key, label: b.label };
}

/// Rendered by the `?` help overlay. Single-key rows derive from SINGLE_KEYS
/// (j/k fold into one row); chord/Esc/Enter rows are the static display of
/// the bespoke handlers in shortcuts.ts.
export const SHORTCUT_GROUPS: ReadonlyArray<{ title: string; items: ShortcutItem[] }> = [
  {
    title: 'Library',
    items: [
      fromKey('/'),
      // j/k are two bindings but one display row — the explicit pairing rule.
      { keys: `${fromKey('j').keys} / ${fromKey('k').keys}`, label: 'Next / previous paper' },
      { keys: 'Enter', label: 'Open selected paper' },
      fromKey('['),
    ],
  },
  {
    title: 'Reader',
    items: [
      fromKey('i'),
      fromKey('c'),
      fromKey('a'),
      fromKey('z'),
      fromKey('x'),
      { keys: '⌘F', label: 'Find in PDF' },
      { keys: '⌘C', label: 'Copy selected text' },
    ],
  },
  {
    title: 'Anywhere',
    items: [
      { keys: '⌘K', label: 'Command palette' },
      fromKey('?'),
      { keys: 'Esc', label: 'Close panel · exit zen' },
    ],
  },
];
