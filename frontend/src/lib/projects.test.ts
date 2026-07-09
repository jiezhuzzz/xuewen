import { beforeEach, describe, expect, it, vi } from 'vitest';
import { filters, library, loadProjects, projects, setProjectFilter } from './state.svelte';

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
    library.papers = [];
    vi.unstubAllGlobals();
  });

  it('loads projects', async () => {
    stubFetch((url) => {
      if (url === '/api/projects')
        return [{ id: 'p1', name: 'Survey', note: null, paper_count: 2 }];
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
});
