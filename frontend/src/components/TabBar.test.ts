import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import TabBar from './TabBar.svelte';
import { closeTab, openTab, ui, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', starred: false, tags: [], projects: [],
  };
}

describe('TabBar', () => {
  beforeEach(() => {
    viewer.tabs = [];
    viewer.activeId = null;
    ui.zen = false;
  });

  it('renders one tab per open paper and closes them', async () => {
    openTab(paper('a', 'First Paper'));
    openTab(paper('b', 'Second Paper'));
    render(TabBar);
    expect(screen.getByText('First Paper')).toBeInTheDocument();
    expect(screen.getByText('Second Paper')).toBeInTheDocument();
    expect(viewer.tabs.length).toBe(2);
    expect(viewer.activeId).toBe('b'); // most-recently opened is active

    closeTab('b');
    expect(viewer.tabs.length).toBe(1);
    expect(viewer.activeId).toBe('a'); // falls back to a neighbor
  });

  it('always shows the Library home tab and returns home on click', async () => {
    openTab(paper('a', 'First Paper'));
    render(TabBar);
    const home = screen.getByRole('button', { name: 'Library' });
    expect(home).toBeInTheDocument();
    home.click();
    await Promise.resolve();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1); // tabs survive going home
  });

  it('marks the home tab current when no PDF tab is active', () => {
    render(TabBar);
    expect(screen.getByRole('button', { name: 'Library' })).toHaveAttribute('aria-current', 'page');
  });

  it('keeps the close button visible on keyboard focus and titles truncated tabs', () => {
    openTab(paper('a', 'A very long paper title'));
    render(TabBar);
    expect(screen.getByTitle('A very long paper title')).toBeInTheDocument();
    const close = screen.getByRole('button', { name: 'Close tab' });
    expect(close.className).toContain('focus-visible:opacity-100');
  });

  it('hosts no zen/info buttons (they live on the PDF toolbar)', async () => {
    openTab(paper('a', 'First Paper'));
    render(TabBar);
    expect(screen.queryByRole('button', { name: /zen/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Toggle info' })).not.toBeInTheDocument();
  });
});
