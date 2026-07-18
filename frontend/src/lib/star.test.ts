import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return { ...mod, setStar: vi.fn(async () => {}), listPapers: vi.fn(async () => []) };
});

import * as api from './api';
import { filters, library, toggleStar } from './state.svelte';
import { toasts } from './toasts.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: 'T', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', starred: false, tags: [], projects: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1')];
  filters.starred = undefined;
  toasts.items = [];
});

describe('optimistic star toggle', () => {
  it('flips the star before the server responds', async () => {
    let resolve!: () => void;
    (api.setStar as Mock).mockImplementation(() => new Promise<void>((r) => (resolve = r)));
    const done = toggleStar('p1');
    expect(library.papers[0].starred).toBe(true); // flipped before the await settles
    resolve();
    await done;
    expect(library.papers[0].starred).toBe(true);
  });

  it('rolls back and toasts when the server rejects', async () => {
    (api.setStar as Mock).mockRejectedValue(new Error('boom'));
    await toggleStar('p1');
    expect(library.papers[0].starred).toBe(false);
    expect(toasts.items.some((t) => t.kind === 'error')).toBe(true);
  });
});
