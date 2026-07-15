import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PaperRow from './PaperRow.svelte';
import { closeContextMenu, contextMenu } from '../lib/contextMenu.svelte';
import { selection, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const paper: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
  source: null, status: 'resolved', added_at: '', starred: false, tags: [], projects: [],
};

beforeEach(() => {
  selection.id = null;
  viewer.tabs = [];
  viewer.activeId = null;
  closeContextMenu();
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

  it('shows a lone author unchanged', () => {
    render(PaperRow, { props: { paper: { ...paper, authors: ['Vaswani'] } } });
    expect(screen.getByText('Vaswani')).toBeInTheDocument();
  });

  it('shows both names when there are exactly two authors', () => {
    render(PaperRow, { props: { paper: { ...paper, authors: ['Vaswani', 'Polosukhin'] } } });
    expect(screen.getByText('Vaswani, Polosukhin')).toBeInTheDocument();
  });

  it('collapses three or more authors to first and last only', () => {
    render(PaperRow, {
      props: { paper: { ...paper, authors: ['Vaswani', 'Shazeer', 'Parmar', 'Polosukhin'] } },
    });
    expect(screen.getByText('Vaswani … Polosukhin')).toBeInTheDocument();
    expect(screen.queryByText(/Shazeer/)).not.toBeInTheDocument();
  });

  it('shows the abbreviated venue with the full name as a tooltip', () => {
    const venue = '2025 IEEE Symposium on Security and Privacy (SP)';
    render(PaperRow, { props: { paper: { ...paper, venue } } });
    const el = screen.getByText('S&P');
    expect(el).toBeInTheDocument();
    expect(el).toHaveAttribute('title', venue);
    expect(screen.queryByText(/Symposium/)).not.toBeInTheDocument();
  });

  it('clicking the star toggles it via the API without opening the paper', async () => {
    const fetchMock = vi.fn(
      async () => new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
    );
    vi.stubGlobal('fetch', fetchMock);
    render(PaperRow, { props: { paper: { ...paper, starred: false } } });
    const star = screen.getByRole('button', { name: 'Star paper' });
    expect(star).toHaveAttribute('aria-pressed', 'false');
    await userEvent.click(star);
    expect(fetchMock).toHaveBeenCalled();
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'PUT',
    });
    // The row's own open() must not have fired from the star click.
    expect(viewer.tabs).toHaveLength(0);
    expect(selection.id).toBeNull();
    vi.unstubAllGlobals();
  });

  it('activating the star by keyboard toggles it without opening the paper', async () => {
    const fetchMock = vi.fn(
      async () => new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
    );
    vi.stubGlobal('fetch', fetchMock);
    render(PaperRow, { props: { paper: { ...paper, starred: false } } });
    const star = screen.getByRole('button', { name: 'Star paper' });
    star.focus();
    await userEvent.keyboard('{Enter}');
    // The star's own action must run: the row's keydown handler must NOT
    // swallow the synthetic click on the focused nested button.
    expect(fetchMock).toHaveBeenCalled();
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'PUT',
    });
    // ...and the row itself must not have opened the paper.
    expect(viewer.tabs).toHaveLength(0);
    expect(selection.id).toBeNull();
    vi.unstubAllGlobals();
  });

  it('right-click opens the context menu and highlights the row without opening the PDF', async () => {
    render(PaperRow, { props: { paper } });
    await fireEvent.contextMenu(screen.getByRole('button', { name: /Attention/ }));
    expect(contextMenu.open).toBe(true);
    expect(contextMenu.paper?.id).toBe('p1');
    // Highlighted (selected) but the PDF tab must not have opened.
    expect(selection.id).toBe('p1');
    expect(viewer.tabs).toHaveLength(0);
  });

  it('Enter on the row itself still opens the paper', async () => {
    render(PaperRow, { props: { paper } });
    const row = screen.getByRole('button', { name: /Attention/ });
    row.focus();
    await userEvent.keyboard('{Enter}');
    expect(viewer.tabs.map((t) => t.id)).toEqual(['p1']);
    expect(selection.id).toBe('p1');
  });
});
