import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  chat,
  clearChatThread,
  loadChatModels,
  loadThread,
  sendChatMessage,
  setChatModel,
  stopChatStream,
  toggleChat,
} from './chat.svelte';
import { viewer } from './state.svelte';

function sseBody(text: string): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(c) {
      c.enqueue(enc.encode(text));
      c.close();
    },
  });
}

function json(o: unknown): Response {
  return new Response(JSON.stringify(o), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

beforeEach(() => {
  localStorage.clear();
  viewer.tabs = [];
  viewer.activeId = null;
  chat.available = false;
  chat.models = [];
  chat.modelId = null;
  chat.open = false;
  chat.paperId = null;
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  chat.draft = '';
  vi.unstubAllGlobals();
});

describe('models', () => {
  it('loads models and picks the saved or first model', async () => {
    localStorage.setItem('xuewen-chat-model', '1');
    vi.stubGlobal('fetch', vi.fn(async () =>
      json({ available: true, models: [{ id: '0', label: 'A' }, { id: '1', label: 'B' }] }),
    ));
    await loadChatModels();
    expect(chat.available).toBe(true);
    expect(chat.modelId).toBe('1');
    setChatModel('0');
    expect(localStorage.getItem('xuewen-chat-model')).toBe('0');
  });

  it('stays unavailable when the API says so or fails', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ available: false, models: [] })));
    await loadChatModels();
    expect(chat.available).toBe(false);
  });
});

describe('toggleChat', () => {
  it('only opens with an active tab and available chat', () => {
    toggleChat();
    expect(chat.open).toBe(false);
    chat.available = true;
    viewer.tabs = [{ id: 'p1', title: 'T' }];
    viewer.activeId = 'p1';
    toggleChat();
    expect(chat.open).toBe(true);
  });
});

describe('sendChatMessage', () => {
  beforeEach(() => {
    chat.available = true;
    chat.models = [{ id: '0', label: 'Mock' }];
    chat.modelId = '0';
    chat.paperId = 'p1';
  });

  it('streams deltas and folds the finished exchange into messages', async () => {
    vi.stubGlobal('fetch', vi.fn(async () =>
      new Response(
        sseBody(
          'event: delta\ndata: {"text":"Hel"}\n\n' +
            'event: delta\ndata: {"text":"lo"}\n\n' +
            'event: done\ndata: {"id":7}\n\n',
        ),
        { status: 200 },
      ),
    ));
    chat.draft = 'what is this?';
    await sendChatMessage();
    expect(chat.messages.map((m) => m.role)).toEqual(['user', 'assistant']);
    expect(chat.messages[1].content).toBe('Hello');
    expect(chat.messages[1].model).toBe('Mock');
    expect(chat.pending).toBe(null);
    expect(chat.streaming).toBe(null);
    expect(chat.busy).toBe(false);
    expect(chat.error).toBe(null);
    expect(chat.draft).toBe('');
  });

  it('restores the draft and shows an inline error on failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () =>
      new Response(sseBody('event: error\ndata: {"message":"upstream 401"}\n\n'), { status: 200 }),
    ));
    chat.draft = 'hi';
    await sendChatMessage();
    expect(chat.messages).toEqual([]);
    expect(chat.draft).toBe('hi');
    expect(chat.error).toContain('upstream 401');
    expect(chat.error).toContain('Send again to retry.');
  });

  it('treats a stream that ends without done as interrupted', async () => {
    vi.stubGlobal('fetch', vi.fn(async () =>
      new Response(sseBody('event: delta\ndata: {"text":"He"}\n\n'), { status: 200 }),
    ));
    chat.draft = 'hi';
    await sendChatMessage();
    expect(chat.error).toContain('Send again to retry.');
    expect(chat.draft).toBe('hi');
  });

  it('a late abort after done does not disturb the folded exchange', async () => {
    const enc = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(enc.encode('event: delta\ndata: {"text":"Hi"}\n\nevent: done\ndata: {"id":3}\n\n'));
      },
      pull() {
        return Promise.reject(new DOMException('aborted', 'AbortError'));
      },
    });
    vi.stubGlobal('fetch', vi.fn(async () => new Response(body, { status: 200 })));
    chat.draft = 'q';
    await sendChatMessage();
    expect(chat.messages.map((m) => m.role)).toEqual(['user', 'assistant']);
    expect(chat.draft).toBe('');
    expect(chat.error).toBe(null);
    expect(chat.busy).toBe(false);
  });

  it('abort restores the draft without an error', async () => {
    vi.stubGlobal('fetch', vi.fn((_url: unknown, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () =>
          reject(new DOMException('aborted', 'AbortError')),
        );
      }),
    ));
    chat.draft = 'hi';
    const inflight = sendChatMessage();
    stopChatStream();
    await inflight;
    expect(chat.error).toBe(null);
    expect(chat.draft).toBe('hi');
    expect(chat.busy).toBe(false);
  });
});

describe('thread', () => {
  it('loadThread fetches once per paper and clearChatThread empties it', async () => {
    const fetchSpy = vi.fn(async (url: unknown, init?: RequestInit) => {
      if (init?.method === 'DELETE') return new Response(null, { status: 204 });
      return json([{ id: 1, role: 'user', content: 'q', model: null, created_at: '' }]);
    });
    vi.stubGlobal('fetch', fetchSpy);
    await loadThread('p1');
    expect(chat.messages).toHaveLength(1);
    await loadThread('p1'); // same paper -> no refetch
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    await clearChatThread();
    expect(chat.messages).toEqual([]);
  });
});
