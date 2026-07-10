import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DetailView from './DetailView.svelte';
import { library, selection, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const summary: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: '10.1/x', arxiv_id: null, dblp_key: null, cite_key: 'vaswani2017',
  url: null, source: 'crossref', status: 'resolved', added_at: '',
};

beforeEach(() => {
  selection.id = null;
  viewer.tabs = [];
  viewer.activeId = null;
  library.papers = [summary];
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(
        JSON.stringify({ ...summary, abstract: 'The dominant sequence transduction models…', project_ids: [] }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    ),
  );
});

describe('DetailView', () => {
  it('shows the welcome panel when nothing is selected', () => {
    render(DetailView);
    expect(screen.getByText(/Select a paper/i)).toBeInTheDocument();
  });

  it('renders the selected paper and opens its PDF', async () => {
    selection.id = 'p1';
    render(DetailView);
    expect(await screen.findByText('Attention Is All You Need')).toBeInTheDocument();
    expect(await screen.findByText(/dominant sequence transduction/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Open PDF' }));
    expect(viewer.activeId).toBe('p1');
  });
});
