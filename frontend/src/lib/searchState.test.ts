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
  filters, library, loadPapers, loadSearchStatus, projects, searchMeta, searchOpts,
  semanticBlocked, setProjectFilter, setSearch, setStarFilter, setTagFilter,
  toggleSearchEngine, toggleSearchField,
} from './state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(filters, {
    q: '', status: 'all', project: 'all', tag: undefined, starred: undefined,
  });
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
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[0][2]).toBe(true); // keywordOnly
    await vi.advanceTimersByTimeAsync(600);
    expect(api.searchPapers).toHaveBeenCalledTimes(2);
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[1][2]).toBe(false);
    vi.useRealTimers();
  });

  it('setSearch syncs parsed qualifiers into the filter cache', () => {
    vi.useFakeTimers();
    projects.items = [{ id: 'p1', name: 'Thesis', paper_count: 0 }];
    setSearch('tag:nlp project:thesis is:starred status:resolved attention');
    expect(filters.tag).toBe('nlp');
    expect(filters.project).toBe('p1'); // name resolved case-insensitively
    expect(filters.starred).toBe(true);
    expect(filters.status).toBe('resolved');
    projects.items = [];
    vi.useRealTimers();
  });

  it('setSearch clears stale cached filters when qualifiers are removed', () => {
    vi.useFakeTimers();
    setSearch('tag:nlp');
    setSearch('');
    expect(filters.tag).toBeUndefined();
    expect(filters.starred).toBeUndefined();
    expect(filters.status).toBe('all');
    expect(filters.project).toBe('all');
    vi.useRealTimers();
  });

  it('pill setters edit the query string and combine (no mutual exclusion)', () => {
    projects.items = [{ id: 'p1', name: 'My Thesis', paper_count: 0 }];
    void setTagFilter('nlp');
    void setStarFilter(true);
    void setProjectFilter('p1');
    expect(filters.q).toContain('tag:nlp');
    expect(filters.q).toContain('is:starred');
    expect(filters.q).toContain('project:"My Thesis"');
    void setTagFilter(undefined);
    expect(filters.q).not.toContain('tag:nlp');
    expect(filters.q).toContain('is:starred'); // no more mutual exclusion
    projects.items = [];
  });

  it('in: qualifiers sync the field toggles', () => {
    vi.useFakeTimers();
    setSearch('in:title attention');
    expect(searchOpts.title).toBe(true);
    expect(searchOpts.body).toBe(false);
    setSearch('attention');
    expect(searchOpts.body).toBe(true); // no in: tokens = all fields
    vi.useRealTimers();
  });

  it('toggleSearchField writes in: tokens into the query', () => {
    vi.useFakeTimers();
    setSearch('attention');
    toggleSearchField('body'); // turn body off
    expect(filters.q).toContain('in:title');
    expect(filters.q).toContain('in:abstract');
    expect(filters.q).toContain('in:authors');
    expect(filters.q).not.toContain('in:body');
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
