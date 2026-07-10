import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ChatPanel from './ChatPanel.svelte';
import { chat } from '../lib/chat.svelte';

beforeEach(() => {
  chat.available = true;
  chat.models = [{ id: '0', label: 'Mock A' }, { id: '1', label: 'Mock B' }];
  chat.modelId = '0';
  chat.open = true;
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

describe('ChatPanel', () => {
  it('shows the empty-state invitation and the model picker', () => {
    render(ChatPanel);
    expect(screen.getByText(/Ask about the methods/)).toBeInTheDocument();
    expect(screen.getByLabelText('Model')).toHaveValue('0');
  });

  it('changing the model persists the choice', async () => {
    render(ChatPanel);
    await userEvent.selectOptions(screen.getByLabelText('Model'), '1');
    expect(localStorage.getItem('xuewen-chat-model')).toBe('1');
  });

  it('minimize closes the panel; Escape does too', async () => {
    render(ChatPanel);
    await userEvent.click(screen.getByRole('button', { name: 'Minimize chat' }));
    expect(chat.open).toBe(false);
    chat.open = true;
    render(ChatPanel);
    await userEvent.click(screen.getAllByPlaceholderText('Ask about this paper…')[0]);
    await userEvent.keyboard('{Escape}');
    expect(chat.open).toBe(false);
  });

  it('clear asks for confirmation before deleting', async () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '' },
      { id: 2, role: 'assistant', content: 'a', model: 'Mock A', created_at: '' },
    ];
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchSpy);
    render(ChatPanel);
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
    render(ChatPanel);
    // 'Mock A' is also a <select> option's text; scope to the caption
    // paragraph so this doesn't collide with the model picker.
    expect(screen.getByText('Mock A', { selector: 'p' })).toBeInTheDocument();
  });
});
