# Structured Summaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the daily job's per-paper LLM call from a free-text TL;DR to a structured five-part summary (`tldr, problem, approach, results, limitations`) plus a regex-extracted GitHub `code_url`, stored, served, and rendered on the Glance widget.

**Architecture:** Same call count and fallback chain as today — only the prompt, parsing, storage, DTO, and widget change. `Summary` lives in `src/daily/tldr.rs`; migration 0009 adds two nullable columns; the `tldr` column stays (filled from `summary.tldr`) so every existing consumer keeps working. Spec: `docs/superpowers/specs/2026-07-10-structured-summaries-design.md`.

**Tech Stack:** Existing crates only (serde/serde_json, regex + `std::sync::LazyLock` per `src/identify.rs` style, wiremock for tests).

## Global Constraints

- **No new crate dependencies.**
- Run tests with `cargo test` from the repo root (dev shell via direnv; fallback `nix develop -c 'cargo test'`).
- `Summary` has exactly five `String` fields: `tldr, problem, approach, results, limitations`. The prompt demands ONLY a JSON object with exactly those keys, in the configured language, `tldr` one sentence, other fields 1–2 sentences, ~120 words total, concrete numbers preferred in `results`.
- A JSON parse failure is treated exactly like an API failure (rides the existing full-text → abstract-only → `None` fallback). Batch-level behavior must not change: a summary failure can never fail a run.
- `daily_papers.tldr` stays and is set from `summary.tldr` when a summary exists, `NULL` otherwise. Migration `0009_add_daily_summary.sql` is additive only; NEVER edit migrations 0001–0008 (sqlx checksums).
- GitHub regex: `https?://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+`, first match, trailing `.` trimmed; no LLM involvement.
- Commit style: conventional commits with scope. Never commit `docs/superpowers/` or `.superpowers/` files.

---

### Task 1: `Summary` type + `generate_summary` (`src/daily/tldr.rs`)

**Files:**
- Modify: `src/daily/tldr.rs`

**Interfaces:**
- Consumes: existing `ChatClient::complete`, `SYSTEM`, `FULL_TEXT_CAP`.
- Produces: `pub struct Summary { tldr, problem, approach, results, limitations: String }` deriving `Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize`; `pub async fn generate_summary(chat: &ChatClient, language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> Option<Summary>`. `generate_tldr` is left in place (still called by job.rs) and removed in Task 3.

- [ ] **Step 1: Write the failing tests** — append to the `tests` module in `src/daily/tldr.rs` (the module already has `chat_response`, wiremock imports, and `json!`):

```rust
    fn summary_json() -> serde_json::Value {
        json!({
            "tldr": "One line.",
            "problem": "Gap.",
            "approach": "Idea.",
            "results": "+4.2 on X.",
            "limitations": "Small data."
        })
    }

    #[test]
    fn parses_plain_and_fenced_summary_json() {
        let plain = summary_json().to_string();
        assert_eq!(parse_summary(&plain).unwrap().tldr, "One line.");
        let fenced = format!("```json\n{plain}\n```");
        assert_eq!(parse_summary(&fenced).unwrap().problem, "Gap.");
        let bare_fence = format!("```\n{plain}\n```");
        assert_eq!(parse_summary(&bare_fence).unwrap().approach, "Idea.");
        assert!(parse_summary("not json at all").is_err());
    }

    #[test]
    fn prompt_names_all_keys_and_language() {
        let p = prompt("German", "T", "A", None);
        for key in ["tldr", "problem", "approach", "results", "limitations"] {
            assert!(p.contains(&format!("\"{key}\"")), "missing key {key}");
        }
        assert!(p.contains("German"));
    }

    #[tokio::test]
    async fn summary_falls_back_from_full_text_to_abstract() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Preview of main content"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(chat_response(&summary_json().to_string())),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_summary(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert_eq!(out.unwrap().tldr, "One line.");
    }

    #[tokio::test]
    async fn summary_unparsable_reply_falls_back_then_none() {
        // 200s with non-JSON content: parse failure on the full-text attempt,
        // parse failure again on the abstract-only attempt -> None.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(chat_response("free text, no JSON")),
            )
            .expect(2)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_summary(&c, "English", "T", "A", Some("full text")).await;
        assert!(out.is_none());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib daily::tldr`
Expected: compile error — `Summary` / `parse_summary` / `generate_summary` not found.

- [ ] **Step 3: Implement.** In `src/daily/tldr.rs`:

(a) Add after the `SYSTEM` const:

```rust
/// Structured five-part paper summary produced by the LLM.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub tldr: String,
    pub problem: String,
    pub approach: String,
    pub results: String,
    pub limitations: String,
}
```

(b) REPLACE the body of the existing `fn prompt(...)` (signature unchanged) with:

```rust
fn prompt(language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> String {
    let mut p = format!(
        "Summarize the following paper as a JSON object with exactly these string \
         keys: \"tldr\", \"problem\", \"approach\", \"results\", \"limitations\". \
         Write in {language}. Keep \"tldr\" to one sentence and every other field \
         to 1-2 sentences, about 120 words in total. Prefer concrete numbers in \
         \"results\" (benchmark, metric, delta over baseline). Base \"limitations\" \
         on the paper's own discussion when present. Output ONLY the JSON object.\n\n\
         Title: {title}\n\nAbstract: {abstract_text}\n"
    );
    if let Some(t) = full_text {
        let capped: String = t.chars().take(FULL_TEXT_CAP).collect();
        p.push_str("\nPreview of main content:\n");
        p.push_str(&capped);
        p.push('\n');
    }
    p
}
```

(c) Add after `prompt`:

```rust
/// Parse the model's reply as a `Summary`, tolerating a Markdown code fence
/// ("```json ... ```" or "``` ... ```") around the JSON object.
fn parse_summary(reply: &str) -> Result<Summary> {
    let mut s = reply.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        s = rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    Ok(serde_json::from_str(s)?)
}

async fn summary_attempt(
    chat: &ChatClient,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Result<Summary> {
    let reply = chat
        .complete(SYSTEM, &prompt(language, title, abstract_text, full_text))
        .await?;
    parse_summary(&reply)
}

/// Best-effort structured summary: full-text prompt, then abstract-only,
/// then `None`. A parse failure counts as a call failure. Never propagates
/// an error — a bad paper must not fail the batch.
pub async fn generate_summary(
    chat: &ChatClient,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Option<Summary> {
    if full_text.is_some() {
        match summary_attempt(chat, language, title, abstract_text, full_text).await {
            Ok(s) => return Some(s),
            Err(e) => tracing::warn!("full-text summary failed for {title}: {e}"),
        }
    }
    match summary_attempt(chat, language, title, abstract_text, None).await {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("abstract summary failed for {title}: {e}");
            None
        }
    }
}
```

Leave `generate_tldr` and its two `tldr_*` tests untouched — they still compile against the new `prompt` (they assert on the raw string reply and the "Preview of main content" marker, both unchanged). They are deleted in Task 3.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib daily::tldr`
Expected: all pass (10 tests: 6 pre-existing + 4 new). Then `cargo test` once — all green.

- [ ] **Step 5: Commit**

```bash
git add src/daily/tldr.rs
git commit -m "feat(daily): structured five-part summary generation"
```

---

### Task 2: Migration 0009 + store columns

**Files:**
- Create: `migrations/0009_add_daily_summary.sql`
- Modify: `src/daily/store.rs`
- Modify (compile-only field additions): `src/daily/job.rs`, `tests/web_daily_test.rs`

**Interfaces:**
- Consumes: `super::tldr::Summary` (Task 1).
- Produces: `DailyPaper` gains `pub summary: Option<super::tldr::Summary>` and `pub code_url: Option<String>` (after `tldr`); `replace_batch`/`latest_batch` persist/load them. Task 3 fills them with real values; Task 4 serves them.

- [ ] **Step 1: Write the migration** — `migrations/0009_add_daily_summary.sql`:

```sql
ALTER TABLE daily_papers ADD COLUMN summary  TEXT;  -- JSON: {tldr, problem, approach, results, limitations}
ALTER TABLE daily_papers ADD COLUMN code_url TEXT;
```

- [ ] **Step 2: Write the failing test** — append to the `tests` module in `src/daily/store.rs`:

```rust
    #[tokio::test]
    async fn summary_and_code_url_roundtrip() {
        let pool = pool().await;
        let mut p = paper("2026-07-10", 1, "2507.00001");
        p.summary = Some(crate::daily::tldr::Summary {
            tldr: "One line.".into(),
            problem: "Gap.".into(),
            approach: "Idea.".into(),
            results: "+4.2 on X.".into(),
            limitations: "Small data.".into(),
        });
        p.code_url = Some("https://github.com/acme/widget".into());
        replace_batch(&pool, "2026-07-10", std::slice::from_ref(&p)).await.unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(papers[0].summary, p.summary);
        assert_eq!(papers[0].code_url, p.code_url);

        // Unset stays None on read (same path old NULL rows take).
        let bare = paper("2026-07-11", 1, "2507.00002");
        replace_batch(&pool, "2026-07-11", &[bare]).await.unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert!(papers[0].summary.is_none());
        assert!(papers[0].code_url.is_none());
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib daily::store`
Expected: compile error — no `summary`/`code_url` fields.

- [ ] **Step 4: Implement.**

(a) `src/daily/store.rs` — `DailyPaper` gains, right after the `tldr` field:

```rust
    /// Structured five-part summary; `None` for old rows or failed generation.
    pub summary: Option<super::tldr::Summary>,
    /// First GitHub repository URL found in the paper text.
    pub code_url: Option<String>,
```

(b) `replace_batch` — extend the INSERT to 13 columns:

```rust
        sqlx::query(
            "INSERT INTO daily_papers
               (batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url, summary, code_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
```

and after `.bind(&p.pdf_url)` add:

```rust
        .bind(p.summary.as_ref().map(serde_json::to_string).transpose()?)
        .bind(&p.code_url)
```

(c) `latest_batch` — extend the row tuple and SELECT (append the two columns last so existing indices keep meaning):

```rust
    type Row = (
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        f64,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url, summary, code_url
         FROM daily_papers WHERE batch_date = ? ORDER BY rank",
    )
```

and in the mapping, after `pdf_url: r.10,`:

```rust
                summary: r.11.as_deref().and_then(|s| match serde_json::from_str(s) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        tracing::warn!("unparsable stored summary for {}: {e}", r.2);
                        None
                    }
                }),
                code_url: r.12,
```

(d) Compile-only field additions (values wired in Tasks 3–4):
- `src/daily/store.rs` tests: the `paper()` helper gains `summary: None,` and `code_url: None,`.
- `src/daily/job.rs` `pipeline`: the `rows.push(store::DailyPaper { ... })` literal gains `summary: None,` and `code_url: None,` after `tldr,`.
- `tests/web_daily_test.rs`: the `batch_paper()` helper gains `summary: None,` and `code_url: None,` after the `tldr` field.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib daily::store` then `cargo test`
Expected: all green (new roundtrip test included).

- [ ] **Step 6: Commit**

```bash
git add migrations/0009_add_daily_summary.sql src/daily/store.rs src/daily/job.rs tests/web_daily_test.rs
git commit -m "feat(daily): summary and code_url storage (migration 0009)"
```

---

### Task 3: Job wiring — `generate_summary` + `find_code_url`; retire `generate_tldr`

**Files:**
- Modify: `src/daily/job.rs`, `src/daily/tldr.rs`

**Interfaces:**
- Consumes: `tldr::generate_summary` (Task 1), `DailyPaper.summary/code_url` (Task 2).
- Produces: stored rows have `tldr = summary.tldr`, `summary`, and `code_url` populated; `generate_tldr` and its two `tldr_*` tests are deleted (superseded by the `summary_*` tests).

- [ ] **Step 1: Write the failing tests.**

(a) Append to `src/daily/job.rs` tests:

```rust
    #[test]
    fn finds_github_url_and_trims_punctuation() {
        assert_eq!(
            find_code_url("Code at https://github.com/acme/widget. More text"),
            Some("https://github.com/acme/widget".to_string())
        );
        assert_eq!(
            find_code_url("(https://github.com/a-b/c_d)"),
            Some("https://github.com/a-b/c_d".to_string())
        );
        assert_eq!(find_code_url("no links here"), None);
        assert_eq!(find_code_url("see https://gitlab.com/x/y"), None);
    }
```

(b) In the existing `full_run_dedupes_ranks_and_stores` test, change the chat mock's content to a JSON summary and extend the assertions. Replace:

```rust
                "choices": [{"message": {"role": "assistant", "content": "A TLDR."}}]
```

with:

```rust
                "choices": [{"message": {"role": "assistant",
                    "content": "{\"tldr\":\"A TLDR.\",\"problem\":\"Gap.\",\"approach\":\"Idea.\",\"results\":\"+1.\",\"limitations\":\"Few.\"}"}}]
```

and after the existing `assert_eq!(papers[0].tldr.as_deref(), Some("A TLDR."));` add:

```rust
        let s = papers[0].summary.as_ref().expect("summary stored");
        assert_eq!(s.tldr, "A TLDR.");
        assert_eq!(s.problem, "Gap.");
        assert!(papers[0].code_url.is_none(), "PDFs 404 -> no text -> no code link");
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib daily::job`
Expected: compile error — `find_code_url` not found (and, once it compiles, the tldr assertion would fail until wiring lands).

- [ ] **Step 3: Implement.**

(a) `src/daily/job.rs` — imports gain:

```rust
use regex::Regex;
use std::sync::LazyLock;
```

Add near the consts (style per `src/identify.rs`):

```rust
static GITHUB_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+").unwrap()
});

/// First GitHub repository URL in the text; trailing sentence punctuation
/// the PDF extraction glues on is trimmed.
fn find_code_url(text: &str) -> Option<String> {
    let m = GITHUB_RE.find(text)?;
    Some(m.as_str().trim_end_matches('.').to_string())
}
```

(b) In `pipeline`, replace the TL;DR block of the per-paper loop (from `let tldr = tldr::generate_tldr(` through the `rows.push(...)` fields `tldr, summary: None, code_url: None,`) with:

```rust
        let code_url = full_text.as_deref().and_then(find_code_url);
        let summary = tldr::generate_summary(
            &svc.chat,
            &svc.cfg.llm.language,
            &c.title,
            &c.abstract_text,
            full_text.as_deref(),
        )
        .await;
        rows.push(store::DailyPaper {
            batch_date: batch_date.to_string(),
            rank: i as i64 + 1,
            arxiv_id: c.arxiv_id.clone(),
            title: c.title,
            authors: c.authors,
            abstract_text: c.abstract_text,
            categories: c.categories,
            score: s as f64,
            tldr: summary.as_ref().map(|s| s.tldr.clone()),
            summary,
            code_url,
            abs_url: format!("{ARXIV_ABS_BASE}/{}", c.arxiv_id),
            pdf_url: format!("{ARXIV_PDF_BASE}/{}", c.arxiv_id),
        });
```

(c) `src/daily/tldr.rs` — delete `pub async fn generate_tldr` and the two tests `tldr_falls_back_from_full_text_to_abstract` and `tldr_gives_none_when_all_prompts_fail` (their behavior is covered by `summary_falls_back_from_full_text_to_abstract` / `summary_unparsable_reply_falls_back_then_none`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib daily` then full `cargo test`
Expected: all green; no references to `generate_tldr` remain (`grep -rn generate_tldr src/` is empty).

- [ ] **Step 5: Commit**

```bash
git add src/daily/job.rs src/daily/tldr.rs
git commit -m "feat(daily): wire structured summaries and code links into the job"
```

---

### Task 4: API — expose `summary` and `code_url`

**Files:**
- Modify: `src/web/dto.rs`, `tests/web_daily_test.rs`

**Interfaces:**
- Consumes: `DailyPaper.summary/code_url` (Tasks 2–3); `tldr::Summary` already derives `Serialize`.
- Produces: `GET /api/daily` papers carry `summary` (nested object or null) and `code_url` (string or null); `tldr` unchanged.

- [ ] **Step 1: Write the failing test.** In `tests/web_daily_test.rs`, inside `get_daily_returns_latest_batch`, give the first 2026-07-10 paper real values — after building the two papers, before `replace_batch`, change the construction of the first one:

```rust
    let mut rich = batch_paper("2026-07-10", 1, "2507.2", Some("Short."));
    rich.summary = Some(xuewen::daily::tldr::Summary {
        tldr: "Short.".into(),
        problem: "Gap.".into(),
        approach: "Idea.".into(),
        results: "+4.2 on X.".into(),
        limitations: "Small data.".into(),
    });
    rich.code_url = Some("https://github.com/acme/widget".into());
```

(use `rich` in the `replace_batch` slice in place of the old first paper) and extend the assertions:

```rust
    assert_eq!(v["papers"][0]["summary"]["problem"], "Gap.");
    assert_eq!(v["papers"][0]["summary"]["limitations"], "Small data.");
    assert_eq!(v["papers"][0]["code_url"], "https://github.com/acme/widget");
    assert_eq!(v["papers"][1]["summary"], Value::Null);
    assert_eq!(v["papers"][1]["code_url"], Value::Null);
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test web_daily_test`
Expected: FAIL — `summary` serializes as missing (field not on the DTO yet), assertion on `"summary"["problem"]` fails.

- [ ] **Step 3: Implement.** `src/web/dto.rs` — `DailyPaperDto` gains after `tldr`:

```rust
    pub summary: Option<crate::daily::tldr::Summary>,
    pub code_url: Option<String>,
```

and the `From<&DailyPaper>` impl gains:

```rust
            summary: p.summary.clone(),
            code_url: p.code_url.clone(),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test web_daily_test` then full `cargo test`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add src/web/dto.rs tests/web_daily_test.rs
git commit -m "feat(web): expose summary and code_url in /api/daily"
```

---

### Task 5: Widget — collapsed details block

**Files:**
- Modify: `deploy/k8s/README.md`

**Interfaces:**
- Consumes: the JSON contract from Task 4.
- Produces: documentation only.

- [ ] **Step 1: Update the widget template.** In the "Daily arXiv papers on Glance" section, replace the line

```html
        <p>{{ if .String "tldr" }}{{ .String "tldr" }}{{ else }}{{ .String "abstract" }}{{ end }}</p>
```

with:

```html
        <p>{{ if .String "tldr" }}{{ .String "tldr" }}{{ else }}{{ .String "abstract" }}{{ end }}{{ if .String "code_url" }} · <a href="{{ .String "code_url" }}">Code</a>{{ end }}</p>
        {{ if .String "summary.problem" }}
        <details>
          <summary class="size-h6 color-subdue">details</summary>
          <p><strong>Problem:</strong> {{ .String "summary.problem" }}</p>
          <p><strong>Approach:</strong> {{ .String "summary.approach" }}</p>
          <p><strong>Results:</strong> {{ .String "summary.results" }}</p>
          <p><strong>Limitations:</strong> {{ .String "summary.limitations" }}</p>
        </details>
        {{ end }}
```

(The section's existing "check against your installed Glance version" caveat already covers the template syntax; no wording change needed.)

- [ ] **Step 2: Verify docs-only.** Run `cargo test --test web_daily_test` (unchanged, should pass) and visually confirm the YAML block indentation is consistent with the surrounding template lines.

- [ ] **Step 3: Commit**

```bash
git add deploy/k8s/README.md
git commit -m "docs(deploy): render structured summary details in the Glance widget"
```

---

## Plan Self-Review (completed)

- **Spec coverage:** Summary type/prompt/parse/fallback (T1), migration + storage + NULL semantics (T2), job wiring + tldr-from-summary + code_url + generate_tldr removal (T3), API fields (T4), widget details block + code link (T5). Error handling and compatibility requirements are embedded in T1–T3 steps. No gaps.
- **Placeholder scan:** none.
- **Type consistency:** `Summary` path is `crate::daily::tldr::Summary` (`super::tldr::Summary` inside `src/daily/`); field additions in T2(d) match the literals T3/T4 later rewrite; the 13-column INSERT/SELECT lists match the tuple and the mapping indices (summary = r.11, code_url = r.12).
- **Build-green-per-task check:** T1 keeps `generate_tldr` alive for job.rs; T2 adds fields everywhere they're constructed (store tests, job.rs, web test helper); T3 removes the last `generate_tldr` references in the same commit that stops calling it.
