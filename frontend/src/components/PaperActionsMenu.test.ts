import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PaperActionsMenu from './PaperActionsMenu.svelte';
import { identifyState, library, viewer } from '../lib/state.svelte';
import type { PaperDetail } from '../lib/types';

const d: PaperDetail = {
  id: 'p1', title: 'Attention', authors: [], venue: null, year: null, doi: '10.1/x',
  arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
  status: 'resolved', added_at: '', abstract: null, project_ids: [], summary: null,
};

beforeEach(() => {
  identifyState.open = false;
  identifyState.paperId = null;
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('PaperActionsMenu', () => {
  it('opens the menu and launches Identify', async () => {
    render(PaperActionsMenu, { props: { d } });
    await userEvent.click(screen.getByRole('button', { name: /More actions/ }));
    await userEvent.click(screen.getByRole('menuitem', { name: /Identify/ }));
    expect(identifyState.open).toBe(true);
    expect(identifyState.paperId).toBe('p1');
  });

  it('requires confirmation before deleting', async () => {
    const fetchMock = vi.fn(async () =>
      new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
    );
    vi.stubGlobal('fetch', fetchMock);
    render(PaperActionsMenu, { props: { d } });
    await userEvent.click(screen.getByRole('button', { name: /More actions/ }));
    await userEvent.click(screen.getByRole('menuitem', { name: /Delete paper/ }));
    expect(fetchMock).not.toHaveBeenCalled(); // confirm step first
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).toHaveBeenCalled();
  });
});
