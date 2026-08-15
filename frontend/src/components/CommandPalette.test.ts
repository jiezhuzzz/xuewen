import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('../lib/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});

import * as api from '../lib/api';
import CommandPalette from './CommandPalette.svelte';
import { library } from '../lib/library.svelte';
import { viewer } from '../lib/tabs.svelte';
import { ui } from '../lib/ui.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // The palette's paper corpus comes from its own unfiltered fetch, not the
  // (possibly filtered) library.papers — seed the fetch.
  (api.listPapers as Mock).mockResolvedValue([
    paper('p1', 'Attention Is All You Need'),
    paper('p2', 'Denoising Diffusion'),
  ]);
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
  ui.paletteOpen = true;
});

describe('CommandPalette', () => {
  it('filters papers by fuzzy query and opens on Enter', async () => {
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'atten');
    expect(screen.getByText('Attention Is All You Need')).toBeInTheDocument();
    expect(screen.queryByText('Denoising Diffusion')).not.toBeInTheDocument();
    await userEvent.keyboard('{Enter}');
    expect(viewer.activeId).toBe('p1');
    expect(ui.paletteOpen).toBe(false);
  });

  it('finds papers outside the currently filtered sidebar view', async () => {
    // A project pill / search left only p2 in the sidebar list; ⌘K must
    // still reach p1 through its own unfiltered corpus.
    library.papers = [paper('p2', 'Denoising Diffusion')];
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'atten');
    expect(screen.getByText('Attention Is All You Need')).toBeInTheDocument();
  });

  it('falls back to the sidebar list while the corpus fetch is pending', async () => {
    (api.listPapers as Mock).mockReturnValue(new Promise(() => {})); // never settles
    library.papers = [paper('p3', 'Interim Paper')];
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'interim');
    expect(screen.getByText('Interim Paper')).toBeInTheDocument();
  });

  it('shows key hints beside actions that have a shortcut', () => {
    render(CommandPalette);
    const zen = screen.getByRole('option', { name: /toggle zen mode/i });
    expect(within(zen).getByText('z', { selector: 'kbd' })).toBeInTheDocument();
    const pane = screen.getByRole('option', { name: /toggle list pane/i });
    expect(within(pane).getByText('[', { selector: 'kbd' })).toBeInTheDocument();
  });

  it('offers a Keyboard shortcuts action that opens the help overlay', async () => {
    ui.helpOpen = false;
    render(CommandPalette);
    const row = screen.getByRole('option', { name: /keyboard shortcuts/i });
    await userEvent.click(within(row).getByRole('button'));
    expect(ui.helpOpen).toBe(true);
    expect(ui.paletteOpen).toBe(false);
  });

  it('lists actions and runs them', async () => {
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'import');
    await userEvent.click(screen.getByText('Import papers…'));
    expect(ui.importOpen).toBe(true);
    expect(ui.paletteOpen).toBe(false);
  });

  it('closes on Escape', async () => {
    render(CommandPalette);
    await userEvent.keyboard('{Escape}');
    expect(ui.paletteOpen).toBe(false);
  });

  it('keeps focus on the input when Tab is pressed', async () => {
    render(CommandPalette);
    const input = screen.getByRole('combobox');
    (input as HTMLElement).focus();
    await userEvent.keyboard('{Tab}');
    expect(document.activeElement).toBe(input);
  });
});
