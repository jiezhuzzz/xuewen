import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ReaderDock from './ReaderDock.svelte';
import { chat } from '../lib/chat.svelte';
import { handleKeydown } from '../lib/shortcuts';
import { appSettings, dock, ui, viewer } from '../lib/state.svelte';

const detail = {
  id: 'p1', title: 'Attention', authors: ['Vaswani'], venue: 'NeurIPS', year: 2017,
  doi: null, arxiv_id: null, dblp_key: null, cite_key: 'vaswani2017', url: null,
  source: null, status: 'resolved', added_at: '2026-07-08T00:00:00Z',
  abstract: 'Abs.', starred: false, tags: [], projects: [], summary: null,
};

beforeEach(() => {
  viewer.activeId = 'p1';
  dock.open = true;
  dock.tab = 'details';
  ui.zen = false;
  appSettings.foldAbstract = false;
  chat.available = true;
  chat.models = [{ id: '0', label: 'Mock A' }];
  chat.modelId = '0';
  chat.paperId = 'p1';
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  chat.draft = '';
  localStorage.clear();
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(JSON.stringify(detail), {
        status: 200, headers: { 'content-type': 'application/json' },
      }),
    ),
  );
});

describe('ReaderDock', () => {
  it('opens on Details and switches to Ask via the tab', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    expect(await screen.findByText('Attention')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('tab', { name: /Ask/ }));
    expect(dock.tab).toBe('ask');
    expect(screen.getByPlaceholderText('Ask about this paper…')).toBeInTheDocument();
  });

  it('hides the Ask tab and degrades a restored ask tab when chat is unavailable', async () => {
    chat.available = false;
    dock.tab = 'ask';
    render(ReaderDock, { props: { id: 'p1' } });
    expect(screen.queryByRole('tab', { name: /Ask/ })).not.toBeInTheDocument();
    expect(await screen.findByText('Attention')).toBeInTheDocument(); // Details shown instead
    expect(dock.tab).toBe('details');
  });

  it('wires each tab to its panel for assistive tech', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    const panel = await screen.findByRole('tabpanel');
    expect(panel.id).toBeTruthy();
    expect(screen.getByRole('tab', { name: 'Details' })).toHaveAttribute('aria-controls', panel.id);
    expect(screen.getByRole('tab', { name: /Ask/ })).toHaveAttribute('aria-controls');
  });

  it('the close button closes the dock', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    await userEvent.click(screen.getByRole('button', { name: 'Close panel' }));
    expect(dock.open).toBe(false);
  });

  it('Escape inside the dock closes it without leaving zen', async () => {
    ui.zen = true;
    dock.tab = 'ask';
    render(ReaderDock, { props: { id: 'p1' } });
    await userEvent.click(screen.getByPlaceholderText('Ask about this paper…'));
    await userEvent.keyboard('{Escape}');
    expect(dock.open).toBe(false);
    expect(ui.zen).toBe(true);
  });

  it('Escape inside the dock never reaches the global shortcut handler', async () => {
    ui.zen = true;
    dock.tab = 'ask';
    // Mount the real app-level keydown handler: without the dock's
    // stopPropagation it would see the dock already closed and exit zen.
    window.addEventListener('keydown', handleKeydown);
    try {
      render(ReaderDock, { props: { id: 'p1' } });
      await userEvent.click(screen.getByPlaceholderText('Ask about this paper…'));
      await userEvent.keyboard('{Escape}');
      expect(dock.open).toBe(false);
      expect(ui.zen).toBe(true);
    } finally {
      window.removeEventListener('keydown', handleKeydown);
    }
  });

  it('the zen button toggles zen', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    await userEvent.click(screen.getByRole('button', { name: 'Zen mode' }));
    expect(ui.zen).toBe(true);
  });
});
