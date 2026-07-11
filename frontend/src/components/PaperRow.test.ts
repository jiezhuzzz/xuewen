import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PaperRow from './PaperRow.svelte';
import { selection, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const paper: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
  source: null, status: 'resolved', added_at: '',
};

beforeEach(() => {
  selection.id = null;
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('PaperRow', () => {
  it('single click opens the PDF tab and highlights the row', async () => {
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(viewer.tabs.map((t) => t.id)).toEqual(['p1']);
    expect(viewer.activeId).toBe('p1');
    expect(selection.id).toBe('p1');
  });

  it('opening an already-open paper activates its tab without duplicating', async () => {
    viewer.tabs = [{ id: 'p1', title: 'Attention Is All You Need' }];
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(viewer.tabs).toHaveLength(1);
    expect(viewer.activeId).toBe('p1');
  });
});
