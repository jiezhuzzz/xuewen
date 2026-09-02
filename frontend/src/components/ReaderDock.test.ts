import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ReaderDock from './ReaderDock.svelte';
import { chat } from '../lib/chat.svelte';
import { handleKeydown } from '../lib/shortcuts';
import { viewer } from '../lib/tabs.svelte';
import { appSettings, dock, ui } from '../lib/ui.svelte';

const detail = {
  id: 'p1', title: 'Attention', authors: ['Vaswani'], venue: 'NeurIPS', year: 2017,
  doi: null, arxiv_id: null, dblp_key: null, cite_key: 'vaswani2017', url: null,
  source: null, status: 'resolved', added_at: '2026-07-08T00:00:00Z',
  abstract: 'Abs.', name: null, starred: false, tags: [], projects: [], summary: null,
};

beforeEach(() => {
  viewer.activeId = 'p1';
  dock.open = true;
  dock.entry = null;
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
  it('carries the record and the composer on one surface, with no tabs', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    expect(await screen.findByText('Attention')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Ask about this paper…')).toBeInTheDocument();
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(screen.queryByRole('tab')).not.toBeInTheDocument();
  });

  it('drops the composer when chat is unavailable, keeping the record', async () => {
    chat.available = false;
    render(ReaderDock, { props: { id: 'p1' } });
    expect(await screen.findByText('Attention')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText('Ask about this paper…')).not.toBeInTheDocument();
  });

  it('renders the thread below the record in the same scroll', async () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'Why scaled attention?', model: null, created_at: '', tools: null },
      { id: 2, role: 'assistant', content: 'Gradients.', model: 'mock', created_at: '', tools: null },
    ];
    render(ReaderDock, { props: { id: 'p1' } });
    const record = await screen.findByText('Attention');
    const answer = screen.getByText('Gradients.');
    expect(record.compareDocumentPosition(answer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("an 'ask' entry focuses the composer and is consumed", async () => {
    dock.entry = 'ask';
    render(ReaderDock, { props: { id: 'p1' } });
    await screen.findByText('Attention');
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Ask about this paper…'));
    expect(dock.entry).toBeNull();
  });

  it('the close button closes the dock', async () => {
    render(ReaderDock, { props: { id: 'p1' } });
    await userEvent.click(screen.getByRole('button', { name: 'Close panel' }));
    expect(dock.open).toBe(false);
  });

  it('Escape inside the dock closes it without leaving zen', async () => {
    ui.zen = true;
    render(ReaderDock, { props: { id: 'p1' } });
    await userEvent.click(screen.getByPlaceholderText('Ask about this paper…'));
    await userEvent.keyboard('{Escape}');
    expect(dock.open).toBe(false);
    expect(ui.zen).toBe(true);
  });

  it('Escape inside the dock never reaches the global shortcut handler', async () => {
    ui.zen = true;
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
