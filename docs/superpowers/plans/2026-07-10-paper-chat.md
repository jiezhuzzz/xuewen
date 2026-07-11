# Paper Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A floating chat assistant over the open PDF — configurable OpenAI-compatible models, streamed replies grounded in the paper's full text, per-paper persistent threads — that works in zen mode and disappears cleanly when unconfigured.

**Architecture:** Backend: a shared OpenAI-compatible client (`src/llm.rs`, extracted from `daily::tldr`, gaining SSE streaming), a `src/chat/` module (message store + context builder + `ChatService`), one migration, and four endpoints (`GET /api/chat/models`, `GET|POST|DELETE /api/papers/{id}/chat`, POST streams SSE). Frontend: a `chat` rune store with fetch-based SSE parsing (`lib/sse.ts`), a `ChatBubble` FAB and `ChatPanel` floating card rendered inside the reader's PDF wrapper (overlay ⇒ zen-proof), `c` shortcut, Esc-chain integration.

**Tech Stack:** Rust (axum 0.8 `response::sse`, sqlx/SQLite, reqwest + `stream` feature, `async-stream`, `futures-util`, wiremock/axum-test for tests); Svelte 5 runes + Tailwind 4 + existing motion tokens; vitest.

**Spec:** `docs/superpowers/specs/2026-07-10-paper-chat-design.md`

**Environment:** direnv loads the flake dev shell (`$IN_NIX_SHELL` set) — `cargo`/`npm` work directly; if a tool is missing run via `nix develop -c <command>`. Commit with `git -c commit.gpgsign=false commit -m "..."`. Conventional Commits, types feat/fix/docs/chore/ci; scopes: `feat(chat)` for backend tasks, `feat(frontend)` for UI tasks. Keep Rust rustfmt-clean (`cargo fmt` before each commit). npm commands run from repo root with `--prefix frontend`.

## Global Constraints

- Backend dependency additions are exactly: the `"stream"` feature on the existing reqwest dependency, `futures-util = "0.3"`, `async-stream = "0.3"`. Nothing else.
- **Zero new npm dependencies.**
- API keys never appear in any HTTP response. `GET /api/chat/models` returns only `{id, label}` pairs.
- Persistence is all-or-nothing per exchange: user + assistant rows insert in one transaction only after the upstream stream completes; aborts/failures persist nothing.
- Chat disabled (no `[[chat.models]]`) ⇒ `GET /api/chat/models` → `{"available": false, "models": []}`, `POST …/chat` → 503, no chat UI renders, `c` does nothing.
- Model `id` = the entry's position in the config file, as a string (`"0"`, `"1"`, …). Every configured entry is served; keyless entries send no Authorization header.
- Frontend: every animation duration flows through `dur()` from `lib/motion.ts`; amber (`amber-700` light / `amber-600`+`amber-500` hover dark) is the only action accent; cinnabar stays in exactly its two existing placements (the chat bubble is **amber**, not cinnabar).
- The 問 glyph appears in exactly one place: the chat bubble.
- Copy rules: sentence case, active voice, errors say what went wrong and what to do next. Exact strings in the Design Foundation table below are normative.
- After every task: `cargo test` (backend tasks) and `npm --prefix frontend run test` + `npm --prefix frontend run check` (frontend tasks) pass; existing suites stay green.

---

## Design Foundation (frontend-design pass)

**Subject:** the reader's assistant in a personal paper library. Its one job: answer questions about the paper on screen without pulling the reader out of it.

**Palette:** no new colors. Bubble + send button = amber action recipe (`bg-amber-700 hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500`); card surfaces = `bg-paper`/`dark:bg-soot` with `border-stone-200/800` hairlines; user turns on `bg-parchment`/`dark:bg-stone-800`; errors red family.

**Type roles:** assistant replies in **Source Serif** (`font-serif text-[15px] leading-relaxed`) — the paper answers in the same voice its abstract is set in (DetailView precedent); user turns in Inter `text-sm`; the model attribution under each assistant turn in `font-mono text-[10px] uppercase tracking-wide text-stone-400`.

**Signature:** the bubble glyph is **問** (wèn, "to ask") in paper-white serif on the amber disc — the counterpart of the wordmark's cinnabar 學 (xué, "to learn"): *Xuewen* 學問 is literally "learning-and-asking". The library stamps what you've learned; the bubble is where you ask. One glyph, one placement, amber because it is an action. Everything else stays quiet.

**Self-critique vs. the generic default:** the template AI-chat is a blue/violet gradient FAB with a sparkles icon and markdown bubbles. This design spends its distinctiveness on the 問/學 pairing and the serif "paper's voice" for replies; no gradients, no sparkles, no new accent.

**Normative copy:**

| Where | String |
|---|---|
| Bubble aria-label/title | `Chat about this paper` / `Chat about this paper (c)` |
| Panel heading (aria-label) | `Paper chat` |
| Empty transcript | `Ask about the methods, the results, or how this paper connects to what you already know.` |
| Input placeholder | `Ask about this paper…` |
| Send / Stop buttons | `Send` / `Stop` |
| Clear flow | button aria `Clear conversation` → inline `Clear this conversation?` + `Clear` / `Cancel` |
| Request failure (inline) | `The model request failed: {reason} Send again to retry.` |
| History load failure | `Could not load this conversation. Close and reopen the chat to retry.` |
| Model picker aria-label | `Model` |

---

## File Structure

```
Cargo.toml                     modify: reqwest "stream" feature; + futures-util, async-stream
src/
  lib.rs                       modify: `pub mod chat; pub mod llm;`
  config.rs                    modify: ChatConfig, ChatModelConfig (+ tests)
  llm.rs                       NEW: LlmClient { complete, stream }, ChatMessage (client moved from daily/tldr.rs)
  daily/tldr.rs                modify: ChatClient becomes a thin wrapper over llm::LlmClient
  chat/mod.rs                  NEW: ChatService (models + text cache), submodule decls
  chat/store.rs                NEW: chat_messages CRUD (list / insert_exchange / clear)
  chat/context.rs              NEW: system_prompt()
  web/mod.rs                   modify: AppState.chat, builders, routes, serve() param
  web/chat.rs                  NEW: models/history/clear/send handlers (send = SSE)
  main.rs                      modify: serve arm wires ChatService; purge arm clears chat rows
migrations/0010_add_chat.sql   NEW
tests/web_chat_test.rs         NEW: endpoint tests (wiremock upstream)
frontend/src/
  lib/sse.ts                   NEW: readSse() parser        + lib/sse.test.ts
  lib/chat.svelte.ts           NEW: chat store              + lib/chat.test.ts
  components/ChatBubble.svelte NEW
  components/ChatPanel.svelte  NEW                          + components/ChatPanel.test.ts
  components/Toaster.svelte    modify: bottom-right → bottom-left
  App.svelte                   modify: bubble/panel in the PDF wrapper, loadChatModels, thread effect
  lib/shortcuts.ts             modify: `c` + Esc chain      + extend lib/shortcuts.test.ts
```

---

## Task 1: Dependencies and `[chat]` configuration

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`

**Interfaces:**
- Produces: `ChatModelConfig { label: String, base_url: String, model: String, api_key: Option<String>, api_key_env: String }` with `resolve_key(&self) -> Option<String>`; `ChatConfig { models: Vec<ChatModelConfig>, max_context_chars: usize }` (default 60_000); `Config.chat: ChatConfig` (serde default). Reuses the existing `default_embed_base_url` / `default_api_key_env` helpers in config.rs.

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml`, extend reqwest's feature list and add two crates next to it:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "multipart", "json", "stream"] }
futures-util = "0.3"
async-stream = "0.3"
```

Run: `cargo build` — expected: compiles (no code uses them yet).

- [ ] **Step 2: Write the failing config tests**

Append to the `mod tests` in `src/config.rs` (mirror the existing `[daily.llm]` test's minimal-config preamble — the three required top-level keys):

```rust
    #[test]
    fn chat_config_parses_models_with_defaults() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"

[[chat.models]]
label = "GPT-5 Mini"
model = "gpt-5-mini"

[[chat.models]]
label    = "Local Qwen"
base_url = "http://localhost:11434/v1"
model    = "qwen3:32b"
"#,
        )
        .unwrap();
        assert_eq!(cfg.chat.models.len(), 2);
        assert_eq!(cfg.chat.models[0].base_url, "https://api.openai.com/v1");
        assert_eq!(cfg.chat.models[0].api_key_env, "OPENAI_API_KEY");
        assert_eq!(cfg.chat.models[1].model, "qwen3:32b");
        assert_eq!(cfg.chat.max_context_chars, 60_000);
    }

    #[test]
    fn chat_config_absent_means_disabled() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"
"#,
        )
        .unwrap();
        assert!(cfg.chat.models.is_empty());
        assert_eq!(cfg.chat.max_context_chars, 60_000);
    }

    #[test]
    fn chat_model_key_resolution() {
        let m = ChatModelConfig {
            label: "x".into(),
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: Some("sk-inline".into()),
            api_key_env: "XUEWEN_TEST_UNSET_ENV".into(),
        };
        assert_eq!(m.resolve_key().as_deref(), Some("sk-inline"));

        let keyless = ChatModelConfig { api_key: None, ..m };
        // Env var unset -> keyless entry (requests carry no Authorization).
        assert_eq!(keyless.resolve_key(), None);
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib config` — expected: FAIL, `ChatModelConfig`/`chat` not found.

- [ ] **Step 4: Implement in `src/config.rs`**

Add after `DailyLlmConfig` (reusing its default helpers):

```rust
/// One selectable chat model (`[[chat.models]]`): an OpenAI-compatible
/// chat-completions endpoint. Its API id is its position in the config file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChatModelConfig {
    /// Shown in the UI dropdown; display-only, need not be unique.
    pub label: String,
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    pub model: String,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

impl ChatModelConfig {
    /// Inline key wins; else the env var. Unset/empty -> None: the entry is
    /// served keyless (no Authorization header) — right for local servers,
    /// and a forgotten hosted key surfaces as a 401 in the chat's inline
    /// error rather than silently hiding the model.
    pub fn resolve_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var(&self.api_key_env).ok())
            .filter(|k| !k.trim().is_empty())
    }
}

/// Paper-chat settings (`[chat]`). No models = feature disabled.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChatConfig {
    #[serde(default)]
    pub models: Vec<ChatModelConfig>,
    /// Chars of extracted paper text included in the system prompt.
    #[serde(default = "default_chat_max_context_chars")]
    pub max_context_chars: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            max_context_chars: default_chat_max_context_chars(),
        }
    }
}

fn default_chat_max_context_chars() -> usize {
    60_000
}
```

And add the field to `Config` (next to `daily`):

```rust
    #[serde(default)]
    pub chat: ChatConfig,
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib config` — expected: PASS (new tests + all existing config tests).

- [ ] **Step 6: Format and commit**

```bash
cargo fmt
git add Cargo.toml Cargo.lock src/config.rs
git -c commit.gpgsign=false commit -m "feat(chat): [[chat.models]] configuration and streaming-capable dependencies"
```

---

## Task 2: Shared LLM client with streaming (`src/llm.rs`)

**Files:**
- Create: `src/llm.rs`
- Modify: `src/lib.rs` (add `pub mod llm;`)
- Modify: `src/daily/tldr.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `llm::ChatMessage { role: &'static str, content: String }` (serde Serialize); `llm::LlmClient::new(base_url: &str, model: &str, api_key: Option<String>) -> LlmClient`; `complete(&self, system: &str, user: &str) -> Result<String>` (identical behavior/retries to today's `daily::tldr::ChatClient::complete`); `stream(&self, messages: &[ChatMessage]) -> Result<impl Stream<Item = Result<String>> + Send>` yielding content deltas. `daily::tldr::ChatClient` keeps its exact public surface (`from_config`, `for_tests`, `complete`) as a thin wrapper.

- [ ] **Step 1: Write the failing streaming tests**

Create `src/llm.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn stream_yields_deltas_until_done() {
        let server = MockServer::start().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                    data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        let stream = client
            .stream(&[ChatMessage { role: "user", content: "hi".into() }])
            .await
            .unwrap();
        futures_util::pin_mut!(stream);
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            out.push_str(&item.unwrap());
        }
        assert_eq!(out, "Hello");
    }

    #[tokio::test]
    async fn stream_surfaces_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        let err = client
            .stream(&[ChatMessage { role: "user", content: "hi".into() }])
            .await
            .err()
            .expect("401 must fail");
        assert!(err.to_string().contains("401"), "got: {err}");
    }
}
```

Add `pub mod llm;` to `src/lib.rs` (alphabetical with the other modules).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib llm` — expected: FAIL to compile (`LlmClient` undefined).

- [ ] **Step 3: Implement the client**

Top of `src/llm.rs` (above the test module):

```rust
//! Minimal OpenAI-compatible chat-completions client. One client, two
//! callers: the daily TL;DR uses blocking `complete` (with retries); the
//! paper chat uses SSE `stream` (no retry once streaming has begun).

use anyhow::{anyhow, Result};
use futures_util::{Stream, StreamExt};
use std::time::Duration;

const ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: &'static str, // "system" | "user" | "assistant"
    pub content: String,
}

pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl LlmClient {
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("building chat HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        }
    }

    fn request(&self, body: &serde_json::Value) -> reqwest::RequestBuilder {
        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// Blocking completion with retries — behavior moved verbatim from
    /// `daily::tldr::ChatClient::complete`.
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        // MOVE the entire body of the current `daily::tldr::ChatClient::complete`
        // (src/daily/tldr.rs, the `pub async fn complete` from its opening brace
        // to its close) here unchanged, with two mechanical adaptations:
        //   1. build the request via `self.request(&body)` instead of the
        //      inline `self.http.post(url)…bearer_auth` chain;
        //   2. field references stay `self.model` / `self.base_url` as before.
        // The retry loop (ATTEMPTS, backoff on 429/5xx/network) moves as-is;
        // ATTEMPTS is defined above.
        unimplemented!() // replaced by the moved body in this same step
    }

    /// Stream assistant deltas from a `stream: true` completion. The HTTP
    /// error (non-2xx) is returned from this call; mid-stream failures come
    /// through as an `Err` item. Per-request timeout is longer than the
    /// client default because generation time counts against it.
    pub async fn stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<impl Stream<Item = Result<String>> + Send> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
        let resp = self
            .request(&body)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| anyhow!("chat request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("chat completions failed: {status}: {text}"));
        }
        let mut bytes = resp.bytes_stream();
        Ok(async_stream::try_stream! {
            let mut buf: Vec<u8> = Vec::new();
            'read: while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(|e| anyhow!("stream read failed: {e}"))?;
                buf.extend_from_slice(&chunk);
                // SSE events end with a blank line.
                while let Some(pos) = find_double_newline(&buf) {
                    let event: Vec<u8> = buf.drain(..pos + 2).collect();
                    let event = String::from_utf8_lossy(&event).into_owned();
                    for line in event.lines() {
                        let Some(data) = line.strip_prefix("data:") else { continue };
                        let data = data.trim_start();
                        if data == "[DONE]" {
                            break 'read;
                        }
                        let v: serde_json::Value = serde_json::from_str(data)
                            .map_err(|e| anyhow!("bad stream payload: {e}"))?;
                        if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
                            if !s.is_empty() {
                                yield s.to_string();
                            }
                        }
                    }
                }
            }
        })
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}
```

**In this same step** replace the `unimplemented!()` by actually moving the retry-loop body from `src/daily/tldr.rs::ChatClient::complete` as the comment describes. A leftover `unimplemented!()` is a task failure.

- [ ] **Step 4: Rewire `src/daily/tldr.rs`**

Replace the `ChatClient` struct and impl (keep `Summary`, the prompt constants, and everything else untouched):

```rust
/// Chat client for the daily TL;DR — a thin wrapper that keeps this module's
/// config-driven construction while the HTTP logic lives in `crate::llm`.
pub struct ChatClient {
    inner: crate::llm::LlmClient,
}

impl ChatClient {
    /// `None` when no API key is resolvable — the daily feature is then
    /// disabled, but nothing fails.
    pub fn from_config(cfg: &DailyLlmConfig) -> Option<Self> {
        let key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var(&cfg.api_key_env).ok())
            .filter(|k| !k.trim().is_empty());
        let Some(key) = key else {
            tracing::warn!(
                "[daily.llm] configured but no API key (set api_key or ${}) — daily papers disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self {
            inner: crate::llm::LlmClient::new(&cfg.base_url, &cfg.model, Some(key)),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str) -> Self {
        Self {
            inner: crate::llm::LlmClient::new(base_url, model, None),
        }
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.inner.complete(system, user).await
    }
}
```

Remove the now-unused direct `reqwest`/`Duration` imports from tldr.rs if nothing else in the file uses them (`cargo build` will tell you).

- [ ] **Step 5: Run the full backend suite**

Run: `cargo test` — expected: PASS, including all existing daily/TL;DR tests (they exercise `ChatClient::for_tests` + `complete` against wiremock and must be untouched).

- [ ] **Step 6: Format and commit**

```bash
cargo fmt
git add src/llm.rs src/lib.rs src/daily/tldr.rs
git -c commit.gpgsign=false commit -m "feat(chat): shared OpenAI-compatible LLM client with SSE streaming"
```

---

## Task 3: Migration and chat-message store

**Files:**
- Create: `migrations/0010_add_chat.sql`
- Create: `src/chat/mod.rs` (module skeleton), `src/chat/store.rs`
- Modify: `src/lib.rs` (add `pub mod chat;`), `src/main.rs` (purge arm)

**Interfaces:**
- Produces: `chat::store::ChatMessageRow { id: i64, role: String, content: String, model: Option<String>, created_at: String }` (serde Serialize); `list(pool, paper_id) -> Result<Vec<ChatMessageRow>>`; `insert_exchange(pool, paper_id, user_content, assistant_content, model_label) -> Result<i64>` (transactional, returns assistant row id); `clear(pool, paper_id) -> Result<()>`.

- [ ] **Step 1: Write the migration**

`migrations/0010_add_chat.sql`:

```sql
-- Per-paper chat threads: one thread per paper, insertion-ordered.
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

- [ ] **Step 2: Write the failing store tests**

`src/chat/mod.rs`:

```rust
//! Paper chat: per-paper LLM conversations grounded in the paper's text.

pub mod store;
```

`src/chat/store.rs` — start with the tests. For the seed row, mirror the paper-insert used by the existing store tests in `src/db.rs` (see e.g. the `soft_delete_hides_and_purge_removes` test's setup); the helper below shows the shape — adjust the column list to match what those tests actually insert if it differs:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn pool_with_paper(id: &str) -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        // Minimal parent row for the FK; mirror src/db.rs test seeding.
        sqlx::query(
            "INSERT INTO papers (id, content_hash, rel_path, added_at, status)
             VALUES (?, 'hash', 'p.pdf', datetime('now'), 'resolved')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn exchange_roundtrip_in_order() {
        let pool = pool_with_paper("p1").await;
        let aid = insert_exchange(&pool, "p1", "what is this?", "a paper.", "GPT-5 Mini")
            .await
            .unwrap();
        assert!(aid > 0);
        insert_exchange(&pool, "p1", "and the method?", "transformers.", "Local Qwen")
            .await
            .unwrap();

        let rows = list(&pool, "p1").await.unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[0].content, "what is this?");
        assert_eq!(rows[0].model, None);
        assert_eq!(rows[1].role, "assistant");
        assert_eq!(rows[1].model.as_deref(), Some("GPT-5 Mini"));
        assert_eq!(rows[3].model.as_deref(), Some("Local Qwen"));
    }

    #[tokio::test]
    async fn clear_empties_one_thread_only() {
        let pool = pool_with_paper("p1").await;
        sqlx::query(
            "INSERT INTO papers (id, content_hash, rel_path, added_at, status)
             VALUES ('p2', 'hash2', 'q.pdf', datetime('now'), 'resolved')",
        )
        .execute(&pool)
        .await
        .unwrap();
        insert_exchange(&pool, "p1", "q", "a", "M").await.unwrap();
        insert_exchange(&pool, "p2", "q", "a", "M").await.unwrap();

        clear(&pool, "p1").await.unwrap();
        assert!(list(&pool, "p1").await.unwrap().is_empty());
        assert_eq!(list(&pool, "p2").await.unwrap().len(), 2);
    }
}
```

Add `pub mod chat;` to `src/lib.rs`.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib chat::store` — expected: FAIL to compile (functions undefined).

- [ ] **Step 4: Implement the store**

Top of `src/chat/store.rs`:

```rust
//! SQLite persistence for chat threads. Writes are all-or-nothing per
//! exchange: nothing is stored for aborted or failed generations, so the
//! thread only ever contains completed exchanges.

use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ChatMessageRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: String,
}

pub async fn list(pool: &SqlitePool, paper_id: &str) -> Result<Vec<ChatMessageRow>> {
    Ok(sqlx::query_as::<_, ChatMessageRow>(
        "SELECT id, role, content, model, created_at
         FROM chat_messages WHERE paper_id = ? ORDER BY id",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?)
}

/// Persist one completed exchange atomically; returns the assistant row id.
pub async fn insert_exchange(
    pool: &SqlitePool,
    paper_id: &str,
    user_content: &str,
    assistant_content: &str,
    model_label: &str,
) -> Result<i64> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO chat_messages (paper_id, role, content) VALUES (?, 'user', ?)")
        .bind(paper_id)
        .bind(user_content)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query(
        "INSERT INTO chat_messages (paper_id, role, content, model) VALUES (?, 'assistant', ?, ?)",
    )
    .bind(paper_id)
    .bind(assistant_content)
    .bind(model_label)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(res.last_insert_rowid())
}

pub async fn clear(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chat_messages WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 5: Hook the purge flow**

In `src/main.rs`, `Command::Purge` arm — inside the `for p in &targets` loop, **before** `db::delete_row(&pool, &p.id).await?;` (chat rows reference the paper row):

```rust
                    xuewen::chat::store::clear(&pool, &p.id).await?;
```

- [ ] **Step 6: Run and verify pass**

Run: `cargo test` — expected: PASS (store tests + everything pre-existing; migration applies cleanly in every test that runs `sqlx::migrate!`).

- [ ] **Step 7: Format and commit**

```bash
cargo fmt
git add migrations/0010_add_chat.sql src/chat src/lib.rs src/main.rs
git -c commit.gpgsign=false commit -m "feat(chat): chat_messages store with transactional exchanges"
```

---

## Task 4: ChatService and the context builder

**Files:**
- Modify: `src/chat/mod.rs`
- Create: `src/chat/context.rs`

**Interfaces:**
- Consumes: `config::{ChatConfig, ChatModelConfig}`, `models::Paper` (fields: `id`, `rel_path`, `meta.title: Option<String>`, `meta.authors: Authors(pub Vec<String>)`, `meta.venue: Option<String>`, `meta.year: Option<i64>`, `meta.abstract_text: Option<String>`), `pdf::extract_text_all(&Path) -> Result<String>`.
- Produces: `chat::ChatService { pub models: Vec<ChatModelConfig>, pub max_context_chars: usize }` with `from_config(&ChatConfig) -> Option<Arc<ChatService>>` and `paper_text(&self, library_root: &Path, paper: &Paper) -> Option<Arc<String>>` (cached); `chat::context::system_prompt(paper: &Paper, full_text: Option<&str>, cap: usize) -> String`.

- [ ] **Step 1: Write the failing context tests**

`src/chat/context.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    fn paper() -> Paper {
        Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p.pdf".into(),
            cite_key: Some("smith2024".into()),
            added_at: "2026-01-01".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("A Great Paper".into()),
                abstract_text: Some("We do things.".into()),
                authors: Authors(vec!["A. Smith".into(), "B. Jones".into()]),
                venue: Some("NeurIPS".into()),
                year: Some(2024),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    #[test]
    fn prompt_includes_metadata_and_capped_text() {
        let text = "x".repeat(100);
        let p = system_prompt(&paper(), Some(&text), 10);
        assert!(p.contains("Title: A Great Paper"));
        assert!(p.contains("Authors: A. Smith, B. Jones"));
        assert!(p.contains("Venue: NeurIPS"));
        assert!(p.contains("Abstract: We do things."));
        assert!(p.contains("PAPER TEXT (truncated)"));
        assert!(p.contains(&"x".repeat(10)));
        assert!(!p.contains(&"x".repeat(11)), "cap must apply");
        assert!(p.contains("plain prose"), "markdown-free instruction");
    }

    #[test]
    fn prompt_notes_missing_text() {
        let p = system_prompt(&paper(), None, 10);
        assert!(p.contains("full text was unavailable"));
        assert!(!p.contains("PAPER TEXT"));
    }
}
```

(If `PaperStatus`'s variant is named differently, use whatever `src/models.rs` defines for the resolved state — check the enum.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chat::context` — expected: FAIL to compile.

- [ ] **Step 3: Implement the context builder**

Top of `src/chat/context.rs`:

```rust
//! Builds the system prompt: instructions + metadata + capped paper text.

use crate::models::Paper;

pub fn system_prompt(paper: &Paper, full_text: Option<&str>, cap: usize) -> String {
    let mut s = String::from(
        "You are a research assistant discussing one specific paper with a researcher.\n\
         Answer from the paper's content; when the paper does not contain the answer, say so plainly.\n\
         Answer in plain prose without markdown formatting.\n\n--- PAPER METADATA ---\n",
    );
    let m = &paper.meta;
    s.push_str(&format!(
        "Title: {}\n",
        m.title.as_deref().unwrap_or("(untitled)")
    ));
    if !m.authors.0.is_empty() {
        s.push_str(&format!("Authors: {}\n", m.authors.0.join(", ")));
    }
    if let Some(v) = &m.venue {
        s.push_str(&format!("Venue: {v}\n"));
    }
    if let Some(y) = m.year {
        s.push_str(&format!("Year: {y}\n"));
    }
    if let Some(a) = &m.abstract_text {
        s.push_str(&format!("Abstract: {a}\n"));
    }
    match full_text {
        Some(t) => {
            let clipped: String = t.chars().take(cap).collect();
            let marker = if t.chars().count() > cap { " (truncated)" } else { "" };
            s.push_str(&format!("\n--- PAPER TEXT{marker} ---\n{clipped}\n"));
        }
        None => s.push_str(
            "\n(The paper's full text was unavailable; only the metadata above is known.)\n",
        ),
    }
    s
}
```

- [ ] **Step 4: Implement `ChatService` in `src/chat/mod.rs`**

```rust
//! Paper chat: per-paper LLM conversations grounded in the paper's text.

pub mod context;
pub mod store;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::{ChatConfig, ChatModelConfig};
use crate::models::Paper;

/// The configured chat feature: the model list plus a process-lifetime cache
/// of extracted paper text (extraction spawns pdftotext; repeat turns and
/// concurrent requests must not re-run it).
pub struct ChatService {
    pub models: Vec<ChatModelConfig>,
    pub max_context_chars: usize,
    text_cache: Mutex<HashMap<String, Arc<String>>>,
}

impl ChatService {
    /// `None` when no models are configured — chat is then disabled and the
    /// API answers 503 / `available: false`.
    pub fn from_config(cfg: &ChatConfig) -> Option<Arc<Self>> {
        if cfg.models.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            models: cfg.models.clone(),
            max_context_chars: cfg.max_context_chars,
            text_cache: Mutex::new(HashMap::new()),
        }))
    }

    /// The paper's extracted full text, cached. `None` when extraction fails
    /// — the chat then runs on metadata alone (see `context::system_prompt`).
    pub async fn paper_text(&self, library_root: &Path, paper: &Paper) -> Option<Arc<String>> {
        if let Some(t) = self.text_cache.lock().await.get(&paper.id) {
            return Some(t.clone());
        }
        let path = library_root.join(&paper.rel_path);
        let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&path))
            .await
            .ok()?
            .ok()?;
        let text = Arc::new(text);
        self.text_cache
            .lock()
            .await
            .insert(paper.id.clone(), text.clone());
        Some(text)
    }
}
```

- [ ] **Step 5: Run and verify pass**

Run: `cargo test --lib chat` — expected: PASS (context + store tests).

- [ ] **Step 6: Format and commit**

```bash
cargo fmt
git add src/chat
git -c commit.gpgsign=false commit -m "feat(chat): ChatService with cached extraction and context builder"
```

---

## Task 5: Chat API endpoints (models, history, clear, SSE send)

**Files:**
- Create: `src/web/chat.rs`
- Modify: `src/web/mod.rs` (AppState field, builders, routes, `serve()` signature)
- Modify: `src/main.rs` (serve arm)
- Test: `tests/web_chat_test.rs`

**Interfaces:**
- Consumes: Task 2 `llm::{LlmClient, ChatMessage}`, Task 3 `chat::store`, Task 4 `chat::{ChatService, context}`; existing `db::get_by_id`, the 404/500 response helpers in `src/web/api.rs` (reuse them — check their names in that file, e.g. `not_found()`), and the existing 503 pattern used by the daily endpoints.
- Produces: routes `GET /api/chat/models`, `GET|POST|DELETE /api/papers/{id}/chat` (use the same path-param syntax as the existing `/api/papers/{id}` routes in `router_with`); `AppState.chat: Option<Arc<crate::chat::ChatService>>`; `pub fn build_router_with_chat(pool, library_root, chat: Arc<ChatService>) -> Router` for tests; `web::serve(...)` gains a trailing `chat: Option<Arc<ChatService>>` parameter.

- [ ] **Step 1: Write the failing endpoint tests**

`tests/web_chat_test.rs` — reuse the seeding/server helpers this repo's `tests/web_test.rs` uses (`tests/common`); the sketch below shows intent with a plain axum-test server; adapt the seed call to the existing helper that inserts a paper row (the same one `web_test.rs` uses), and point the seeded paper's `rel_path` at a nonexistent file so extraction falls back to metadata:

```rust
mod common;

use axum_test::TestServer;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::config::{ChatConfig, ChatModelConfig};

fn chat_cfg(base_url: &str) -> ChatConfig {
    ChatConfig {
        models: vec![
            ChatModelConfig {
                label: "Mock Model".into(),
                base_url: base_url.into(),
                model: "mock-1".into(),
                api_key: None,
                api_key_env: "XUEWEN_TEST_UNSET".into(),
            },
        ],
        max_context_chars: 60_000,
    }
}

#[tokio::test]
async fn models_report_unavailable_without_config() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await; // reuse/adapt the web_test.rs helper
    let server = TestServer::new(xuewen::web::build_router(pool, root)).unwrap();
    let resp = server.get("/api/chat/models").await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["available"], false);
}

#[tokio::test]
async fn models_list_labels_but_never_keys() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server =
        TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();
    let resp = server.get("/api/chat/models").await;
    let v: serde_json::Value = resp.json();
    assert_eq!(v["available"], true);
    assert_eq!(v["models"][0]["id"], "0");
    assert_eq!(v["models"][0]["label"], "Mock Model");
    let raw = resp.text();
    assert!(!raw.contains("base_url") && !raw.contains("api_key"), "no provider details leak");
}

#[tokio::test]
async fn send_streams_deltas_and_persists_the_exchange() {
    let upstream = MockServer::start().await;
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
               data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
               data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;

    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg(&upstream.uri())).unwrap();
    let server =
        TestServer::new(xuewen::web::build_router_with_chat(pool.clone(), root, chat)).unwrap();

    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "what is this?"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("event: delta"), "body: {body}");
    assert!(body.contains("Hel"));
    assert!(body.contains("event: done"));

    let rows = xuewen::chat::store::list(&pool, "p1").await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role, "user");
    assert_eq!(rows[1].content, "Hello");
    assert_eq!(rows[1].model.as_deref(), Some("Mock Model"));
}

#[tokio::test]
async fn send_validates_model_message_paper_and_config() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;

    // 503 when chat is unconfigured.
    let plain = TestServer::new(xuewen::web::build_router(pool.clone(), root.clone())).unwrap();
    plain
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server =
        TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();
    // 400: unknown model id; 400: empty message; 404: unknown paper.
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "9", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "   "}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .post("/api/papers/nope/chat")
        .json(&json!({"model_id": "0", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_roundtrip_and_clear() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    xuewen::chat::store::insert_exchange(&pool, "p1", "q", "a", "M")
        .await
        .unwrap();
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server =
        TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();

    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 2);

    server
        .delete("/api/papers/p1/chat")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 0);

    server
        .get("/api/papers/nope/chat")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
```

If `tests/common` has no ready paper-seeding helper returning `(pool, library_root)`, add one there (`pool_and_root_with_paper`) that follows `web_test.rs`'s existing seeding (in-memory pool + `sqlx::migrate!` + one paper row + a temp dir as library root).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test web_chat_test` — expected: FAIL to compile (`build_router_with_chat`, `web::chat` missing).

- [ ] **Step 3: Implement `src/web/chat.rs`**

```rust
//! Chat endpoints. POST streams SSE: `delta` events, then `done` (or
//! `error`). Persistence is all-or-nothing after the stream completes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde_json::json;

use super::AppState;
use crate::chat::{context, store};
use crate::db;
use crate::llm::{ChatMessage, LlmClient};

#[derive(serde::Deserialize)]
pub struct ChatRequest {
    pub model_id: String,
    pub message: String,
}

fn sse_event(name: &str, data: serde_json::Value) -> Result<Event, std::convert::Infallible> {
    Ok(Event::default().event(name).data(data.to_string()))
}

pub async fn models(State(app): State<AppState>) -> Response {
    match &app.chat {
        None => Json(json!({ "available": false, "models": [] })).into_response(),
        Some(c) => {
            let models: Vec<_> = c
                .models
                .iter()
                .enumerate()
                .map(|(i, m)| json!({ "id": i.to_string(), "label": m.label }))
                .collect();
            Json(json!({ "available": true, "models": models })).into_response()
        }
    }
}

/// Look up a live (non-deleted) paper or answer 404/500. Mirrors the guard
/// pattern of the other paper endpoints in api.rs.
async fn live_paper(app: &AppState, id: &str) -> Result<crate::models::Paper, Response> {
    match db::get_by_id(&app.pool, id).await {
        Ok(Some(p)) if p.deleted_at.is_none() => Ok(p),
        Ok(_) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(e) => {
            tracing::error!("chat paper lookup: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub async fn history(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match store::list(&app.pool, &paper.id).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => {
            tracing::error!("chat history: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn clear(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match store::clear(&app.pool, &paper.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("chat clear: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn send(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let Some(chat) = app.chat.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"code": 503, "message": "chat is not configured"}})),
        )
            .into_response();
    };
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let model = req
        .model_id
        .parse::<usize>()
        .ok()
        .and_then(|i| chat.models.get(i).cloned());
    let Some(model) = model else {
        return (StatusCode::BAD_REQUEST, "unknown model_id").into_response();
    };
    let user_msg = req.message.trim().to_string();
    if user_msg.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty message").into_response();
    }
    let history = match store::list(&app.pool, &paper.id).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("chat history for send: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let text = chat.paper_text(&app.library_root, &paper).await;
    let system = context::system_prompt(
        &paper,
        text.as_deref().map(|s| s.as_str()),
        chat.max_context_chars,
    );
    let mut messages = vec![ChatMessage { role: "system", content: system }];
    for row in &history {
        messages.push(ChatMessage {
            role: if row.role == "user" { "user" } else { "assistant" },
            content: row.content.clone(),
        });
    }
    messages.push(ChatMessage { role: "user", content: user_msg.clone() });

    let client = LlmClient::new(&model.base_url, &model.model, model.resolve_key());
    let (pool, paper_id, label) = (app.pool.clone(), paper.id.clone(), model.label.clone());

    let stream = async_stream::stream! {
        let upstream = match client.stream(&messages).await {
            Ok(s) => s,
            Err(e) => {
                yield sse_event("error", json!({ "message": e.to_string() }));
                return;
            }
        };
        futures_util::pin_mut!(upstream);
        let mut full = String::new();
        while let Some(item) = upstream.next().await {
            match item {
                Ok(delta) => {
                    full.push_str(&delta);
                    yield sse_event("delta", json!({ "text": delta }));
                }
                Err(e) => {
                    yield sse_event("error", json!({ "message": e.to_string() }));
                    return;
                }
            }
        }
        // Client disconnects drop this stream before we get here, so
        // nothing is persisted for aborted generations.
        match store::insert_exchange(&pool, &paper_id, &user_msg, &full, &label).await {
            Ok(assistant_id) => yield sse_event("done", json!({ "id": assistant_id })),
            Err(e) => yield sse_event("error", json!({ "message": format!("saving the exchange failed: {e}") })),
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}
```

- [ ] **Step 4: Wire state, routes, builders, serve**

In `src/web/mod.rs`:
1. `mod chat;` next to `mod api;`.
2. `AppState` gains:
   ```rust
       /// Present when paper chat is configured (`serve`). `None` -> chat
       /// endpoints answer 503 / available:false.
       pub chat: Option<Arc<crate::chat::ChatService>>,
   ```
3. Every existing `AppState { ... }` literal in this file gains `chat: None,`.
4. New test builder (after `build_router_with_daily`):
   ```rust
   /// Read-only router plus a configured chat service. Used by tests.
   pub fn build_router_with_chat(
       pool: SqlitePool,
       library_root: PathBuf,
       chat: Arc<crate::chat::ChatService>,
   ) -> Router {
       router_with(AppState {
           pool,
           library_root,
           ingest: None,
           proxy_login_url: None,
           search: None,
           daily: None,
           chat: Some(chat),
       })
   }
   ```
5. Routes in `router_with` (same param syntax as the existing `/api/papers/{id}` routes):
   ```rust
       .route("/api/chat/models", get(chat::models))
       .route(
           "/api/papers/{id}/chat",
           get(chat::history).post(chat::send).delete(chat::clear),
       )
   ```
6. `pub async fn serve(...)` gains a trailing parameter `chat: Option<Arc<crate::chat::ChatService>>` and passes it into its `AppState` literal.

In `src/main.rs`, `Command::Serve` arm — after the daily service construction:

```rust
            let chat = xuewen::chat::ChatService::from_config(&cfg.chat);
            if chat.is_none() {
                tracing::info!("paper chat disabled (no [[chat.models]] configured)");
            }
```

and pass `chat` as the new final argument to `web::serve(...)`.

- [ ] **Step 5: Run and verify pass**

Run: `cargo test` — expected: PASS (new web_chat_test + all existing suites; `web_test.rs`/`web_daily_test.rs` compile against the extended AppState because they use the builder functions, which you updated).

- [ ] **Step 6: Format and commit**

```bash
cargo fmt
git add src/web src/main.rs tests/web_chat_test.rs tests/common
git -c commit.gpgsign=false commit -m "feat(chat): chat API — models list, thread history/clear, SSE send"
```

---

## Task 6: Frontend SSE parser (`lib/sse.ts`)

**Files:**
- Create: `frontend/src/lib/sse.ts`
- Test: `frontend/src/lib/sse.test.ts`

**Interfaces:**
- Produces: `SseEvent { event: string; data: string }`; `readSse(body: ReadableStream<Uint8Array>, onEvent: (e: SseEvent) => void): Promise<void>` — resolves when the stream ends; used by Task 7.

- [ ] **Step 1: Write the failing test**

`frontend/src/lib/sse.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { readSse, type SseEvent } from './sse';

function streamOf(...chunks: string[]): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc.encode(c));
      controller.close();
    },
  });
}

describe('readSse', () => {
  it('parses events split arbitrarily across chunks', async () => {
    const events: SseEvent[] = [];
    await readSse(
      streamOf('event: delta\ndata: {"text":"He', 'l"}\n\nevent: done\ndata: {"id":1}\n\n'),
      (e) => events.push(e),
    );
    expect(events).toEqual([
      { event: 'delta', data: '{"text":"Hel"}' },
      { event: 'done', data: '{"id":1}' },
    ]);
  });

  it('defaults the event name to message and joins multi-line data', async () => {
    const events: SseEvent[] = [];
    await readSse(streamOf('data: a\ndata: b\n\n'), (e) => events.push(e));
    expect(events).toEqual([{ event: 'message', data: 'a\nb' }]);
  });

  it('ignores a trailing partial event', async () => {
    const events: SseEvent[] = [];
    await readSse(streamOf('event: delta\ndata: {"text":"x"}'), (e) => events.push(e));
    expect(events).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix frontend run test -- src/lib/sse.test.ts` — expected: FAIL (module missing).

- [ ] **Step 3: Implement `frontend/src/lib/sse.ts`**

```ts
/// Minimal SSE reader for fetch() response bodies (EventSource cannot POST).
/// Calls onEvent once per complete event; a trailing partial event (stream
/// cut mid-message) is dropped — the caller treats a missing `done` as an
/// interrupted reply.
export interface SseEvent {
  event: string;
  data: string;
}

export async function readSse(
  body: ReadableStream<Uint8Array>,
  onEvent: (e: SseEvent) => void,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx: number;
    while ((idx = buf.indexOf('\n\n')) !== -1) {
      const raw = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      let event = 'message';
      const dataLines: string[] = [];
      for (const line of raw.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) dataLines.push(line.slice(5).trimStart());
      }
      if (dataLines.length) onEvent({ event, data: dataLines.join('\n') });
    }
  }
}
```

- [ ] **Step 4: Run to verify pass, then commit**

```bash
npm --prefix frontend run test -- src/lib/sse.test.ts
npm --prefix frontend run check
git add frontend/src/lib/sse.ts frontend/src/lib/sse.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): SSE reader for streamed chat replies"
```

---

## Task 7: Chat store (`lib/chat.svelte.ts`)

**Files:**
- Create: `frontend/src/lib/chat.svelte.ts`
- Test: `frontend/src/lib/chat.test.ts`

**Interfaces:**
- Consumes: `readSse` (Task 6), `viewer` from `lib/state.svelte`.
- Produces (used by Tasks 8–9): `chat` rune store `{ available, models: {id,label}[], modelId: string|null, open: boolean, paperId: string|null, messages: ChatTurn[], pending: string|null, streaming: string|null, busy: boolean, error: string|null, draft: string }` with `ChatTurn { id, role: 'user'|'assistant', content, model: string|null, created_at }`; functions `loadChatModels(): Promise<void>`, `setChatModel(id: string): void`, `toggleChat(): void` (no-op unless a PDF tab is active and chat is available), `loadThread(paperId: string): Promise<void>` (no-op if already on that paper), `sendChatMessage(): Promise<void>` (sends `chat.draft`), `stopChatStream(): void`, `clearChatThread(): Promise<void>`.

- [ ] **Step 1: Write the failing store tests**

`frontend/src/lib/chat.test.ts`:

```ts
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
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix frontend run test -- src/lib/chat.test.ts` — expected: FAIL (module missing).

- [ ] **Step 3: Implement `frontend/src/lib/chat.svelte.ts`**

```ts
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
  aborter = new AbortController();
  try {
    const resp = await fetch(`/api/papers/${encodeURIComponent(chat.paperId)}/chat`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ model_id: chat.modelId, message: text }),
      signal: aborter.signal,
    });
    if (!resp.ok || !resp.body) throw new Error(`request failed (${resp.status})`);
    let failure: string | null = null;
    let completed = false;
    await readSse(resp.body, (e) => {
      if (my !== session) return;
      if (e.event === 'delta') {
        chat.streaming = (chat.streaming ?? '') + (JSON.parse(e.data).text ?? '');
      } else if (e.event === 'error') {
        failure = String(JSON.parse(e.data).message ?? 'unknown error');
      } else if (e.event === 'done') {
        completed = true;
        const label = chat.models.find((m) => m.id === chat.modelId)?.label ?? null;
        chat.messages.push({ id: -1, role: 'user', content: text, model: null, created_at: '' });
        chat.messages.push({
          id: Number(JSON.parse(e.data).id ?? -1),
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
    if (my !== session) return;
    const aborted = err instanceof DOMException && err.name === 'AbortError';
    chat.pending = null;
    chat.streaming = null;
    chat.draft = text; // give the message back for editing or resend
    chat.error = aborted
      ? null
      : `The model request failed: ${(err as Error).message} Send again to retry.`;
  } finally {
    if (my === session) chat.busy = false;
    aborter = null;
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
```

- [ ] **Step 4: Run and verify pass, then commit**

```bash
npm --prefix frontend run test -- src/lib/chat.test.ts
npm --prefix frontend run test
npm --prefix frontend run check
git add frontend/src/lib/chat.svelte.ts frontend/src/lib/chat.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): chat store with streamed sends and per-paper threads"
```

---

## Task 8: ChatBubble, ChatPanel, App wiring, Toaster move

**Files:**
- Create: `frontend/src/components/ChatBubble.svelte`
- Create: `frontend/src/components/ChatPanel.svelte`
- Modify: `frontend/src/components/Toaster.svelte` (position class only)
- Modify: `frontend/src/App.svelte`
- Test: `frontend/src/components/ChatPanel.test.ts`

**Interfaces:**
- Consumes: the `chat` store + functions (Task 7), `dur`/`DUR` from motion, `viewer` from state.
- Produces: `<ChatBubble />` and `<ChatPanel />` (no props) rendered inside the reader's PDF wrapper. Normative copy from the Design Foundation table.

- [ ] **Step 1: Write the failing panel tests**

`frontend/src/components/ChatPanel.test.ts`:

```ts
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
    expect(screen.getByText('Mock A')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix frontend run test -- src/components/ChatPanel.test.ts` — expected: FAIL (component missing).

- [ ] **Step 3: Create `frontend/src/components/ChatBubble.svelte`**

```svelte
<script lang="ts">
  // The assistant's launcher: 問 (wèn, "to ask") — the counterpart of the
  // wordmark's 學. Amber because it is an action; the cinnabar seal stays
  // reserved for its two identity placements.
  import { fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { chat } from '../lib/chat.svelte';
</script>

<button
  type="button"
  aria-label="Chat about this paper"
  title="Chat about this paper (c)"
  onclick={() => (chat.open = true)}
  transition:fade={{ duration: dur(DUR.fast) }}
  class="absolute bottom-5 right-5 z-[45] flex h-12 w-12 select-none items-center justify-center rounded-full bg-amber-700 font-serif text-xl leading-none text-paper shadow-lg hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
>問</button>
```

- [ ] **Step 4: Create `frontend/src/components/ChatPanel.svelte`**

```svelte
<script lang="ts">
  import { Eraser, Minus, SendHorizontal, Square } from 'lucide-svelte';
  import { scale } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import {
    chat,
    clearChatThread,
    sendChatMessage,
    setChatModel,
    stopChatStream,
  } from '../lib/chat.svelte';

  let transcript = $state<HTMLElement | null>(null);
  // Stick to the bottom unless the reader scrolled up to reread something.
  let stick = $state(true);
  function onScroll() {
    if (!transcript) return;
    stick = transcript.scrollTop + transcript.clientHeight >= transcript.scrollHeight - 40;
  }
  $effect(() => {
    void chat.messages.length;
    void chat.streaming;
    if (stick && transcript) transcript.scrollTop = transcript.scrollHeight;
  });

  let confirmingClear = $state(false);

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // The panel owns this Esc — it must not also exit zen.
      e.stopPropagation();
      chat.open = false;
    }
  }
  function onComposerKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void sendChatMessage();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the section
     is not an interaction target; it delegates Esc bubbling up from the
     focused composer/controls so the panel can close itself. -->
<section
  role="complementary"
  aria-label="Paper chat"
  onkeydown={onKeydown}
  transition:scale={{ start: 0.92, duration: dur(DUR.base) }}
  style="transform-origin: bottom right"
  class="absolute bottom-5 right-5 z-[45] flex h-[560px] max-h-[80%] w-[400px] max-w-[calc(100%-2.5rem)] flex-col overflow-hidden rounded-xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
>
  <header class="flex shrink-0 items-center gap-2 border-b border-stone-200 px-3 py-2 dark:border-stone-800">
    <select
      aria-label="Model"
      value={chat.modelId}
      onchange={(e) => setChatModel((e.currentTarget as HTMLSelectElement).value)}
      class="min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
    >
      {#each chat.models as m (m.id)}
        <option value={m.id}>{m.label}</option>
      {/each}
    </select>
    <button
      type="button"
      aria-label="Clear conversation"
      onclick={() => (confirmingClear = true)}
      class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <Eraser size={15} />
    </button>
    <button
      type="button"
      aria-label="Minimize chat"
      title="Minimize (Esc)"
      onclick={() => (chat.open = false)}
      class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <Minus size={15} />
    </button>
  </header>

  {#if confirmingClear}
    <div class="flex shrink-0 items-center gap-2 border-b border-stone-200 bg-parchment/60 px-3 py-2 text-sm dark:border-stone-800 dark:bg-stone-800/40">
      <span class="min-w-0 flex-1 text-stone-600 dark:text-stone-300">Clear this conversation?</span>
      <button
        type="button"
        onclick={() => {
          confirmingClear = false;
          void clearChatThread();
        }}
        class="rounded-lg bg-red-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-red-700"
      >
        Clear
      </button>
      <button
        type="button"
        onclick={() => (confirmingClear = false)}
        class="rounded-lg px-2.5 py-1 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        Cancel
      </button>
    </div>
  {/if}

  <div bind:this={transcript} onscroll={onScroll} class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
    {#if chat.messages.length === 0 && chat.pending === null}
      <p class="px-2 pt-6 text-center text-sm text-stone-400 dark:text-stone-500">
        Ask about the methods, the results, or how this paper connects to what you already know.
      </p>
    {/if}
    {#each chat.messages as m (m.id + m.role + m.content.length)}
      {#if m.role === 'user'}
        <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
          {m.content}
        </div>
      {:else}
        <div class="mr-2">
          <div class="whitespace-pre-wrap font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
            {m.content}
          </div>
          {#if m.model}
            <p class="mt-1 font-mono text-[10px] uppercase tracking-wide text-stone-400 dark:text-stone-500">
              {m.model}
            </p>
          {/if}
        </div>
      {/if}
    {/each}
    {#if chat.pending !== null}
      <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
        {chat.pending}
      </div>
      <div class="mr-2 whitespace-pre-wrap font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
        {chat.streaming}<span class="animate-pulse">▍</span>
      </div>
    {/if}
    {#if chat.error}
      <p class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-500/10 dark:text-red-400">
        {chat.error}
      </p>
    {/if}
  </div>

  <footer class="flex shrink-0 items-end gap-2 border-t border-stone-200 p-2 dark:border-stone-800">
    <textarea
      bind:value={chat.draft}
      onkeydown={onComposerKeydown}
      rows="2"
      placeholder="Ask about this paper…"
      class="min-h-0 flex-1 resize-none rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-sm outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
    ></textarea>
    {#if chat.busy}
      <button
        type="button"
        onclick={stopChatStream}
        class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      >
        <Square size={13} /> Stop
      </button>
    {:else}
      <button
        type="button"
        onclick={() => void sendChatMessage()}
        disabled={!chat.draft.trim()}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <SendHorizontal size={14} /> Send
      </button>
    {/if}
  </footer>
</section>
```

- [ ] **Step 5: Move the Toaster and wire App**

`frontend/src/components/Toaster.svelte`: change the wrapper class `bottom-4 right-4` → `bottom-4 left-4` (the assistant owns the bottom-right corner now).

`frontend/src/App.svelte`:
1. Imports: `ChatBubble`, `ChatPanel` from components; `chat, loadChatModels, loadThread` from `./lib/chat.svelte`.
2. In `onMount`, add `loadChatModels();` alongside the other loads.
3. Add an effect near the pane effects:
   ```ts
   // The chat thread follows the active paper while the panel is open.
   $effect(() => {
     if (chat.open && viewer.activeId) void loadThread(viewer.activeId);
   });
   ```
4. The PDF wrapper div gains `relative` and the two overlays. It becomes:
   ```svelte
        <div class={`relative min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
          {#if chat.available && !chat.open}<ChatBubble />{/if}
          {#if chat.open}<ChatPanel />{/if}
        </div>
   ```
   (The wrapper hides with `display: none` on the Library home, which hides bubble and card with it; zen leaves this wrapper visible, so the overlay survives zen by construction.)

- [ ] **Step 6: Run everything and verify pass**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
```

Expected: PASS. If svelte-check flags the `<section onkeydown>` a11y rule despite the ignore comment, keep the comment format exactly as the existing one in `SearchBox.svelte` (same rule, same delegation pattern).

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/{ChatBubble,ChatPanel,Toaster}.svelte frontend/src/components/ChatPanel.test.ts frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): floating paper-chat bubble and streaming panel"
```

---

## Task 9: `c` shortcut, Esc chain, final verification

**Files:**
- Modify: `frontend/src/lib/shortcuts.ts`
- Test: `frontend/src/lib/shortcuts.test.ts` (extend)

**Interfaces:**
- Consumes: `toggleChat`, `chat` from `lib/chat.svelte` (Task 7).
- Produces: final keymap — `c` toggles chat; Esc precedence palette → chat → zen.

- [ ] **Step 1: Extend the shortcuts tests**

Add to `frontend/src/lib/shortcuts.test.ts` (extend the existing imports with `chat` from `./chat.svelte`; reset `chat.open = false; chat.available = false;` in the existing `beforeEach`):

```ts
  it('c toggles the chat only with an active tab and available chat', () => {
    handleKeydown(key('c'));
    expect(chat.open).toBe(false);
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('c'));
    expect(chat.open).toBe(true);
    handleKeydown(key('c'));
    expect(chat.open).toBe(false);
  });

  it('Escape closes the chat before exiting zen', () => {
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    handleKeydown(key('c'));
    expect(ui.zen).toBe(true);
    expect(chat.open).toBe(true);
    handleKeydown(key('Escape'));
    expect(chat.open).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix frontend run test -- src/lib/shortcuts.test.ts` — expected: FAIL (`c` unbound; Esc exits zen first).

- [ ] **Step 3: Implement in `frontend/src/lib/shortcuts.ts`**

1. Import: `import { chat, toggleChat } from './chat.svelte';`
2. Escape branch becomes (palette → chat → zen):
   ```ts
   if (e.key === 'Escape') {
     if (ui.paletteOpen) ui.paletteOpen = false;
     else if (chat.open) chat.open = false;
     else if (ui.zen) ui.zen = false;
     return;
   }
   ```
3. Add to the single-key switch (alphabetically with the others):
   ```ts
   case 'c':
     toggleChat();
     break;
   ```

- [ ] **Step 4: Run, full verification**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
npm --prefix frontend run build
cargo test
cargo fmt --check
cargo build
```

Expected: all green; `cargo build` embeds the fresh `frontend/dist`.

- [ ] **Step 5: Manual QA checklist** (controller runs against a live server with a `[[chat.models]]` entry — a local Ollama/vLLM or a keyed provider)

- No `[chat]` config → no bubble anywhere; `c` inert; `/api/chat/models` says unavailable.
- With config: open a PDF → amber 問 bubble bottom-right; click or `c` expands the card from the corner.
- Model dropdown lists config labels; choice survives a reload.
- Send → user turn on parchment, serif reply streams in with a cursor, model label appears underneath when done; thread survives a reload.
- Stop mid-reply → message returns to the input, nothing persisted.
- Provider error (bad key) → inline red error naming the failure, message back in the input.
- Zen mode: bubble and card visible and functional; Esc order = card → zen.
- Switching PDF tabs swaps the thread; Library home hides bubble/card.
- Clear → confirm strip → transcript empties (and stays empty after reload).
- Toasts now appear bottom-left and don't collide with the chat.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/shortcuts.ts frontend/src/lib/shortcuts.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): c shortcut and Esc precedence for the paper chat"
```

---

## Plan Self-Review (done at authoring time)

1. **Spec coverage:** config (T1), shared client + streaming (T2), schema/store/purge (T3), service + context + cache + extraction fallback (T4), all four endpoints incl. SSE protocol, key privacy, validation codes, all-or-nothing persistence (T5), SSE parsing (T6), store with abort/error/draft-restore/session-guard/localStorage (T7), bubble/panel/copy/Toaster-move/App wiring incl. zen-survival and home-hiding (T8), `c` + Esc chain + manual QA (T9). Non-goals respected: no tools, no markdown rendering (plain-prose instruction in T4's prompt), no RAG.
2. **Placeholder scan:** T2 Step 3 contains an explicit `unimplemented!()` marker with same-step instructions to replace it by moving the existing `complete` body — the step's text makes leaving it a task failure; T3/T5 reference existing test seeding helpers by anchor (mirror `src/db.rs` tests / `tests/common`) with concrete fallback code shown. No TBDs.
3. **Type consistency:** `ChatModelConfig`/`resolve_key` (T1) used in T2 tests? — no; used in T5 (`model.resolve_key()`) ✓; `LlmClient::new(&str, &str, Option<String>)` consistent T2/T5 ✓; `insert_exchange(pool, paper_id, user, assistant, label) -> Result<i64>` consistent T3/T5 ✓; `ChatService::from_config(&ChatConfig) -> Option<Arc<Self>>` and `paper_text(&self, &Path, &Paper)` consistent T4/T5 ✓; `readSse(body, onEvent)` consistent T6/T7 ✓; store field names (`pending`, `streaming`, `draft`, `modelId`) consistent T7/T8/T9 ✓; SSE event names `delta`/`done`/`error` and payload keys `text`/`id`/`message` consistent T5/T6-tests/T7 ✓.
