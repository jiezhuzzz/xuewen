import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { handleKeydown, isEditable } from './shortcuts';
import { chat } from './chat.svelte';
import { identifyState, library, selection, ui, viewer } from './state.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
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
  viewer.infoOpen = false;
  selection.id = null;
  ui.zen = false;
  ui.paletteOpen = false;
  ui.sidebarOpen = true;
  ui.importOpen = false;
  ui.projectsOpen = false;
  identifyState.open = false;
  chat.open = false;
  chat.available = false;
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

  it('c toggles the chat only with an active tab and available chat', () => {
    handleKeydown(key('c'));
    expect(chat.open).toBe(false);
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('c'));
    expect(chat.open).toBe(true);
    handleKeydown(key('c'));
    expect(chat.open).toBe(false);
  });

  it('Escape closes the chat before exiting zen', () => {
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    handleKeydown(key('c'));
    expect(ui.zen).toBe(true);
    expect(chat.open).toBe(true);
    handleKeydown(key('Escape'));
    expect(chat.open).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });

  it('i toggles the info panel only with an active tab', () => {
    handleKeydown(key('i'));
    expect(viewer.infoOpen).toBe(false); // no active paper
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('i'));
    expect(viewer.infoOpen).toBe(true);
    handleKeydown(key('i'));
    expect(viewer.infoOpen).toBe(false);
  });

  it('Escape closes the info panel before exiting zen', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    viewer.infoOpen = true;
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(viewer.infoOpen).toBe(false);
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

  it('single-key shortcuts are inert while a modal is open', () => {
    ui.importOpen = true;
    handleKeydown(key('['));
    expect(ui.sidebarOpen).toBe(true);
    handleKeydown(key('Escape')); // the modal owns Esc
    expect(ui.importOpen).toBe(true); // handler must not touch it
  });
});
