import { beforeEach, describe, expect, it, vi } from 'vitest';
import { library, openTab, removePaper, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

describe('removePaper', () => {
  beforeEach(() => {
    library.papers = [];
    viewer.tabs = [];
    viewer.activeId = null;
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

  it('deletes on the server, closes the tab, and drops it from the list', async () => {
    library.papers = [paper('x'), paper('y')];
    openTab(paper('x'));
    expect(viewer.tabs.length).toBe(1);

    await removePaper('x');

    expect(library.papers.map((p) => p.id)).toEqual(['y']);
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'DELETE',
    });
  });
});
