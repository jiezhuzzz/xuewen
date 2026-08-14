import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Annotation, NewAnnotation } from './types';

/// The server's rows, keyed by paper. The fakes below read and write this so
/// the tests exercise the cache-vs-server relationship, not a stub that always
/// echoes its input.
const server: Record<string, Record<string, Annotation>> = {};
let listFails: string | null = null;

vi.mock('./api', () => ({
  listAnnotations: vi.fn(async (paperId: string) => {
    if (listFails) throw new Error(listFails);
    return Object.values(server[paperId] ?? {});
  }),
  putAnnotation: vi.fn(async (paperId: string, id: string, body: NewAnnotation) => {
    const prev = server[paperId]?.[id];
    const saved: Annotation = {
      paper_id: paperId,
      id,
      ...body,
      created_at: prev?.created_at ?? '2026-08-14T00:00:00Z',
      updated_at: '2026-08-14T01:00:00Z',
    };
    (server[paperId] ??= {})[id] = saved;
    return saved;
  }),
  patchAnnotation: vi.fn(async (paperId: string, id: string, patch: Record<string, unknown>) => {
    const row = server[paperId]?.[id];
    if (!row) throw new Error('not found: 404');
    const note = 'note' in patch ? (patch.note as string) || null : row.note;
    const saved = { ...row, ...patch, note, updated_at: '2026-08-14T02:00:00Z' } as Annotation;
    server[paperId][id] = saved;
    return saved;
  }),
  deleteAnnotation: vi.fn(async (paperId: string, id: string) => {
    delete server[paperId]?.[id];
  }),
  clearAnnotations: vi.fn(async (paperId: string) => {
    const n = Object.keys(server[paperId] ?? {}).length;
    server[paperId] = {};
    return n;
  }),
}));

import * as api from './api';
import {
  annotationCount,
  annotationList,
  annotations,
  dropAnnotations,
  isLoaded,
  loadAnnotations,
  recolor,
  removeAllAnnotations,
  removeAnnotation,
  saveAnnotation,
  setNote,
} from './annotationStore.svelte';

function body(over: Partial<NewAnnotation> = {}): NewAnnotation {
  return {
    page_index: 0,
    kind: 'highlight',
    color: 'amber',
    quoted_text: 'quoted',
    note: null,
    payload: { annotation: { id: 'x' } },
    ...over,
  };
}

function seed(paperId: string, id: string, over: Partial<Annotation> = {}): void {
  (server[paperId] ??= {})[id] = {
    paper_id: paperId,
    id,
    ...body(),
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const k of Object.keys(server)) delete server[k];
  listFails = null;
  annotations.byPaper = {};
  annotations.loaded = {};
  annotations.error = {};
});

describe('loading', () => {
  it('fills the cache and marks the paper loaded', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    expect(isLoaded('p1')).toBe(true);
    expect(annotationCount('p1')).toBe(1);
    expect(annotations.error['p1']).toBeNull();
  });

  it('records a failure instead of throwing, and stays unloaded', async () => {
    listFails = 'network down';
    await expect(loadAnnotations('p1')).resolves.toBeUndefined();
    expect(annotations.error['p1']).toBe('network down');
    // Unloaded is NOT the same as empty: the sync loop must not read this
    // paper as "no marks" and start saving over the server's copy.
    expect(isLoaded('p1')).toBe(false);
  });

  it('replaces rather than merges, so a server-side delete disappears', async () => {
    seed('p1', 'a1');
    seed('p1', 'a2');
    await loadAnnotations('p1');
    delete server['p1']['a1'];
    await loadAnnotations('p1');
    expect(annotationList('p1').map((a) => a.id)).toEqual(['a2']);
  });
});

describe('ordering', () => {
  it('sorts by page, then creation time, then id', async () => {
    seed('p1', 'later-page', { page_index: 5 });
    seed('p1', 'b', { page_index: 1, created_at: '2026-08-14T00:00:02Z' });
    seed('p1', 'a', { page_index: 1, created_at: '2026-08-14T00:00:01Z' });
    seed('p1', 'tie-z', { page_index: 1, created_at: '2026-08-14T00:00:01Z' });
    await loadAnnotations('p1');
    expect(annotationList('p1').map((x) => x.id)).toEqual(['a', 'tie-z', 'b', 'later-page']);
  });

  it('is empty for a paper nobody has loaded', () => {
    expect(annotationList('nope')).toEqual([]);
    expect(annotationCount('nope')).toBe(0);
  });
});

describe('writing', () => {
  it('caches the server echo, not the local guess', async () => {
    await saveAnnotation('p1', 'a1', body());
    const row = annotations.byPaper['p1']['a1'];
    expect(row.updated_at).toBe('2026-08-14T01:00:00Z');
    expect(row.paper_id).toBe('p1');
  });

  it('re-saving the same id replaces rather than duplicating', async () => {
    await saveAnnotation('p1', 'a1', body());
    await saveAnnotation('p1', 'a1', body({ color: 'blue' }));
    expect(annotationCount('p1')).toBe(1);
    expect(annotationList('p1')[0].color).toBe('blue');
  });

  it('patches only the named field', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    await recolor('p1', 'a1', 'violet');
    expect(annotationList('p1')[0]).toMatchObject({ color: 'violet', quoted_text: 'quoted' });
  });

  it('sends an empty string to clear a note, which is what NULLs it', async () => {
    seed('p1', 'a1', { note: 'old' });
    await loadAnnotations('p1');
    await setNote('p1', 'a1', '');
    expect(api.patchAnnotation).toHaveBeenCalledWith('p1', 'a1', { note: '' });
    expect(annotationList('p1')[0].note).toBeNull();
  });

  it('leaves the cache alone when the server refuses', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    await expect(recolor('p1', 'missing', 'blue')).rejects.toThrow('404');
    expect(annotationCount('p1')).toBe(1);
  });
});

describe('removing', () => {
  it('drops one row from both the server and the cache', async () => {
    seed('p1', 'a1');
    seed('p1', 'a2');
    await loadAnnotations('p1');
    await removeAnnotation('p1', 'a1');
    expect(annotationList('p1').map((a) => a.id)).toEqual(['a2']);
    expect(Object.keys(server['p1'])).toEqual(['a2']);
  });

  it('clears a whole paper and reports the count', async () => {
    seed('p1', 'a1');
    seed('p1', 'a2');
    await loadAnnotations('p1');
    await expect(removeAllAnnotations('p1')).resolves.toBe(2);
    expect(annotationCount('p1')).toBe(0);
  });

  it('scopes every operation to its own paper', async () => {
    seed('p1', 'a1');
    seed('p2', 'a1'); // same annotation id, different paper
    await loadAnnotations('p1');
    await loadAnnotations('p2');
    await removeAllAnnotations('p1');
    expect(annotationCount('p1')).toBe(0);
    expect(annotationCount('p2')).toBe(1);
  });

  it('dropping a closed tab forgets the cache but not the server rows', async () => {
    seed('p1', 'a1');
    await loadAnnotations('p1');
    dropAnnotations('p1');
    expect(isLoaded('p1')).toBe(false);
    expect(annotationCount('p1')).toBe(0);
    expect(api.deleteAnnotation).not.toHaveBeenCalled();
    expect(Object.keys(server['p1'])).toEqual(['a1']);
  });
});
