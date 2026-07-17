import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DockAsk from './DockAsk.svelte';
import { chat } from '../lib/chat.svelte';

beforeEach(() => {
  chat.available = true;
  chat.models = [{ id: '0', label: 'Mock A' }, { id: '1', label: 'Mock B' }];
  chat.modelId = '0';
  chat.paperId = 'p1';
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  chat.draft = '';
  localStorage.clear();
  vi.unstubAllGlobals();
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
});

describe('DockAsk', () => {
  it('shows the empty-state invitation and the model picker', () => {
    render(DockAsk);
    expect(screen.getByText(/Ask about the methods/)).toBeInTheDocument();
    expect(screen.getByLabelText('Model')).toHaveValue('0');
  });

  it('changing the model persists the choice', async () => {
    render(DockAsk);
    await userEvent.selectOptions(screen.getByLabelText('Model'), '1');
    expect(localStorage.getItem('xuewen-chat-model')).toBe('1');
  });

  it('clear asks for confirmation before deleting', async () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '' },
      { id: 2, role: 'assistant', content: 'a', model: 'Mock A', created_at: '' },
    ];
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchSpy);
    render(DockAsk);
    await userEvent.click(screen.getByRole('button', { name: 'Clear conversation' }));
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(screen.getByText('Clear this conversation?')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(fetchSpy).toHaveBeenCalled();
  });

  it('renders the model label under assistant turns', () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '' },
      { id: 2, role: 'assistant', content: 'a', model: 'Mock A', created_at: '' },
    ];
    render(DockAsk);
    expect(screen.getByText('Mock A', { selector: 'p' })).toBeInTheDocument();
  });
});
