import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import TabBar from './TabBar.svelte';
import { closeTab, goHome, openTab, ui, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
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

  it('shows the zen toggle only while a PDF tab is active', async () => {
    render(TabBar);
    expect(screen.queryByRole('button', { name: 'Zen mode' })).not.toBeInTheDocument();
    openTab(paper('a', 'First Paper'));
    await Promise.resolve();
    expect(screen.getByRole('button', { name: 'Zen mode' })).toBeInTheDocument();
  });
});
