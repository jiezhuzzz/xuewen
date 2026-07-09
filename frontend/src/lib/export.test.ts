import { beforeEach, describe, expect, it, vi } from 'vitest';
import { exportUrl } from './api';
import { bibFormat, copyCitation } from './state.svelte';
import type { Filters } from './types';

const baseFilters: Filters = { q: '', status: 'all', sort: 'year_desc', project: 'all' };

describe('exportUrl', () => {
  it('builds a url with only the format when no filters are set', () => {
    const url = exportUrl(baseFilters, 'bibtex');
    expect(url).toBe('/api/papers/export?format=bibtex');
  });

  it('includes active search, status, and project filters', () => {
    const url = exportUrl(
      { q: 'graph', status: 'resolved', sort: 'year_desc', project: 'p1' },
      'biblatex',
    );
    expect(url).toContain('q=graph');
    expect(url).toContain('status=resolved');
    expect(url).toContain('project=p1');
    expect(url).toContain('format=biblatex');
  });
});

describe('copyCitation', () => {
  beforeEach(() => {
    bibFormat.value = 'bibtex';
    vi.unstubAllGlobals();
  });

  it('fetches the entry in the current format and writes it to the clipboard', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    let requested = '';
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL) => {
        requested = String(url);
        return new Response('@article{x,\n}', { status: 200 });
      }),
    );

    bibFormat.value = 'biblatex';
    await copyCitation('aaaa1111');

    expect(requested).toBe('/api/papers/aaaa1111/export?format=biblatex');
    expect(writeText).toHaveBeenCalledWith('@article{x,\n}');
  });
});
