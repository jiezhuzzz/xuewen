import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { appSettings, library, openTab, removePaper, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';
import InfoPanel from './InfoPanel.svelte';

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

function detail(id: string) {
  return {
    id, title: 'Attention', authors: ['Vaswani'], venue: 'NeurIPS', year: 2017,
    doi: '10.1/x', arxiv_id: '1706.03762', dblp_key: null, cite_key: 'vaswani2017',
    url: null, source: 'crossref', status: 'resolved', added_at: '2026-07-08T00:00:00Z',
    abstract: 'The dominant sequence transduction models…', project_ids: [],
  };
}

describe('InfoPanel', () => {
  beforeEach(() => {
    viewer.infoOpen = true;
    appSettings.foldAbstract = false;
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify(detail('info1')), {
          status: 200, headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('renders the title, identifier pills, and abstract', async () => {
    render(InfoPanel, { props: { id: 'info1' } });
    expect(await screen.findByText('Attention')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /DOI/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /arXiv/ })).toBeInTheDocument();
    expect(screen.getByText(/dominant sequence transduction/)).toBeInTheDocument();
  });

  it('collapses the abstract', async () => {
    render(InfoPanel, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    await userEvent.click(screen.getByRole('button', { name: /Abstract/ }));
    expect(screen.queryByText(/dominant sequence transduction/)).not.toBeInTheDocument();
  });

  it('the close button remembers the panel as closed', async () => {
    render(InfoPanel, { props: { id: 'info1' } });
    await screen.findByText('Attention');
    await userEvent.click(screen.getByRole('button', { name: /Close details/ }));
    expect(viewer.infoOpen).toBe(false);
    expect(localStorage.getItem('xuewen-info-open')).toBe('0');
  });

  it('renders the LLM summary when present', async () => {
    // Use a fresh id: loadDetail caches by id, and 'info1' is already cached
    // (without a summary) by earlier tests in this file.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            ...detail('info-summary'),
            summary: { tldr: 'Short.', problem: 'P', approach: 'A', results: 'R', limitations: 'L' },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    );

    render(InfoPanel, { props: { id: 'info-summary' } });
    expect(await screen.findByText('Short.')).toBeInTheDocument();
    expect(screen.getByText(/Results/i)).toBeInTheDocument();
  });

  it('starts the abstract folded when fold_abstract is true', async () => {
    appSettings.foldAbstract = true;
    render(InfoPanel, { props: { id: 'info1' } });
    const toggle = await screen.findByRole('button', { name: /abstract/i });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
  });
});
