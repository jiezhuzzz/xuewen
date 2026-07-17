import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  deleteTag,
  detailRefresh,
  filters,
  library,
  loadDetail,
  loadProjects,
  projects,
  removeProject,
  renameProject,
  renameTag,
  setProjectFilter,
  setStarFilter,
  setTagFilter,
  tags,
} from './state.svelte';

function stubFetch(handler: (url: string, init?: RequestInit) => unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const body = handler(String(url), init);
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }),
  );
}

describe('projects state', () => {
  beforeEach(() => {
    projects.items = [];
    filters.q = '';
    filters.project = 'all';
    filters.tag = undefined;
    filters.starred = undefined;
    library.papers = [];
    vi.unstubAllGlobals();
  });

  it('loads projects', async () => {
    stubFetch((url) => {
      if (url === '/api/projects') return [{ id: 'p1', name: 'Survey', paper_count: 2 }];
      return [];
    });
    await loadProjects();
    expect(projects.items).toHaveLength(1);
    expect(projects.items[0].name).toBe('Survey');
  });

  it('setProjectFilter sends the project query param', async () => {
    let lastUrl = '';
    stubFetch((url) => {
      lastUrl = url;
      return [];
    });
    await setProjectFilter('p1');
    expect(filters.project).toBe('p1');
    expect(lastUrl).toContain('project=p1');
  });

  it('the project/tag/starred filters combine via query qualifiers', async () => {
    stubFetch(() => []);

    await setProjectFilter('p1');
    expect(filters.project).toBe('p1');
    expect(filters.q).toContain('project:p1');

    await setTagFilter('security');
    expect(filters.tag).toBe('security');
    expect(filters.project).toBe('p1'); // filters AND together now

    await setStarFilter(true);
    expect(filters.starred).toBe(true);
    expect(filters.tag).toBe('security');

    // toggling each qualifier off removes only that filter
    await setProjectFilter('all');
    expect(filters.project).toBe('all');
    expect(filters.tag).toBe('security');
    await setTagFilter(undefined);
    expect(filters.tag).toBeUndefined();
    expect(filters.starred).toBe(true);
    await setStarFilter(false);
    expect(filters.q).toBe('');
  });

  it('setTagFilter and setStarFilter send the matching query params', async () => {
    let lastUrl = '';
    stubFetch((url) => {
      lastUrl = url;
      return [];
    });
    await setTagFilter('ml');
    expect(lastUrl).toContain('tag=ml');
    await setStarFilter(true);
    expect(lastUrl).toContain('starred=true');
  });
});

describe('global rename/delete clears the per-paper detail cache', () => {
  beforeEach(() => {
    projects.items = [];
    tags.items = [];
    filters.project = 'all';
    filters.tag = undefined;
    filters.starred = undefined;
    library.papers = [];
    vi.unstubAllGlobals();
  });

  function stubDetailAnd(handler: (url: string, init?: RequestInit) => unknown) {
    stubFetch((url, init) => {
      if (url.startsWith('/api/papers/') && !url.includes('/projects/') && !url.includes('/tags')) {
        return { id: 'x', title: 'X', authors: [], venue: null, year: null, doi: null,
          arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
          status: 'resolved', added_at: '', starred: false, tags: [], projects: [], summary: null };
      }
      return handler(url, init);
    });
  }

  it('renameProject and removeProject evict the cached detail and bump detailRefresh', async () => {
    stubDetailAnd(() => ({ id: 'p1', name: 'Survey', paper_count: 1 }));
    await loadDetail('x');
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const callsBefore = fetchMock.mock.calls.length;
    const refreshBefore = detailRefresh.n;

    await renameProject('p1', { name: 'Renamed' });
    expect(detailRefresh.n).toBeGreaterThan(refreshBefore);

    await loadDetail('x'); // must hit the network again: cache was cleared
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore);

    const refreshBefore2 = detailRefresh.n;
    await removeProject('p1');
    expect(detailRefresh.n).toBeGreaterThan(refreshBefore2);
    const callsBefore2 = fetchMock.mock.calls.length;
    await loadDetail('x');
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore2);
  });

  it('renameTag and deleteTag evict the cached detail and bump detailRefresh', async () => {
    stubDetailAnd(() => []);
    await loadDetail('x');
    const fetchMock = globalThis.fetch as ReturnType<typeof vi.fn>;
    const callsBefore = fetchMock.mock.calls.length;
    const refreshBefore = detailRefresh.n;

    await renameTag('t1', 'renamed');
    expect(detailRefresh.n).toBeGreaterThan(refreshBefore);

    await loadDetail('x'); // must hit the network again: cache was cleared
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore);

    const refreshBefore2 = detailRefresh.n;
    await deleteTag('t1');
    expect(detailRefresh.n).toBeGreaterThan(refreshBefore2);
    const callsBefore2 = fetchMock.mock.calls.length;
    await loadDetail('x');
    expect(fetchMock.mock.calls.length).toBeGreaterThan(callsBefore2);
  });
});
