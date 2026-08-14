import { fireEvent, render, screen } from '@testing-library/svelte';
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
import { columnWidths, resetColumnWidths } from '../lib/columnWidths.svelte';
import { PINNED_COLUMNS } from '../lib/tableColumns';
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

// This jsdom build has no PointerEvent; MouseEvent carries the same fields
// the resize action reads (clientX/button), and pointer capture is try/caught.
function pointerEvent(type: string, clientX: number): Event {
  const Ctor: typeof MouseEvent =
    typeof PointerEvent === 'undefined' ? MouseEvent : (PointerEvent as unknown as typeof MouseEvent);
  return new Ctor(type, { clientX, button: 0, bubbles: true, cancelable: true });
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1', 'First Paper'), paper('p2', 'Second Paper', { starred: true })];
  projects.items = [{ id: 'pr1', name: 'RTOS Fuzzing', paper_count: 0 }];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  toasts.items.length = 0;
  resetColumnWidths(); // module singleton — a prior test's widths would leak
  localStorage.clear();
  Object.assign(filters, {
    q: '', status: 'all', sort: 'year_desc', project: 'all', tag: undefined, starred: undefined,
  });
});

describe('LibraryTable', () => {
  it('renders one row per paper and opens a paper from its title', async () => {
    render(LibraryTable);
    expect(screen.getAllByRole('row')).toHaveLength(3); // header + 2 papers
    expect(screen.getAllByText('Ada Lovelace')).toHaveLength(2); // first-author column
    expect(screen.getAllByText('Grace Hopper')).toHaveLength(2); // last-author column
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

  it('shows em-dash placeholders for missing metadata, without a status pill', () => {
    library.papers = [
      paper('p3', 'Mystery Paper', { authors: [], venue: null, year: null, status: 'needs_review' }),
    ];
    render(LibraryTable);
    // first author + last author + venue + year each show a placeholder dash
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(4);
    // The table deliberately carries no needs-review pill; the sidebar list
    // and the right-click Identify… action cover that.
    expect(screen.queryByText(/needs review/i)).not.toBeInTheDocument();
  });

  it('a single-author paper shows that author in both author columns', () => {
    library.papers = [paper('p4', 'Solo Work', { authors: ['Ada Lovelace'] })];
    render(LibraryTable);
    expect(screen.getAllByText('Ada Lovelace')).toHaveLength(2);
  });

  it('shows the abbreviated venue in the cell and the raw venue in the tooltip', () => {
    const raw = '2019 IEEE Symposium on Security and Privacy (SP)';
    library.papers = [paper('p5', 'Secure Paper', { venue: raw })];
    render(LibraryTable);
    const cell = screen.getByText('S&P');
    expect(cell.closest('[title]')?.getAttribute('title')).toBe(raw);
  });

  it('renders one resize separator per pinned column', () => {
    render(LibraryTable);
    expect(screen.getAllByRole('separator')).toHaveLength(6);
  });

  it('dragging a handle commits and persists the width', async () => {
    render(LibraryTable);
    const handle = screen.getByRole('separator', { name: 'Resize Title column' });
    await fireEvent(handle, pointerEvent('pointerdown', 100));
    await fireEvent(handle, pointerEvent('pointermove', 150));
    await fireEvent(handle, pointerEvent('pointerup', 150));
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth + 50);
    const saved = JSON.parse(localStorage.getItem('xuewen-library-columns') ?? '{}');
    expect(saved.title).toBe(PINNED_COLUMNS.title.defaultWidth + 50);
    // A real browser fires a post-drag click that consumes the one-shot
    // swallower; jsdom doesn't, so drain it or it leaks into the next test.
    window.dispatchEvent(new MouseEvent('click'));
  });

  it('double-clicking a handle auto-fits that column to its content', async () => {
    render(LibraryTable);
    await fireEvent.dblClick(screen.getByRole('separator', { name: 'Resize Title column' }));
    // jsdom has no canvas, so the estimate measurer runs: the short fixture
    // titles land the column at its minimum — down from the default.
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.minWidth);
    expect(localStorage.getItem('xuewen-library-columns')).not.toBeNull();
  });

  it('right-clicking the header offers reset-to-defaults', async () => {
    render(LibraryTable);
    columnWidths.title = 400;
    await fireEvent.contextMenu(screen.getAllByRole('row')[0]);
    await userEvent.click(screen.getByRole('menuitem', { name: /reset to default widths/i }));
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('auto-fit all from the header menu commits and persists the pinned columns', async () => {
    render(LibraryTable);
    await fireEvent.contextMenu(screen.getAllByRole('row')[0]);
    await userEvent.click(screen.getByRole('menuitem', { name: /auto-fit all columns/i }));
    expect(columnWidths.title).toBeLessThan(PINNED_COLUMNS.title.defaultWidth);
    const saved = JSON.parse(localStorage.getItem('xuewen-library-columns') ?? '{}');
    expect(saved.title).toBe(columnWidths.title);
  });
});
