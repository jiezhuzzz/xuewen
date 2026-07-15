import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    searchPapers: vi.fn(async () => ({
      semantic: { available: true, reason: null },
      results: [
        {
          paper: { id: 'p1', title: 'T', authors: [], venue: null, year: null, doi: null,
                   arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
                   status: 'resolved', added_at: '', starred: false, tags: [], projects: [] },
          match: { engine: 'keyword', field: 'body', snippet: 'a <mark>hit</mark>', page: 7 },
        },
      ],
    })),
    getSearchStatus: vi.fn(async () => ({
      fts: { indexed: 1, pending: 0, failed: 0 },
      vectors: { indexed: 0, pending: 3, failed: 0 },
      semantic_available: false,
      reason: 'no key',
    })),
  };
});

import * as api from './api';
import {
  filters, library, loadPapers, loadSearchStatus, searchMeta, searchOpts,
  semanticBlocked, setSearch, toggleSearchEngine, toggleSearchField,
} from './state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  Object.assign(searchOpts, {
    title: true, authors: true, abstract: true, body: true, keyword: true, semantic: true,
  });
  searchMeta.byId = {};
  searchMeta.semantic = { available: true, reason: null };
  searchMeta.pending = 0;
});

describe('search state', () => {
  it('loadPapers uses searchPapers when q is set and stores match info', async () => {
    filters.q = 'fuzz';
    await loadPapers();
    expect(api.searchPapers).toHaveBeenCalledOnce();
    expect(library.papers.map((p) => p.id)).toEqual(['p1']);
    expect(searchMeta.byId['p1'].snippet).toContain('<mark>');
  });

  it('loadPapers uses listPapers and clears match info when q is empty', async () => {
    searchMeta.byId = { p1: { engine: 'keyword', field: 'body', snippet: 'x', page: null } };
    await loadPapers();
    expect(api.listPapers).toHaveBeenCalledOnce();
    expect(Object.keys(searchMeta.byId)).toHaveLength(0);
  });

  it('setSearch debounces: keyword-only first, then full', async () => {
    vi.useFakeTimers();
    setSearch('fuzz');
    expect(api.searchPapers).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(200);
    expect(api.searchPapers).toHaveBeenCalledTimes(1);
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[0][3]).toBe(true); // keywordOnly
    await vi.advanceTimersByTimeAsync(600);
    expect(api.searchPapers).toHaveBeenCalledTimes(2);
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[1][3]).toBe(false);
    vi.useRealTimers();
  });

  it('cannot turn off the last field or engine', () => {
    Object.assign(searchOpts, { title: false, authors: false, abstract: false });
    toggleSearchField('body'); // body is the last field
    expect(searchOpts.body).toBe(true);
    searchOpts.semantic = false;
    toggleSearchEngine('keyword'); // keyword is the last engine
    expect(searchOpts.keyword).toBe(true);
  });

  it('semanticBlocked for authors-only or unavailable backend', async () => {
    expect(semanticBlocked()).toBe(false);
    Object.assign(searchOpts, { title: false, abstract: false, body: false });
    expect(semanticBlocked()).toBe(true);
    Object.assign(searchOpts, { title: true, abstract: true, body: true });
    await loadSearchStatus();
    expect(searchMeta.semantic.available).toBe(false);
    expect(semanticBlocked()).toBe(true);
    expect(searchMeta.pending).toBe(3); // max(fts.pending, vectors.pending)
  });
});
