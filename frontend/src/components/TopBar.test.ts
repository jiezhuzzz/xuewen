import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('../lib/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});

import * as api from '../lib/api';
import TopBar from './TopBar.svelte';
import { filters, stats } from '../lib/state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  filters.status = 'all';
  stats.value = { total: 9, resolved: 8, needs_review: 1 };
});

describe('TopBar review count', () => {
  it('clicking the review count filters the list to needs-review', async () => {
    render(TopBar);
    await userEvent.click(screen.getByRole('button', { name: /1 to review/ }));
    expect(filters.status).toBe('needs_review');
    expect(api.listPapers).toHaveBeenCalled();
  });

  it('hides the review count when nothing needs review', () => {
    stats.value = { total: 9, resolved: 9, needs_review: 0 };
    render(TopBar);
    expect(screen.queryByText(/to review/)).not.toBeInTheDocument();
  });
});

describe('TopBar counts', () => {
  it('shows the match count instead of library total while searching', async () => {
    const { library } = await import('../lib/state.svelte');
    filters.q = 'fuzzing';
    library.papers = [
      { id: 'p1', title: 'A', authors: [], venue: null, year: null, doi: null,
        arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
        status: 'resolved', added_at: '2026-01-01', starred: false, tags: [], projects: [] },
    ];
    render(TopBar);
    expect(screen.getByText('1 match')).toBeInTheDocument();
    expect(screen.queryByText('9 papers')).not.toBeInTheDocument();
  });
});
