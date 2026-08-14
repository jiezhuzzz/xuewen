import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    getPaper: vi.fn(async (id: string) => {
      if (id === 'dead') throw new Error('404');
      return { id } as never;
    }),
    // Echoes the name back the way the server does (it is authoritative about
    // trimming and empty -> null; nothing here exercises that).
    setPaperName: vi.fn(async (_id: string, name: string | null) => ({ name }) as never),
  };
});

import {
  activateTab,
  closeTab,
  goHome,
  initTabs,
  openTab,
  setPaperName,
  viewer,
} from './state.svelte';
import type { PaperSummary } from './types';

const TABS_KEY = 'xuewen-tabs';

function paper(id: string, title: string, name: string | null = null): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name, starred: false, tags: [], projects: [],
  };
}

function saved(): {
  tabs: { id: string; title: string; name: string | null }[];
  activeId: string | null;
} {
  return JSON.parse(localStorage.getItem(TABS_KEY)!);
}

beforeEach(() => {
  localStorage.clear();
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('tab persistence', () => {
  it('open/activate/close/goHome all write the tab set to storage', () => {
    openTab(paper('a', 'A'));
    openTab(paper('b', 'B'));
    expect(saved()).toEqual({
      tabs: [
        { id: 'a', title: 'A', name: null },
        { id: 'b', title: 'B', name: null },
      ],
      activeId: 'b',
    });
    activateTab('a');
    expect(saved().activeId).toBe('a');
    expect(viewer.activeId).toBe('a');
    goHome();
    expect(saved().activeId).toBe(null);
    closeTab('a');
    expect(saved().tabs).toEqual([{ id: 'b', title: 'B', name: null }]);
  });

  it("carries the paper's name onto the tab, and through storage", async () => {
    openTab(paper('a', 'Attention Is All You Need', 'Transformer'));
    expect(viewer.tabs[0].name).toBe('Transformer');
    expect(saved().tabs).toEqual([
      { id: 'a', title: 'Attention Is All You Need', name: 'Transformer' },
    ]);
    viewer.tabs = [];
    await initTabs();
    expect(viewer.tabs[0].name).toBe('Transformer');
  });

  it('relabels an already-open tab when the paper is renamed', async () => {
    openTab(paper('a', 'Attention Is All You Need'));
    await setPaperName('a', 'Transformer');
    expect(viewer.tabs[0].name).toBe('Transformer');
    expect(saved().tabs[0].name).toBe('Transformer'); // and the new label survives a reload
  });

  it('initTabs restores tabs and the active tab, defaulting a missing name', async () => {
    // No `name` key: exactly what a build from before the field wrote. Those
    // tabs must restore (with name null), not be rejected as malformed.
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({ tabs: [{ id: 'a', title: 'A' }], activeId: 'a' }),
    );
    await initTabs();
    expect(viewer.tabs).toEqual([{ id: 'a', title: 'A', name: null }]);
    expect(viewer.activeId).toBe('a');
  });

  it('initTabs drops tabs whose papers no longer exist', async () => {
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({
        tabs: [
          { id: 'a', title: 'A' },
          { id: 'dead', title: 'Gone' },
        ],
        activeId: 'dead',
      }),
    );
    await initTabs();
    expect(viewer.tabs.map((t) => t.id)).toEqual(['a']);
    expect(viewer.activeId).toBe(null); // the active tab died → land on home
    expect(saved().tabs.map((t) => t.id)).toEqual(['a']); // pruned set re-saved
  });

  it('initTabs tolerates corrupted or missing storage', async () => {
    localStorage.setItem(TABS_KEY, '{nope');
    await initTabs();
    expect(viewer.tabs).toEqual([]);
    localStorage.removeItem(TABS_KEY);
    await initTabs();
    expect(viewer.tabs).toEqual([]);
  });
});
