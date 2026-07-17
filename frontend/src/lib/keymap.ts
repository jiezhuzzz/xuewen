/// The app's keymap as data — rendered by the `?` help overlay and the
/// command palette's key hints. The handlers live in `shortcuts.ts`; when a
/// key is added or changed there, update this list in the same commit.
export interface ShortcutItem {
  keys: string;
  label: string;
}

export const SHORTCUT_GROUPS: ReadonlyArray<{ title: string; items: ShortcutItem[] }> = [
  {
    title: 'Library',
    items: [
      { keys: '/', label: 'Search library' },
      { keys: 'j / k', label: 'Next / previous paper' },
      { keys: 'Enter', label: 'Open selected paper' },
      { keys: '[', label: 'Toggle list pane' },
    ],
  },
  {
    title: 'Reader',
    items: [
      { keys: 'i', label: 'Details panel' },
      { keys: 'c', label: 'Ask panel' },
      { keys: 'z', label: 'Zen mode' },
      { keys: 'x', label: 'Close tab' },
      { keys: '⌘F', label: 'Find in PDF' },
    ],
  },
  {
    title: 'Anywhere',
    items: [
      { keys: '⌘K', label: 'Command palette' },
      { keys: '?', label: 'Keyboard shortcuts' },
      { keys: 'Esc', label: 'Close panel · exit zen' },
    ],
  },
];
