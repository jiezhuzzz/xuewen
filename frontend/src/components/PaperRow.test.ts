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
  it('single click selects without opening a tab', async () => {
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(selection.id).toBe('p1');
    expect(viewer.tabs).toHaveLength(0);
  });

  it('double click opens the PDF tab', async () => {
    render(PaperRow, { props: { paper } });
    await userEvent.dblClick(screen.getByRole('button', { name: /Attention/ }));
    expect(viewer.tabs.map((t) => t.id)).toEqual(['p1']);
    expect(viewer.activeId).toBe('p1');
  });

  it('clicking while a PDF is active returns to the Library home to inspect', async () => {
    viewer.tabs = [{ id: 'other', title: 'Other' }];
    viewer.activeId = 'other';
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(selection.id).toBe('p1');
    expect(viewer.activeId).toBe(null); // home shows the detail
    expect(viewer.tabs).toHaveLength(1); // the open tab is untouched
  });
});
