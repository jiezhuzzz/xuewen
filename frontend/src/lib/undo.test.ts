import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    deletePaper: vi.fn(async () => {}),
    restorePaper: vi.fn(async () => {}),
    listPapers: vi.fn(async () => []),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
  };
});

import * as api from './api';
import { library, removePaper, removePapers } from './state.svelte';
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
  toasts.items.length = 0;
});

describe('paper delete undo', () => {
  it('removePaper shows a Deleted toast with an Undo action', async () => {
    await removePaper('p1');
    const t = toasts.items.find((x) => x.action);
    expect(t?.message).toMatch(/deleted/i);
    expect(t?.action?.label).toBe('Undo');
  });

  it('removePapers deletes each id and shows ONE combined Undo toast', async () => {
    library.papers = [paper('p1'), paper('p2')];
    await removePapers(['p1', 'p2']);
    expect((api.deletePaper as Mock).mock.calls.map(([id]) => id)).toEqual(['p1', 'p2']);
    const undoToasts = toasts.items.filter((x) => x.action);
    expect(undoToasts).toHaveLength(1);
    expect(undoToasts[0].message).toMatch(/2 papers deleted/i);
    undoToasts[0].action!.run();
    await vi.waitFor(() => {
      expect((api.restorePaper as Mock).mock.calls.map(([id]) => id)).toEqual(['p1', 'p2']);
    });
  });

  it('running Undo restores the paper and reloads the list', async () => {
    await removePaper('p1');
    toasts.items.find((x) => x.action)!.action!.run();
    await vi.waitFor(() => {
      expect(api.restorePaper as Mock).toHaveBeenCalledWith('p1');
      expect(api.listPapers as Mock).toHaveBeenCalled();
    });
    expect(toasts.items.some((x) => /restored/i.test(x.message))).toBe(true);
  });
});
