import { beforeEach, describe, expect, it, vi } from 'vitest';
import { annotationTools, setActiveTool } from './annotationState.svelte';
import { library, removePaper } from './library.svelte';
import {
  closeTab,
  goHome,
  openTab,
  selection,
  selectPaper,
  toggleZen,
  viewer,
} from './tabs.svelte';
import { ui } from './ui.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

beforeEach(() => {
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  ui.zen = false;
  annotationTools.active = null;
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(JSON.stringify({ total: 0, resolved: 0, needs_review: 0 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    ),
  );
});

describe('selection and home tab', () => {
  it('selectPaper sets and clears the browsing selection', () => {
    selectPaper('a');
    expect(selection.id).toBe('a');
    selectPaper(null);
    expect(selection.id).toBe(null);
  });

  it('openTab activates the tab and selects the paper', () => {
    openTab(paper('a'));
    expect(viewer.activeId).toBe('a');
    expect(selection.id).toBe('a');
  });

  it('goHome keeps tabs open but activates the Library home', () => {
    openTab(paper('a'));
    goHome();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1);
  });

  it('closing the last tab lands on the Library home', () => {
    openTab(paper('a'));
    closeTab('a');
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
  });
});

describe('zen mode', () => {
  it('toggleZen only engages while a PDF tab is active', () => {
    toggleZen();
    expect(ui.zen).toBe(false); // home active — nothing to zen into
    openTab(paper('a'));
    toggleZen();
    expect(ui.zen).toBe(true);
    toggleZen();
    expect(ui.zen).toBe(false);
  });

  it('closing the last tab exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    closeTab('a');
    expect(ui.zen).toBe(false);
  });

  it('closing the last tab disarms the annotation tool', () => {
    // A tool armed with no reader on screen would silently re-arm on the next
    // opened tab (AnnotationTools re-applies the persisted tool per document).
    openTab(paper('a'));
    openTab(paper('b'));
    setActiveTool('highlight');
    closeTab('a');
    expect(annotationTools.active).toBe('highlight'); // a reader is still up
    closeTab('b');
    expect(annotationTools.active).toBeNull();
  });

  it('goHome exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    goHome();
    expect(ui.zen).toBe(false);
  });
});

describe('removePaper selection', () => {
  it('clears the selection when the selected paper is deleted', async () => {
    library.papers = [paper('x')];
    selectPaper('x');
    await removePaper('x');
    expect(selection.id).toBe(null);
  });
});
