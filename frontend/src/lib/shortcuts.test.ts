import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { handleKeydown, isEditable } from './shortcuts';
import { chat } from './chat.svelte';
import { dock, identifyState, library, selection, ui, viewer } from './state.svelte';
import { reader } from './readerState.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', starred: false, tags: [], projects: [],
  };
}

function key(k: string, opts: Partial<KeyboardEvent> & { target?: EventTarget } = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: k, ...opts });
  if (opts.target) Object.defineProperty(e, 'target', { value: opts.target });
  return e;
}

beforeEach(() => {
  library.papers = [paper('a'), paper('b'), paper('c')];
  viewer.tabs = [];
  viewer.activeId = null;
  dock.open = false;
  dock.tab = 'details';
  selection.id = null;
  ui.zen = false;
  ui.paletteOpen = false;
  ui.sidebarOpen = true;
  ui.importOpen = false;
  ui.helpOpen = false;
  identifyState.open = false;
  chat.available = false;
  localStorage.clear();
  for (const k of Object.keys(reader.find)) delete reader.find[k];
});

describe('isEditable', () => {
  it('flags inputs and textareas', () => {
    expect(isEditable(document.createElement('input'))).toBe(true);
    expect(isEditable(document.createElement('textarea'))).toBe(true);
    expect(isEditable(document.createElement('div'))).toBe(false);
    expect(isEditable(null)).toBe(false);
  });
});

describe('handleKeydown', () => {
  it('cmd+k toggles the palette even from an input', () => {
    handleKeydown(key('k', { metaKey: true, target: document.createElement('input') }));
    expect(ui.paletteOpen).toBe(true);
  });

  it('[ toggles the pane; ignored while typing', () => {
    handleKeydown(key('['));
    expect(ui.sidebarOpen).toBe(false);
    handleKeydown(key('[', { target: document.createElement('input') }));
    expect(ui.sidebarOpen).toBe(false); // unchanged
  });

  it('j/k move the selection through the list, Enter opens it', () => {
    handleKeydown(key('j'));
    expect(selection.id).toBe('a');
    handleKeydown(key('j'));
    expect(selection.id).toBe('b');
    handleKeydown(key('k'));
    expect(selection.id).toBe('a');
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
  });

  it('z toggles zen only with an active tab; x closes the active tab', () => {
    handleKeydown(key('z'));
    expect(ui.zen).toBe(false);
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    expect(ui.zen).toBe(true);
    handleKeydown(key('x'));
    expect(viewer.tabs).toHaveLength(0);
    expect(ui.zen).toBe(false);
  });

  it('matches letters case-insensitively (Caps Lock / Shift → uppercase)', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
    handleKeydown(key('Z')); // uppercase, e.g. Caps Lock on
    expect(ui.zen).toBe(true);
    handleKeydown(key('X')); // uppercase close-tab
    expect(viewer.tabs).toHaveLength(0);
    expect(ui.zen).toBe(false);
  });

  it('Escape closes the palette first, then exits zen', () => {
    ui.paletteOpen = true;
    ui.zen = true;
    handleKeydown(key('Escape'));
    expect(ui.paletteOpen).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });

  it('c toggles the dock on Ask only with an active tab and available chat', () => {
    handleKeydown(key('c'));
    expect(dock.open).toBe(false);
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('c'));
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('ask');
    handleKeydown(key('c'));
    expect(dock.open).toBe(false);
  });

  it('Escape closes the dock before exiting zen', () => {
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    handleKeydown(key('c'));
    expect(ui.zen).toBe(true);
    expect(dock.open).toBe(true);
    handleKeydown(key('Escape'));
    expect(dock.open).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });

  it('i toggles the dock on Details only with an active tab', () => {
    handleKeydown(key('i'));
    expect(dock.open).toBe(false); // no active paper
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('i'));
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('details');
    handleKeydown(key('i'));
    expect(dock.open).toBe(false);
  });

  it('Escape closes a dock opened directly (not via a shortcut) before exiting zen', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    dock.open = true;
    dock.tab = 'details';
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(dock.open).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });

  it('/ opens the pane, exits zen, and focuses the search input', async () => {
    const input = document.createElement('input');
    input.setAttribute('data-search-input', '');
    document.body.appendChild(input);
    ui.sidebarOpen = false;
    ui.zen = true;
    handleKeydown(key('/'));
    expect(ui.sidebarOpen).toBe(true);
    expect(ui.zen).toBe(false);
    await tick();
    expect(document.activeElement).toBe(input);
    input.remove();
  });

  it('Enter on a focused button activates the button only, not the selection', () => {
    handleKeydown(key('j')); // selection = first paper
    const btn = document.createElement('button');
    handleKeydown(key('Enter', { target: btn }));
    expect(viewer.tabs).toHaveLength(0);
  });

  // A keydown that bubbled out of a shadow DOM (e.g. the PDF viewer): the
  // browser retargets `target` to the non-editable host, but composedPath()[0]
  // is the real element inside the shadow tree.
  function shadowKey(k: string, realTarget: EventTarget, host: EventTarget): KeyboardEvent {
    const e = new KeyboardEvent('keydown', { key: k });
    Object.defineProperty(e, 'target', { value: host });
    Object.defineProperty(e, 'composedPath', { value: () => [realTarget, host, window] });
    return e;
  }

  it('ignores shortcuts when a shadow-DOM input (viewer find box) is the real target', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
    const host = document.createElement('embedpdf-container'); // non-editable shadow host
    const input = document.createElement('input'); // real target inside the shadow tree
    handleKeydown(shadowKey('x', input, host));
    expect(viewer.tabs).toHaveLength(1); // NOT closed — the key belongs to the find box
  });

  it('still fires shortcuts when the real shadow target is non-editable (viewport)', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    const host = document.createElement('embedpdf-container');
    const viewport = document.createElement('div'); // non-editable
    handleKeydown(shadowKey('x', viewport, host));
    expect(viewer.tabs).toHaveLength(0); // closed — shortcuts still work while reading
  });

  it('single-key shortcuts are inert while a modal is open', () => {
    ui.importOpen = true;
    handleKeydown(key('['));
    expect(ui.sidebarOpen).toBe(true);
    handleKeydown(key('Escape')); // the modal owns Esc
    expect(ui.importOpen).toBe(true); // handler must not touch it
  });

  it('? opens the shortcut help overlay', () => {
    handleKeydown(key('?', { shiftKey: true }));
    expect(ui.helpOpen).toBe(true);
  });

  it('? is inert while focus is in a text control', () => {
    handleKeydown(key('?', { target: document.createElement('input') }));
    expect(ui.helpOpen).toBe(false);
  });

  it('single-key shortcuts are inert while the help overlay is open', () => {
    ui.helpOpen = true;
    handleKeydown(key('z'));
    expect(ui.zen).toBe(false);
  });

  it('cmd+f opens the find bar for the active paper', () => {
    viewer.tabs = [{ id: 'a', title: 'A' }];
    viewer.activeId = 'a';
    const e = key('f', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(reader.find['a']).toBe(true);
    expect(e.defaultPrevented).toBe(true); // the browser find must not fire
  });

  it('cmd+f on the Library view leaves the browser find alone', () => {
    viewer.activeId = null;
    const e = key('f', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it('cmd+f is inert while a modal is open', () => {
    viewer.tabs = [{ id: 'a', title: 'A' }];
    viewer.activeId = 'a';
    ui.importOpen = true;
    handleKeydown(key('f', { metaKey: true, cancelable: true }));
    expect(reader.find['a']).toBeUndefined();
  });
});
