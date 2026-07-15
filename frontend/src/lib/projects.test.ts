import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  filters,
  library,
  loadProjects,
  projects,
  setProjectFilter,
  setStarFilter,
  setTagFilter,
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

  it('the project/tag/starred filters are mutually exclusive', async () => {
    stubFetch(() => []);

    await setProjectFilter('p1');
    expect(filters.project).toBe('p1');
    expect(filters.tag).toBeUndefined();
    expect(filters.starred).toBeUndefined();

    await setTagFilter('security');
    expect(filters.tag).toBe('security');
    expect(filters.project).toBe('all');
    expect(filters.starred).toBeUndefined();

    await setStarFilter(true);
    expect(filters.starred).toBe(true);
    expect(filters.project).toBe('all');
    expect(filters.tag).toBeUndefined();

    // setting the project filter again clears starred
    await setProjectFilter('p2');
    expect(filters.project).toBe('p2');
    expect(filters.tag).toBeUndefined();
    expect(filters.starred).toBeUndefined();
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
