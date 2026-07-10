import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import CommandPalette from './CommandPalette.svelte';
import { library, ui, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

beforeEach(() => {
  library.papers = [paper('p1', 'Attention Is All You Need'), paper('p2', 'Denoising Diffusion')];
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
