# Design: Paper Chat — Floating Assistant in the Reader

**Date:** 2026-07-10
**Status:** Approved design, pending implementation plan

## Overview

A chat assistant for the paper being read. A floating bubble sits over the
PDF whenever a PDF tab is active — including zen mode, because it is a
fixed overlay rather than part of the hideable chrome. Clicking it (or
pressing `c`) expands a floating card where the user picks a model from a
config-defined list and discusses the open paper with it. The model
receives the paper's metadata and its full extracted text (capped).
Replies stream token-by-token; conversations persist in SQLite, one
thread per paper.

Builds on existing plumbing: the OpenAI-compatible chat client used by
the daily TL;DR, `pdf::extract_text_all`, the axum API, and the
frontend's Paper & Ink component/motion system.

## Goals

- Ask questions about the open paper and get streamed answers grounded in
  its full text.
- Choose among several configured models/providers per message.
- Keep per-paper chat history across restarts; clear it on demand.
- Work in zen mode.
- Degrade cleanly: with no chat configuration, no chat UI appears and
  nothing else breaks.

## Non-goals

- Tool use / agentic behavior (library search, reference fetching, web
  search) — the design leaves room, but v1 is plain chat.
- Markdown or math rendering of replies — v1 renders plain text
  (`whitespace-pre-wrap`) and instructs the model to answer in plain
  prose. A renderer is a possible follow-up.
- RAG / retrieval over chunks — full-text-in-prompt is the v1 context
  strategy.
- Multiple threads per paper, edit/regenerate of past turns, or sharing.
- Chat outside the reader (on the Library home) — the assistant is about
  the open paper.

## Decisions settled during brainstorming

- **Model list from config**: `[[chat.models]]` entries (label +
  OpenAI-compatible base_url + model id + key), not a single-provider
  list, not runtime `/models` discovery.
- **Context**: capped full text in the system prompt (like the daily
  TL;DR), not RAG, not metadata-only.
- **Persist + stream**: SQLite history and SSE streaming.
- **Plain chat** in v1; agent tools deferred.
- **Placement**: assistant-style floating bubble over the PDF expanding
  to a floating card (user's direction; a docked side panel was
  rejected because it would not survive zen mode's chrome hiding).

## Configuration

```toml
[[chat.models]]
label       = "GPT-5 Mini"                  # shown in the UI dropdown
base_url    = "https://api.openai.com/v1"   # OpenAI-compatible
model       = "gpt-5-mini"
api_key_env = "OPENAI_API_KEY"              # or inline api_key = "..."

[[chat.models]]
label    = "Local Qwen"
base_url = "http://localhost:11434/v1"
model    = "qwen3:32b"                      # keyless endpoints allowed
```

Rules, mirroring `[daily.llm]` conventions:

- Key resolution per entry: inline `api_key` wins; else the env var
  (`api_key_env`, default `OPENAI_API_KEY`). If the env var is unset or
  empty the entry is served **keyless** — requests carry no
  Authorization header. That is correct for local servers; a forgotten
  key for a hosted provider surfaces as a 401 in the chat's inline
  error, which names the model. Every configured entry is served.
- A model's **id** (referenced by the API) is its position in the config
  file rendered as a string ("0", "1", …); labels are display-only and
  need not be unique.
- No `[chat]` tables at all, or zero usable entries → chat is disabled.
- Optional `[chat] max_context_chars` (default 60_000) caps the paper
  text included in the prompt.

## Backend

### Shared LLM client (`src/llm.rs`)

Extract the OpenAI-compatible client out of `daily::tldr` into
`src/llm.rs`:

- `LlmClient { http, base_url, model, api_key }` with
  `from_parts(base_url, model, api_key)`.
- `complete(system, user) -> Result<String>` — current behavior, kept
  for the TL;DR (same retry/backoff on 429/5xx/network).
- `stream(messages) -> Result<impl Stream<Item = Result<String>>>` —
  POSTs `stream: true`, parses SSE `data:` lines, yields content deltas.
  No retry once streaming has begun; connection errors surface as a
  stream error item.
- `daily::tldr::ChatClient` becomes a thin alias/wrapper so daily code
  and tests keep working; its prompt logic does not move.

### Schema (one migration)

```sql
CREATE TABLE chat_messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  paper_id   TEXT NOT NULL REFERENCES papers(id),
  role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content    TEXT NOT NULL,
  model      TEXT,               -- model label, assistant rows only
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX chat_messages_paper ON chat_messages(paper_id, id);
```

Deleting a paper leaves rows orphaned-but-invisible (paper endpoints 404
first); the existing purge flow gains a `DELETE FROM chat_messages WHERE
paper_id = ?` alongside PDF removal.

### Endpoints

- `GET /api/chat/models` → `{ "available": bool, "models": [{ "id",
  "label" }] }`. Keys never leave the server.
- `GET /api/papers/:id/chat` → `[{ id, role, content, model,
  created_at }]` in insertion order. 404 for unknown/deleted papers.
- `DELETE /api/papers/:id/chat` → clears the thread. 204.
- `POST /api/papers/:id/chat` with `{ "model_id": "0", "message": "…" }`
  → `text/event-stream`:
  - `event: delta`, `data: {"text": "…"}` — repeated;
  - `event: done`, `data: {"id": <assistant row id>}` — terminal;
  - `event: error`, `data: {"message": "…"}` — terminal.
  Persistence is all-or-nothing: the user row and assistant row are
  inserted together (one transaction) only when the upstream stream
  completes. On a provider failure or client disconnect/abort, nothing
  is persisted — the stored thread only ever contains completed
  exchanges, so a retry never duplicates a user turn. 400 for unknown
  `model_id` or empty message; 503 when chat is disabled.

### Context builder

System prompt assembled per request (no history compaction in v1):

1. Fixed instruction: the assistant discusses the given paper with a
   researcher; answer in plain prose without markdown formatting; say so
   when the paper does not contain the answer.
2. Metadata block: title, authors, venue, year, abstract.
3. `--- PAPER TEXT (may be truncated) ---` + extracted text capped at
   `max_context_chars`.

Extraction uses `pdf::extract_text_all`, cached in a process-lifetime
`HashMap<PaperId, Arc<String>>` behind a mutex so repeat turns and
concurrent requests don't re-spawn extraction. Extraction failure does
not fail the chat: the prompt falls back to metadata + abstract with a
note that full text was unavailable.

Message array sent upstream: system + full stored thread (user/assistant
rows in order) + the new user message. v1 sends the whole thread; the
context cap applies to paper text only.

## Frontend

### State (`frontend/src/lib/chat.svelte.ts`)

- `chat` store: `{ open: boolean, models: {id,label}[], available:
  boolean, modelId: string | null, messages: ChatMessage[], streaming:
  string | null, busy: boolean, error: string | null }`.
- `modelId` persists in `localStorage("xuewen-chat-model")`, falling
  back to the first available model.
- `loadModels()` once at app start (drives bubble visibility);
  `loadThread(paperId)` when the panel opens or the active paper
  changes; `sendMessage(text)` POSTs and consumes the SSE body via
  `fetch` + ReadableStream parsing (EventSource cannot POST), appending
  deltas into `streaming`, then folding the finished turn into
  `messages`; `stopStreaming()` aborts the in-flight fetch via
  `AbortController`; `clearThread()`.
- A superseded-session guard (same pattern as `identifySession`) drops
  stream chunks that arrive after the panel switched papers.

### Components

- `ChatBubble.svelte` — fixed, bottom-right of the content area
  (`bottom-5 right-5`), amber circular FAB with a message icon,
  `aria-label="Chat about this paper"`. Rendered when
  `viewer.activeId !== null && chat.available && !chat.open`. Bubble and
  card use `z-[45]`: below modals/palette/toasts (50/60/70) so dialogs
  always cover the chat, and below ZenPill (50), which never overlaps it
  spatially (top-center vs bottom-right).
- `ChatPanel.svelte` — fixed card, `bottom-5 right-5`, `w-[400px]
  max-w-[calc(100vw-2.5rem)] h-[560px] max-h-[80vh]`, scale+fade in from
  the corner (`transform-origin: bottom right`, `dur(DUR.base)`).
  Header: model `<select>` (from `chat.models`), clear-thread button
  (inline confirm like project delete), minimize button. Transcript:
  user turns right-aligned on parchment, assistant turns plain with a
  small mono model-label caption, `whitespace-pre-wrap`; auto-scrolls to
  the newest message unless the user has scrolled up. Footer: textarea
  (`Enter` send, `Shift+Enter` newline), send button, stop button while
  streaming. Esc inside the panel collapses it (stopPropagation, same
  ownership pattern as the palette input).
- `Toaster.svelte` moves to bottom-**left** (`bottom-4 left-4`) to cede
  the corner. No test changes required (no position assertions).

### Shortcuts & Esc chain

- `c` toggles the chat card — only when a PDF tab is active, chat is
  available, and no modal/palette is open (standard single-key guards).
- Global Esc chain becomes: palette → **chat card** → zen. (The panel's
  own Esc handler covers focus-inside; the global branch covers focus
  elsewhere.)

### Reader integration

Bubble and card render inside the PDF area wrapper in `App.svelte`
(sibling of `PdfViewer`), so "bottom-right" is the content pane's
corner, not the InfoPanel's. Switching tabs while the card is open keeps
it open and swaps the thread; going home (`activeId === null`) hides
bubble and card.

## Error handling

- Chat disabled → `available: false`, no bubble, `c` does nothing.
- Provider/network failure or `error` event → inline error row in the
  transcript ("The model request failed: … Send again to retry."); the
  composed message stays in the input; never toast-only.
- Paper text extraction failure → chat still works on metadata (noted in
  the system prompt), invisible to the user.
- Stop button aborts the stream; the partial reply is discarded and the
  user's message returns to the input (nothing was persisted, so the
  transcript, the input, and the stored thread stay consistent).

## Testing

- **Rust**: config parsing (key resolution, omission warning, keyless
  local entries, disabled states); `chat_messages` store CRUD + clear +
  purge hook; context builder (cap, extraction-failure fallback); API
  tests — models list hides keys, history/clear, POST happy path with a
  wiremock SSE upstream (the daily tests establish the wiremock
  pattern), 400/404/503 paths.
- **Frontend**: chat store — loadModels/loadThread against stubbed
  fetch, sendMessage against a stubbed ReadableStream SSE body (delta →
  done, and error path), abort, superseded-session guard, localStorage
  model persistence; component tests — bubble visibility rules, panel
  open/minimize/clear-confirm, Esc collapse; shortcuts — `c` guards.
- All existing suites stay green; `svelte-check` and `cargo test` clean.
