import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    getPaper: vi.fn(async (id: string) => {
      if (id === 'dead') throw new Error('404');
      return { id } as never;
    }),
  };
});

import { activateTab, closeTab, goHome, initTabs, openTab, viewer } from './state.svelte';
import type { PaperSummary } from './types';

const TABS_KEY = 'xuewen-tabs';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', starred: false, tags: [], projects: [],
  };
}

function saved(): { tabs: { id: string; title: string }[]; activeId: string | null } {
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
        { id: 'a', title: 'A' },
        { id: 'b', title: 'B' },
      ],
      activeId: 'b',
    });
    activateTab('a');
    expect(saved().activeId).toBe('a');
    expect(viewer.activeId).toBe('a');
    goHome();
    expect(saved().activeId).toBe(null);
    closeTab('a');
    expect(saved().tabs).toEqual([{ id: 'b', title: 'B' }]);
  });

  it('initTabs restores tabs and the active tab', async () => {
    localStorage.setItem(
      TABS_KEY,
      JSON.stringify({ tabs: [{ id: 'a', title: 'A' }], activeId: 'a' }),
    );
    await initTabs();
    expect(viewer.tabs).toEqual([{ id: 'a', title: 'A' }]);
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
