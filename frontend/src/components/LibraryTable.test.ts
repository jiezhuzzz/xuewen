import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('../lib/api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    listProjects: vi.fn(async () => []),
    listTags: vi.fn(async () => []),
    setStar: vi.fn(async () => {}),
    addTag: vi.fn(async (_id: string, name: string) => ({ id: `t-${name}`, name })),
    addPaperToProject: vi.fn(async () => {}),
    deletePaper: vi.fn(async () => {}),
    restorePaper: vi.fn(async () => {}),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
  };
});

import * as api from '../lib/api';
import LibraryTable from './LibraryTable.svelte';
import { filters, library, projects, selection, viewer } from '../lib/state.svelte';
import { toasts } from '../lib/toasts.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string, extra: Partial<PaperSummary> = {}): PaperSummary {
  return {
    id, title, authors: ['Ada Lovelace', 'Alan Turing', 'Grace Hopper'], venue: 'NDSS',
    year: 2026, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
    source: null, status: 'resolved', added_at: '2026-07-01T00:00:00Z', starred: false,
    tags: [], projects: [], ...extra,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1', 'First Paper'), paper('p2', 'Second Paper', { starred: true })];
  projects.items = [{ id: 'pr1', name: 'RTOS Fuzzing', paper_count: 0 }];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  toasts.items.length = 0;
  localStorage.clear();
  Object.assign(filters, {
    q: '', status: 'all', sort: 'year_desc', project: 'all', tag: undefined, starred: undefined,
  });
});

describe('LibraryTable', () => {
  it('renders one row per paper and opens a paper from its title', async () => {
    render(LibraryTable);
    expect(screen.getAllByRole('row')).toHaveLength(3); // header + 2 papers
    expect(screen.getAllByText('Ada Lovelace … Grace Hopper')).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'First Paper' }));
    expect(viewer.activeId).toBe('p1');
  });

  it('the Year header toggles sort direction and reloads', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('button', { name: /^year/i }));
    expect(filters.sort).toBe('year_asc');
    expect(api.listPapers as Mock).toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: /^year/i }));
    expect(filters.sort).toBe('year_desc');
  });

  it('selecting rows shows the bulk bar; select-all and clear work', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select first paper/i }));
    expect(screen.getByText('1 selected')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    expect(screen.getByText('2 selected')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /clear selection/i }));
    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
  });

  it('bulk star stars only the unstarred selected papers', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.click(screen.getByRole('button', { name: /^star$/i }));
    await vi.waitFor(() => {
      expect((api.setStar as Mock).mock.calls).toEqual([['p1', true]]); // p2 already starred
    });
  });

  it('bulk tag adds the tag to every selected paper', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.type(screen.getByPlaceholderText(/add tag/i), 'nlp/eval');
    await userEvent.click(screen.getByRole('button', { name: /apply tag/i }));
    await vi.waitFor(() => {
      expect((api.addTag as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
      expect((api.addTag as Mock).mock.calls.every((c) => c[1] === 'nlp/eval')).toBe(true);
    });
  });

  it('bulk add-to-project adds every selected paper', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.selectOptions(screen.getByRole('combobox', { name: /add to project/i }), 'pr1');
    await vi.waitFor(() => {
      expect((api.addPaperToProject as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
    });
  });

  it('bulk delete confirms, deletes all, and shows one combined Undo toast', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.click(screen.getByRole('button', { name: /^delete$/i }));
    expect(api.deletePaper as Mock).not.toHaveBeenCalled(); // confirm first
    await userEvent.click(screen.getByRole('button', { name: 'Delete 2' }));
    await vi.waitFor(() => {
      expect((api.deletePaper as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
    });
    const undoToasts = toasts.items.filter((t) => t.action);
    expect(undoToasts).toHaveLength(1);
    expect(undoToasts[0].message).toMatch(/2 papers deleted/);
  });

  it('highlights the j/k selection cursor row', async () => {
    selection.id = 'p2';
    render(LibraryTable);
    const row = screen.getByRole('button', { name: 'Second Paper' }).closest('tr')!;
    expect(row.dataset.cursor).toBe('true');
  });

  it('hides sort arrows and disables sort buttons while a search is active', () => {
    filters.q = 'fuzzing';
    render(LibraryTable);
    const year = screen.getByRole('button', { name: 'Year' });
    expect(year).toBeDisabled();
    expect(year.title).toMatch(/relevance/i);
    // No aria-sort claim while relevance-ranked.
    for (const th of screen.getAllByRole('columnheader')) {
      expect(th).not.toHaveAttribute('aria-sort');
    }
  });

  it('shows em-dash placeholders for missing metadata and opens Identify from the pill', async () => {
    const { identifyState } = await import('../lib/state.svelte');
    library.papers = [
      paper('p3', 'Mystery Paper', { authors: [], venue: null, year: null, status: 'needs_review' }),
    ];
    render(LibraryTable);
    // authors + venue + year each show a placeholder dash
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(3);
    await userEvent.click(screen.getByRole('button', { name: /resolve metadata/i }));
    expect(identifyState.open).toBe(true);
    expect(identifyState.paperId).toBe('p3');
  });
});
