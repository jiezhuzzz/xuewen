import { readSse } from './sse';
import { viewer } from './state.svelte';

export interface ChatModelInfo {
  id: string;
  label: string;
}
export interface ChatTurn {
  id: number;
  role: 'user' | 'assistant';
  content: string;
  model: string | null;
  created_at: string;
}

/// The floating paper-chat. `pending` is the user message awaiting a reply,
/// `streaming` the assistant text accumulating under it; both fold into
/// `messages` only when the server confirms the exchange was stored.
export const chat = $state<{
  available: boolean;
  models: ChatModelInfo[];
  modelId: string | null;
  open: boolean;
  paperId: string | null;
  messages: ChatTurn[];
  pending: string | null;
  streaming: string | null;
  busy: boolean;
  error: string | null;
  draft: string;
}>({
  available: false,
  models: [],
  modelId: null,
  open: false,
  paperId: null,
  messages: [],
  pending: null,
  streaming: null,
  busy: false,
  error: null,
  draft: '',
});

// Bumped whenever the thread identity changes; in-flight streams from a
// superseded thread must not write into the current one (same pattern as
// identifySession in state.svelte.ts).
let session = 0;
let aborter: AbortController | null = null;
// Client-assigned ids for optimistic turns: negative and descending, so they
// never collide with server row ids or with each other in keyed eaches.
let localId = -1;

export async function loadChatModels(): Promise<void> {
  try {
    const resp = await fetch('/api/chat/models');
    if (!resp.ok) throw new Error(String(resp.status));
    const body = (await resp.json()) as { available: boolean; models: ChatModelInfo[] };
    chat.models = body.models;
    chat.available = body.available && body.models.length > 0;
    const saved = localStorage.getItem('xuewen-chat-model');
    chat.modelId = chat.models.some((m) => m.id === saved)
      ? saved
      : (chat.models[0]?.id ?? null);
  } catch {
    chat.available = false;
  }
}

export function setChatModel(id: string): void {
  chat.modelId = id;
  localStorage.setItem('xuewen-chat-model', id);
}

/// The bubble/`c` toggle: chat only exists over an open PDF.
export function toggleChat(): void {
  if (!chat.available || viewer.activeId === null) return;
  chat.open = !chat.open;
}

export async function loadThread(paperId: string): Promise<void> {
  if (chat.paperId === paperId) return;
  const my = ++session;
  aborter?.abort();
  chat.paperId = paperId;
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  try {
    const resp = await fetch(`/api/papers/${encodeURIComponent(paperId)}/chat`);
    if (!resp.ok) throw new Error(String(resp.status));
    const rows = (await resp.json()) as ChatTurn[];
    if (my === session) chat.messages = rows;
  } catch {
    if (my === session)
      chat.error = 'Could not load this conversation. Close and reopen the chat to retry.';
  }
}

export async function sendChatMessage(): Promise<void> {
  const text = chat.draft.trim();
  if (!text || chat.busy || !chat.paperId || chat.modelId === null) return;
  const my = session;
  chat.pending = text;
  chat.draft = '';
  chat.busy = true;
  chat.error = null;
  chat.streaming = '';
  const myAborter = new AbortController();
  aborter = myAborter;
  let failure: string | null = null;
  let completed = false;
  try {
    const resp = await fetch(`/api/papers/${encodeURIComponent(chat.paperId)}/chat`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ model_id: chat.modelId, message: text }),
      signal: myAborter.signal,
    });
    if (!resp.ok || !resp.body) throw new Error(`request failed (${resp.status})`);
    await readSse(resp.body, (e) => {
      if (my !== session) return;
      if (e.event === 'delta') {
        chat.streaming = (chat.streaming ?? '') + (JSON.parse(e.data).text ?? '');
      } else if (e.event === 'error') {
        failure = String(JSON.parse(e.data).message ?? 'unknown error');
      } else if (e.event === 'done') {
        completed = true;
        const label = chat.models.find((m) => m.id === chat.modelId)?.label ?? null;
        const doneId: unknown = JSON.parse(e.data).id;
        chat.messages.push({
          id: localId--,
          role: 'user',
          content: text,
          model: null,
          created_at: '',
        });
        chat.messages.push({
          id: doneId == null ? localId-- : Number(doneId),
          role: 'assistant',
          content: chat.streaming ?? '',
          model: label,
          created_at: '',
        });
        chat.pending = null;
        chat.streaming = null;
      }
    });
    if (my !== session) return;
    if (failure) throw new Error(failure);
    if (!completed) throw new Error('the connection closed before the reply finished');
  } catch (err) {
    // Once the exchange folded on `done`, a trailing read rejection (e.g. a
    // Stop clicked in the post-done window) must not repopulate the draft or
    // report a spurious error over a successful exchange.
    if (my !== session || completed) return;
    const aborted = err instanceof DOMException && err.name === 'AbortError';
    chat.pending = null;
    chat.streaming = null;
    chat.draft = text; // give the message back for editing or resend
    chat.error = aborted
      ? null
      : `The model request failed: ${(err as Error).message} Send again to retry.`;
  } finally {
    if (my === session) chat.busy = false;
    if (aborter === myAborter) aborter = null; // never null a newer send's controller
  }
}

export function stopChatStream(): void {
  aborter?.abort();
}

export async function clearChatThread(): Promise<void> {
  if (!chat.paperId) return;
  try {
    const resp = await fetch(`/api/papers/${encodeURIComponent(chat.paperId)}/chat`, {
      method: 'DELETE',
    });
    if (!resp.ok) throw new Error(String(resp.status));
    chat.messages = [];
    chat.error = null;
  } catch {
    chat.error = 'Could not clear this conversation. Try again.';
  }
}
