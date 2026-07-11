# Hybrid Search (Tantivy + Qdrant + Embeddings) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One uniform search box that runs BM25 keyword search (embedded Tantivy) and semantic embedding search (Qdrant server + OpenAI-compatible embeddings API) over paper metadata and full PDF text, fused with RRF, returning papers with highlighted snippets.

**Architecture:** SQLite stays the source of truth (papers + extracted `chunks` + `search_index` state); Tantivy and Qdrant are derived indexes rebuildable from SQLite alone. A background tokio "indexer" task (like the inbox watcher) computes staleness by scanning (pure `planner`), extracts/chunks/embeds stale papers, and removes tombstones. Search runs both engines, fuses ranks, and hydrates through one SQL query that applies project/status filters.

**Tech Stack:** Rust (tokio, axum 0.8, sqlx/SQLite, reqwest, wiremock), `tantivy = "0.24"` (only new crate), Qdrant via its REST API on `:6333` (no qdrant-client crate), OpenAI-compatible `/v1/embeddings`, Svelte 5 runes frontend with vitest.

**Spec:** `docs/superpowers/specs/2026-07-09-semantic-search-design.md`

## Global Constraints

- Only new Rust dependency allowed: `tantivy = "0.24"`. Qdrant and embeddings use the existing `reqwest`. Add feature `"v5"` to the existing `uuid` dependency (Task 9).
- All HTTP in tests is mocked with `wiremock` (already a dev-dependency). Never call a real network in tests.
- Tantivy index directory default: `./search-index` (config `search.index_dir`), gitignored.
- Qdrant defaults: `http://localhost:6333`, collection `xuewen`, cosine distance.
- Embedding defaults: `base_url = "https://api.openai.com/v1"`, `model = "text-embedding-3-small"`, `dims = 1536`, `api_key_env = "OPENAI_API_KEY"`.
- Chunking: target 1200 chars, 200-char overlap, chunks never span pages, `seq 0` = synthetic title+abstract chunk (page NULL).
- Field boosts: `title^3, authors^2, abstract^1.5, body^1`. RRF constant k=60. Keyword top 100; semantic top 50 chunks.
- Search failures degrade, never 500: keyword always works; semantic reports `{available: false, reason}`.
- Snippet strings returned by the server are HTML-safe: text HTML-escaped, only `<mark>`/`</mark>` tags allowed.
- Run Rust tests with `cargo test` (direnv/Nix devshell is already active; if a tool is missing use `nix develop -c '<command>'`). Frontend: `cd frontend && npm test`.
- Commit style: conventional commits like the existing history (`feat(search): …`, `test(web): …`). Do NOT commit `docs/superpowers/**`.
- `pdftotext` (poppler) is present in the devshell; it emits `\f` (form feed) between pages.
- tantivy 0.24 API note: method names in `fts.rs` are written from the 0.24 docs; if the installed crate differs slightly (e.g. snippet module paths), keep the behavior and adjust calls per compiler errors — do not change the public interface of `FtsIndex`.

---

### Task 1: Config — `[search]` and `[search.embedding]` sections

**Files:**
- Modify: `src/config.rs`
- Modify: `xuewen.example.toml` (document the new section)
- Modify: `.gitignore` (add `/search-index/`)

**Interfaces:**
- Produces: `Config.search: SearchConfig` (always present; defaults apply when the TOML section is absent).
- Produces: `pub struct SearchConfig { pub index_dir: PathBuf, pub qdrant_url: String, pub qdrant_collection: String, pub embedding: Option<EmbeddingConfig> }`
- Produces: `pub struct EmbeddingConfig { pub base_url: String, pub model: String, pub dims: usize, pub api_key: Option<String>, pub api_key_env: String }`

- [ ] **Step 1: Write the failing tests** — append to the `tests` module in `src/config.rs`:

```rust
    #[test]
    fn search_defaults_when_section_absent() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.search.index_dir, PathBuf::from("./search-index"));
        assert_eq!(cfg.search.qdrant_url, "http://localhost:6333");
        assert_eq!(cfg.search.qdrant_collection, "xuewen");
        assert!(cfg.search.embedding.is_none());
    }

    #[test]
    fn loads_search_section_with_embedding_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
index_dir = "~/idx"

[search.embedding]
api_key = "sk-test"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        // tilde expanded like inbox_dir/library_root
        assert!(!cfg.search.index_dir.starts_with("~"));
        let e = cfg.search.embedding.unwrap();
        assert_eq!(e.base_url, "https://api.openai.com/v1");
        assert_eq!(e.model, "text-embedding-3-small");
        assert_eq!(e.dims, 1536);
        assert_eq!(e.api_key.as_deref(), Some("sk-test"));
        assert_eq!(e.api_key_env, "OPENAI_API_KEY");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::`
Expected: FAIL — `no field `search` on type `Config``

- [ ] **Step 3: Implement** — in `src/config.rs`, add below `ProxyConfig`:

```rust
/// Search settings. Always present: defaults apply when `[search]` is absent.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Tantivy index directory (derived data; safe to delete).
    pub index_dir: PathBuf,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    /// Absent ⇒ semantic search is unavailable (keyword still works).
    pub embedding: Option<EmbeddingConfig>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            index_dir: PathBuf::from("./search-index"),
            qdrant_url: "http://localhost:6333".to_string(),
            qdrant_collection: "xuewen".to_string(),
            embedding: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    #[serde(default = "default_embed_model")]
    pub model: String,
    #[serde(default = "default_embed_dims")]
    pub dims: usize,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

fn default_embed_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_embed_model() -> String {
    "text-embedding-3-small".to_string()
}
fn default_embed_dims() -> usize {
    1536
}
fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}
```

Add the field to `Config`:

```rust
    #[serde(default)]
    pub search: SearchConfig,
```

And expand tilde in `Config::load` (the `home` value is already cloned once — clone again before the last use):

```rust
        cfg.inbox_dir = expand_tilde(cfg.inbox_dir, home.clone());
        cfg.library_root = expand_tilde(cfg.library_root, home.clone());
        cfg.search.index_dir = expand_tilde(cfg.search.index_dir, home);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::`
Expected: PASS (all config tests, old and new)

- [ ] **Step 5: Document + ignore artifacts.** Append to `xuewen.example.toml`:

```toml
# Search (optional section; these are the defaults).
# Keyword full-text search always works. Semantic search additionally needs
# a running Qdrant server and the [search.embedding] subsection with an API key.
#[search]
#index_dir         = "./search-index"       # Tantivy index (derived; safe to delete)
#qdrant_url        = "http://localhost:6333"
#qdrant_collection = "xuewen"

#[search.embedding]
#base_url    = "https://api.openai.com/v1"  # any OpenAI-compatible endpoint
#model       = "text-embedding-3-small"
#dims        = 1536
#api_key_env = "OPENAI_API_KEY"             # or: api_key = "sk-..."
```

Append to `.gitignore`:

```
/search-index/
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs xuewen.example.toml .gitignore
git commit -m "feat(config): [search] and [search.embedding] sections"
```

---

### Task 2: `pdf::extract_text_all` — full-document extraction

**Files:**
- Modify: `src/pdf.rs`

**Interfaces:**
- Consumes: existing `pdf::extract_text(path, last_page)` pattern (`pdftotext` subprocess).
- Produces: `pub fn extract_text_all(path: &Path) -> Result<String>` — whole document, pages separated by `\f`.

- [ ] **Step 1: Write the failing test** — append inside `src/pdf.rs`'s `tests` module (it already has a `write_pdf`-style helper creating a one-page PDF with `printpdf`; follow the existing test there). Add a two-page fixture:

```rust
    fn write_two_page_pdf(path: &Path, line1: &str, line2: &str) {
        use printpdf::{BuiltinFont, Mm, PdfDocument};
        use std::io::BufWriter;
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line1, 12.0, Mm(15.0), Mm(280.0), &font);
        let (page2, layer2) = doc.add_page(Mm(210.0), Mm(297.0), "L2");
        doc.get_page(page2)
            .get_layer(layer2)
            .use_text(line2, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap()))
            .unwrap();
    }

    #[test]
    fn extract_text_all_returns_every_page_with_separators() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("two.pdf");
        write_two_page_pdf(&pdf, "First Page Words", "Second Page Words");
        let text = extract_text_all(&pdf).unwrap();
        assert!(text.contains("First Page Words"));
        assert!(text.contains("Second Page Words"));
        assert!(text.contains('\u{0c}'), "pdftotext page separator expected");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pdf::`
Expected: FAIL — `cannot find function `extract_text_all``

- [ ] **Step 3: Implement** — add to `src/pdf.rs` below `extract_text`:

```rust
/// Extract text from the whole document (no page limit), pages separated by
/// form feeds (`\f`), using the `pdftotext` binary.
pub fn extract_text_all(path: &Path) -> Result<String> {
    let out = Command::new("pdftotext")
        .arg(path)
        .arg("-") // write to stdout
        .output()
        .map_err(|e| anyhow!("failed to run pdftotext (is poppler-utils installed?): {e}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "pdftotext failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib pdf::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/pdf.rs
git commit -m "feat(pdf): extract_text_all for full-document extraction"
```

---

### Task 3: Chunker (pure)

**Files:**
- Create: `src/search/chunker.rs`
- Create: `src/search/mod.rs` (module shell)
- Modify: `src/lib.rs` (add `pub mod search;` alongside the existing module declarations)

**Interfaces:**
- Produces: `pub struct Chunk { pub seq: i64, pub page: Option<i64>, pub text: String }`
- Produces: `pub fn chunk_paper(title: Option<&str>, abstract_text: Option<&str>, body: &str) -> Vec<Chunk>` — `seq 0` = title+abstract (skipped when both `None`/empty), body chunks `seq >= 1` with 1-based `page`.
- Constants: `pub const TARGET_CHARS: usize = 1200;` `pub const OVERLAP_CHARS: usize = 200;`

- [ ] **Step 1: Create the module shell.** `src/search/mod.rs`:

```rust
pub mod chunker;
```

Add to `src/lib.rs` (alphabetical with the other `pub mod` lines):

```rust
pub mod search;
```

- [ ] **Step 2: Write the failing tests** — bottom of `src/search/chunker.rs` (create the file with just the tests module and a `use super::*;`; the code in Step 4 goes above it):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq0_is_title_plus_abstract() {
        let out = chunk_paper(Some("A Title"), Some("An abstract."), "");
        assert_eq!(out[0].seq, 0);
        assert_eq!(out[0].page, None);
        assert_eq!(out[0].text, "A Title\nAn abstract.");
    }

    #[test]
    fn seq0_skipped_when_no_title_or_abstract() {
        let out = chunk_paper(None, None, "some body text");
        assert!(out.iter().all(|c| c.seq >= 1));
    }

    #[test]
    fn body_chunks_are_page_aware_and_sequential() {
        let body = "page one words\n\nmore text\u{0c}page two words";
        let out = chunk_paper(None, None, body);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].seq, out[0].page), (1, Some(1)));
        assert_eq!((out[1].seq, out[1].page), (2, Some(2)));
        assert!(out[0].text.contains("page one words"));
        assert!(out[1].text.contains("page two words"));
    }

    #[test]
    fn long_page_splits_with_overlap() {
        // 5 paragraphs of ~400 chars force multiple chunks per page.
        let para = "x".repeat(395) + " end.";
        let body = vec![para.clone(); 5].join("\n\n");
        let out = chunk_paper(None, None, &body);
        assert!(out.len() >= 2, "expected multiple chunks, got {}", out.len());
        for c in &out {
            assert!(c.text.len() <= TARGET_CHARS + OVERLAP_CHARS + 2);
        }
        // Overlap: the tail of chunk N reappears at the head of chunk N+1.
        // (overlap_tail is private but visible to this child test module.)
        let tail = overlap_tail(&out[0].text, OVERLAP_CHARS);
        assert!(out[1].text.starts_with(&tail), "chunk 2 must start with chunk 1's tail");
    }

    #[test]
    fn paragraph_longer_than_target_is_split_at_sentences() {
        let sentence = "This sentence is exactly some words long. ";
        let para = sentence.repeat(60); // ~2500 chars, no blank lines
        let out = chunk_paper(None, None, &para);
        assert!(out.len() >= 2);
        assert!(out.iter().all(|c| c.text.len() <= TARGET_CHARS + OVERLAP_CHARS + 2));
    }

    #[test]
    fn empty_body_yields_nothing() {
        assert!(chunk_paper(None, None, "").is_empty());
        assert!(chunk_paper(None, None, "\u{0c}\u{0c}").is_empty());
    }

    #[test]
    fn multibyte_text_never_panics() {
        let body = "日本語のテキスト。".repeat(400);
        let out = chunk_paper(Some("héllo"), None, &body);
        assert!(!out.is_empty()); // no panic on char boundaries
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib search::chunker`
Expected: FAIL — `cannot find function `chunk_paper``

- [ ] **Step 4: Implement** — above the tests in `src/search/chunker.rs`:

```rust
/// Page-aware chunking of `pdftotext` output for indexing and embedding.
///
/// `seq 0` is a synthetic title+abstract chunk (strong paper-level semantic
/// target); body chunks are packed per page (never spanning a page, so
/// snippets can cite an exact page) to ~TARGET_CHARS with OVERLAP_CHARS of
/// carry-over between adjacent chunks.

pub const TARGET_CHARS: usize = 1200;
pub const OVERLAP_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub seq: i64,
    /// 1-based PDF page; `None` for the synthetic seq-0 chunk.
    pub page: Option<i64>,
    pub text: String,
}

pub fn chunk_paper(title: Option<&str>, abstract_text: Option<&str>, body: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let title = title.map(str::trim).filter(|s| !s.is_empty());
    let abstract_text = abstract_text.map(str::trim).filter(|s| !s.is_empty());
    let summary = match (title, abstract_text) {
        (Some(t), Some(a)) => Some(format!("{t}\n{a}")),
        (Some(t), None) => Some(t.to_string()),
        (None, Some(a)) => Some(a.to_string()),
        (None, None) => None,
    };
    if let Some(text) = summary {
        out.push(Chunk { seq: 0, page: None, text });
    }
    let mut seq = 1;
    for (i, page) in body.split('\u{0c}').enumerate() {
        for text in chunk_page(page) {
            out.push(Chunk { seq, page: Some((i + 1) as i64), text });
            seq += 1;
        }
    }
    out
}

/// Pack a page's paragraphs into chunks of ~TARGET_CHARS, carrying
/// OVERLAP_CHARS of tail text into the next chunk.
fn chunk_page(page: &str) -> Vec<String> {
    let paras: Vec<&str> = page
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in paras {
        for piece in split_long(para, TARGET_CHARS) {
            if !cur.is_empty() && cur.len() + piece.len() + 2 > TARGET_CHARS {
                let tail = overlap_tail(&cur, OVERLAP_CHARS);
                chunks.push(std::mem::take(&mut cur));
                cur = tail;
            }
            if !cur.is_empty() {
                cur.push_str("\n\n");
            }
            cur.push_str(&piece);
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Last ~`n` bytes of `s`, starting on a char boundary.
fn overlap_tail(s: &str, n: usize) -> String {
    let mut start = s.len().saturating_sub(n);
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].trim_start().to_string()
}

/// Split a paragraph longer than `max` bytes, preferring sentence boundaries
/// (". "), hard-splitting on a char boundary as a last resort.
fn split_long(para: &str, max: usize) -> Vec<String> {
    if para.len() <= max {
        return vec![para.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = para;
    while rest.len() > max {
        let mut window_end = max;
        while window_end < rest.len() && !rest.is_char_boundary(window_end) {
            window_end += 1;
        }
        let cut = match rest[..window_end].rfind(". ") {
            Some(i) if i > 0 => i + 1, // keep the period
            _ => window_end,
        };
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib search::chunker`
Expected: PASS (7 tests)

- [ ] **Step 6: Commit**

```bash
git add src/search src/lib.rs
git commit -m "feat(search): page-aware chunker with overlap and seq-0 summary chunk"
```

---

### Task 4: Migration 0007 + `search/store.rs` (chunks, search_index, hydration)

**Files:**
- Create: `migrations/0007_add_search.sql`
- Create: `src/search/store.rs`
- Modify: `src/search/mod.rs` (add `pub mod store;`)

**Interfaces:**
- Consumes: `crate::search::chunker::Chunk`, `crate::models::Paper`.
- Produces (all `pub` in `crate::search::store`):
  - `pub struct IndexRow { pub paper_id: String, pub content_hash: String, pub meta_hash: String, pub chunk_count: i64, pub fts_indexed_at: Option<String>, pub vectors_indexed_at: Option<String>, pub embed_model: Option<String>, pub last_error: Option<String>, pub attempts: i64, pub last_attempt_at: Option<String> }` (derives `sqlx::FromRow`)
  - `pub fn meta_hash(p: &Paper) -> String`
  - `pub async fn all_index_rows(pool: &SqlitePool) -> Result<Vec<IndexRow>>`
  - `pub async fn replace_chunks(pool: &SqlitePool, paper_id: &str, chunks: &[Chunk], content_hash: &str, meta_hash: &str) -> Result<()>` — one transaction; clears both `*_indexed_at` stamps
  - `pub async fn chunks_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Chunk>>`
  - `pub async fn chunk_text(pool: &SqlitePool, paper_id: &str, seq: i64) -> Result<Option<Chunk>>`
  - `pub async fn mark_fts_done(pool: &SqlitePool, paper_id: &str) -> Result<()>`
  - `pub async fn mark_vectors_done(pool: &SqlitePool, paper_id: &str, model: &str) -> Result<()>`
  - `pub async fn record_error(pool: &SqlitePool, paper_id: &str, msg: &str) -> Result<()>`
  - `pub async fn remove_index_entry(pool: &SqlitePool, paper_id: &str) -> Result<()>`
  - `pub async fn clear_stamps(pool: &SqlitePool, fts: bool, vectors: bool) -> Result<()>`
  - `pub async fn papers_by_ids_ordered(pool: &SqlitePool, ids: &[String], status: Option<&str>, project: Option<&str>) -> Result<Vec<Paper>>` — non-trashed only, preserves `ids` order

- [ ] **Step 1: Write the migration.** `migrations/0007_add_search.sql`:

```sql
CREATE TABLE chunks (
  paper_id  TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,        -- 0 = synthetic title+abstract chunk
  page      INTEGER,                 -- NULL for seq 0
  text      TEXT NOT NULL,
  PRIMARY KEY (paper_id, seq)
);

-- Deliberately NO foreign key: a row may outlive its paper and act as a
-- tombstone telling the indexer to remove Tantivy/Qdrant entries.
CREATE TABLE search_index (
  paper_id           TEXT PRIMARY KEY,
  content_hash       TEXT NOT NULL,
  meta_hash          TEXT NOT NULL,
  chunk_count        INTEGER NOT NULL DEFAULT 0,
  fts_indexed_at     TEXT,
  vectors_indexed_at TEXT,
  embed_model        TEXT,
  last_error         TEXT,
  attempts           INTEGER NOT NULL DEFAULT 0,
  last_attempt_at    TEXT
);
```

- [ ] **Step 2: Write the failing tests** — tests module at the bottom of `src/search/store.rs`. Reuse the in-repo pattern: `crate::db::connect` against a tempdir SQLite file, and the `sample_paper` shape from `src/db.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, PaperMeta, PaperStatus};
    use crate::search::chunker::Chunk;

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir); // keep the tempdir alive for the test process
        p
    }

    fn paper(id: &str, hash: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    fn two_chunks() -> Vec<Chunk> {
        vec![
            Chunk { seq: 0, page: None, text: "T\nA".into() },
            Chunk { seq: 1, page: Some(1), text: "body".into() },
        ]
    }

    #[test]
    fn meta_hash_changes_with_metadata_only() {
        let a = paper("p1", "h1", "Title One");
        let mut b = paper("p1", "h1", "Title One");
        assert_eq!(meta_hash(&a), meta_hash(&b));
        b.meta.title = Some("Title Two".into());
        assert_ne!(meta_hash(&a), meta_hash(&b));
    }

    #[tokio::test]
    async fn replace_chunks_roundtrip_and_stamp_lifecycle() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();

        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();
        let got = chunks_for_paper(&pool, "p1").await.unwrap();
        assert_eq!(got, two_chunks());
        assert_eq!(chunk_text(&pool, "p1", 1).await.unwrap().unwrap().text, "body");

        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.chunk_count, 2);
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());

        mark_fts_done(&pool, "p1").await.unwrap();
        mark_vectors_done(&pool, "p1", "text-embedding-3-small").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_some());
        assert_eq!(row.embed_model.as_deref(), Some("text-embedding-3-small"));
        assert_eq!(row.attempts, 0);
        assert!(row.last_error.is_none());

        // Replacing chunks again clears the stamps (fresh index required).
        replace_chunks(&pool, "p1", &two_chunks(), "h2", &meta_hash(&p)).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());
        assert_eq!(row.content_hash, "h2");
    }

    #[tokio::test]
    async fn record_error_increments_attempts_and_marks_reset_them() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();

        record_error(&pool, "p1", "boom").await.unwrap();
        record_error(&pool, "p1", "boom2").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.attempts, 2);
        assert_eq!(row.last_error.as_deref(), Some("boom2"));
        assert!(row.last_attempt_at.is_some());

        mark_fts_done(&pool, "p1").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.attempts, 0);
        assert!(row.last_error.is_none());
    }

    #[tokio::test]
    async fn remove_and_clear_stamps() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();
        mark_fts_done(&pool, "p1").await.unwrap();
        mark_vectors_done(&pool, "p1", "m").await.unwrap();

        clear_stamps(&pool, false, true).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_none());

        remove_index_entry(&pool, "p1").await.unwrap();
        assert!(all_index_rows(&pool).await.unwrap().is_empty());
        assert!(chunks_for_paper(&pool, "p1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn papers_by_ids_ordered_preserves_order_and_filters() {
        let pool = pool().await;
        for (id, hash, title) in [("a", "h1", "A"), ("b", "h2", "B"), ("c", "h3", "C")] {
            crate::db::insert_paper(&pool, &paper(id, hash, title)).await.unwrap();
        }
        crate::db::soft_delete(&pool, "c").await.unwrap();

        let ids = vec!["c".to_string(), "b".to_string(), "a".to_string(), "zz".to_string()];
        let got = papers_by_ids_ordered(&pool, &ids, None, None).await.unwrap();
        let got_ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(got_ids, vec!["b", "a"]); // trashed + unknown dropped, order kept
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib search::store`
Expected: FAIL — functions not defined (after adding `pub mod store;` to `src/search/mod.rs`, which you should do now)

- [ ] **Step 4: Implement** — `src/search/store.rs` above the tests:

```rust
use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, SqlitePool};

use crate::models::Paper;
use crate::search::chunker::Chunk;

/// State of a paper's derived search indexes. May outlive its paper (tombstone).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IndexRow {
    pub paper_id: String,
    pub content_hash: String,
    pub meta_hash: String,
    pub chunk_count: i64,
    pub fts_indexed_at: Option<String>,
    pub vectors_indexed_at: Option<String>,
    pub embed_model: Option<String>,
    pub last_error: Option<String>,
    pub attempts: i64,
    pub last_attempt_at: Option<String>,
}

/// Hash of the metadata that feeds the search indexes. Comparing this against
/// the stored value is how identify/refresh edits are detected without any
/// event plumbing in the mutation paths.
pub fn meta_hash(p: &Paper) -> String {
    let mut h = Sha256::new();
    for part in [
        p.meta.title.as_deref().unwrap_or(""),
        p.meta.abstract_text.as_deref().unwrap_or(""),
        p.meta.venue.as_deref().unwrap_or(""),
    ] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    h.update(p.meta.year.map(|y| y.to_string()).unwrap_or_default().as_bytes());
    h.update([0x1f]);
    h.update(serde_json::to_string(&p.meta.authors).unwrap_or_default().as_bytes());
    hex::encode(h.finalize())
}

pub async fn all_index_rows(pool: &SqlitePool) -> Result<Vec<IndexRow>> {
    let rows = sqlx::query_as::<_, IndexRow>("SELECT * FROM search_index")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Replace a paper's chunks and reset its index row (stamps cleared: both
/// tiers must re-index the new content). One transaction.
pub async fn replace_chunks(
    pool: &SqlitePool,
    paper_id: &str,
    chunks: &[Chunk],
    content_hash: &str,
    meta_hash: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    for c in chunks {
        sqlx::query("INSERT INTO chunks (paper_id, seq, page, text) VALUES (?,?,?,?)")
            .bind(paper_id)
            .bind(c.seq)
            .bind(c.page)
            .bind(&c.text)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash, meta_hash, chunk_count) \
         VALUES (?,?,?,?) \
         ON CONFLICT(paper_id) DO UPDATE SET \
           content_hash = excluded.content_hash, meta_hash = excluded.meta_hash, \
           chunk_count = excluded.chunk_count, \
           fts_indexed_at = NULL, vectors_indexed_at = NULL",
    )
    .bind(paper_id)
    .bind(content_hash)
    .bind(meta_hash)
    .bind(chunks.len() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn chunks_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Chunk>> {
    let rows: Vec<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? ORDER BY seq")
            .bind(paper_id)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(seq, page, text)| Chunk { seq, page, text })
        .collect())
}

pub async fn chunk_text(pool: &SqlitePool, paper_id: &str, seq: i64) -> Result<Option<Chunk>> {
    let row: Option<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? AND seq = ?")
            .bind(paper_id)
            .bind(seq)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(seq, page, text)| Chunk { seq, page, text }))
}

pub async fn mark_fts_done(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET fts_indexed_at = ?, attempts = 0, last_error = NULL \
         WHERE paper_id = ?",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_vectors_done(pool: &SqlitePool, paper_id: &str, model: &str) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET vectors_indexed_at = ?, embed_model = ?, \
         attempts = 0, last_error = NULL WHERE paper_id = ?",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(model)
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_error(pool: &SqlitePool, paper_id: &str, msg: &str) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET last_error = ?, attempts = attempts + 1, last_attempt_at = ? \
         WHERE paper_id = ?",
    )
    .bind(msg)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop a paper's index row and chunks (used after de-indexing a tombstone).
pub async fn remove_index_entry(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM search_index WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Force re-indexing of the given tier(s) for every paper (rebuild).
pub async fn clear_stamps(pool: &SqlitePool, fts: bool, vectors: bool) -> Result<()> {
    if fts {
        sqlx::query("UPDATE search_index SET fts_indexed_at = NULL, attempts = 0, last_error = NULL")
            .execute(pool)
            .await?;
    }
    if vectors {
        sqlx::query(
            "UPDATE search_index SET vectors_indexed_at = NULL, attempts = 0, last_error = NULL",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Fetch non-trashed papers by id, preserving the order of `ids` (fusion
/// order), applying the status/project filters the search endpoint supports.
pub async fn papers_by_ids_ordered(
    pool: &SqlitePool,
    ids: &[String],
    status: Option<&str>,
    project: Option<&str>,
) -> Result<Vec<Paper>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL AND id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    qb.push(")");
    if let Some(st) = status.filter(|s| matches!(*s, "resolved" | "needs_review")) {
        qb.push(" AND status = ").push_bind(st.to_string());
    }
    if let Some(pid) = project.map(str::trim).filter(|s| !s.is_empty()) {
        qb.push(" AND id IN (SELECT paper_id FROM paper_projects WHERE project_id = ")
            .push_bind(pid.to_string())
            .push(")");
    }
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    // Reorder to match `ids` (SQL IN gives no ordering guarantee).
    let mut by_id: std::collections::HashMap<String, Paper> =
        papers.into_iter().map(|p| (p.id.clone(), p)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}
```

Note: `Chunk` needs `PartialEq, Eq` for the tests — Task 3 already derives them.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib search::store`
Expected: PASS (5 tests). Also run `cargo test` once — the new migration must not break existing tests.

- [ ] **Step 6: Commit**

```bash
git add migrations/0007_add_search.sql src/search/store.rs src/search/mod.rs
git commit -m "feat(search): chunks + search_index schema and store"
```

---

### Task 5: Staleness planner (pure)

**Files:**
- Create: `src/search/planner.rs`
- Modify: `src/search/mod.rs` (add `pub mod planner;`)

**Interfaces:**
- Consumes: `crate::search::store::IndexRow`.
- Produces:
  - `pub struct PaperState { pub id: String, pub content_hash: String, pub meta_hash: String, pub trashed: bool }`
  - `pub struct Work { pub paper_id: String, pub fts: bool, pub vectors: bool }`
  - `#[derive(Default)] pub struct Plan { pub index: Vec<Work>, pub deindex: Vec<String> }`
  - `pub fn plan(papers: &[PaperState], rows: &[IndexRow], embed_model: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> Plan`

- [ ] **Step 1: Write the failing tests** — bottom of `src/search/planner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn ps(id: &str, ch: &str, mh: &str, trashed: bool) -> PaperState {
        PaperState { id: id.into(), content_hash: ch.into(), meta_hash: mh.into(), trashed }
    }

    fn row(id: &str, ch: &str, mh: &str) -> crate::search::store::IndexRow {
        crate::search::store::IndexRow {
            paper_id: id.into(),
            content_hash: ch.into(),
            meta_hash: mh.into(),
            chunk_count: 2,
            fts_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            vectors_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            embed_model: Some("m1".into()),
            last_error: None,
            attempts: 0,
            last_attempt_at: None,
        }
    }

    #[test]
    fn new_paper_needs_both_tiers() {
        let p = plan(&[ps("a", "h", "m", false)], &[], Some("m1"), Utc::now());
        assert_eq!(p.index.len(), 1);
        assert!(p.index[0].fts && p.index[0].vectors);
        assert!(p.deindex.is_empty());
    }

    #[test]
    fn up_to_date_paper_yields_no_work() {
        let p = plan(&[ps("a", "h", "m", false)], &[row("a", "h", "m")], Some("m1"), Utc::now());
        assert!(p.index.is_empty() && p.deindex.is_empty());
    }

    #[test]
    fn meta_change_and_content_change_force_both_tiers() {
        for (ch, mh) in [("h2", "m"), ("h", "m2")] {
            let p = plan(&[ps("a", ch, mh, false)], &[row("a", "h", "m")], Some("m1"), Utc::now());
            assert!(p.index[0].fts && p.index[0].vectors, "case ({ch},{mh})");
        }
    }

    #[test]
    fn model_change_re_embeds_without_touching_fts() {
        let p = plan(&[ps("a", "h", "m", false)], &[row("a", "h", "m")], Some("m2"), Utc::now());
        assert_eq!(p.index.len(), 1);
        assert!(!p.index[0].fts && p.index[0].vectors);
    }

    #[test]
    fn no_embedder_means_no_vector_work() {
        let p = plan(&[ps("a", "h", "m", false)], &[], None, Utc::now());
        assert!(p.index[0].fts && !p.index[0].vectors);
    }

    #[test]
    fn trashed_and_missing_papers_become_deindex_tombstones() {
        let p = plan(&[ps("a", "h", "m", true)], &[row("a", "h", "m"), row("gone", "h", "m")], Some("m1"), Utc::now());
        assert!(p.index.is_empty());
        let mut d = p.deindex.clone();
        d.sort();
        assert_eq!(d, vec!["a".to_string(), "gone".to_string()]);
    }

    #[test]
    fn failed_rows_back_off_exponentially_capped_at_an_hour() {
        let mut r = row("a", "h", "m");
        r.fts_indexed_at = None;
        r.attempts = 2; // wait = 60 * 2^(2-1) = 120s
        r.last_attempt_at = Some((Utc::now() - Duration::seconds(30)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert!(p.index.is_empty(), "still inside the backoff window");

        r.last_attempt_at = Some((Utc::now() - Duration::seconds(180)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert_eq!(p.index.len(), 1, "window elapsed");

        r.attempts = 50; // cap: never wait more than 3600s
        r.last_attempt_at = Some((Utc::now() - Duration::seconds(3700)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r], None, Utc::now());
        assert_eq!(p.index.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib search::planner`
Expected: FAIL — types not defined (add `pub mod planner;` to `src/search/mod.rs` first)

- [ ] **Step 3: Implement** — above the tests:

```rust
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::search::store::IndexRow;

/// The searchable identity of a paper, as seen by the staleness scan.
#[derive(Debug, Clone)]
pub struct PaperState {
    pub id: String,
    pub content_hash: String,
    pub meta_hash: String,
    pub trashed: bool,
}

/// One paper's pending indexing work (at least one tier is true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub paper_id: String,
    pub fts: bool,
    pub vectors: bool,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub index: Vec<Work>,
    /// Tombstones: index entries whose paper is trashed or gone.
    pub deindex: Vec<String>,
}

/// Compare live papers against `search_index` and decide what to do.
/// Pure: all clock and IO inputs are parameters.
pub fn plan(
    papers: &[PaperState],
    rows: &[IndexRow],
    embed_model: Option<&str>,
    now: DateTime<Utc>,
) -> Plan {
    let by_id: HashMap<&str, &IndexRow> = rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
    let live: HashSet<&str> = papers.iter().filter(|p| !p.trashed).map(|p| p.id.as_str()).collect();
    let mut out = Plan::default();

    for p in papers.iter().filter(|p| !p.trashed) {
        let row = by_id.get(p.id.as_str()).copied();
        let content_changed = row
            .map(|r| r.content_hash != p.content_hash || r.meta_hash != p.meta_hash)
            .unwrap_or(true);
        let fts = content_changed || row.map(|r| r.fts_indexed_at.is_none()).unwrap_or(true);
        let vectors = embed_model.is_some()
            && (content_changed
                || row
                    .map(|r| {
                        r.vectors_indexed_at.is_none() || r.embed_model.as_deref() != embed_model
                    })
                    .unwrap_or(true));
        if (fts || vectors) && backoff_elapsed(row, now) {
            out.index.push(Work { paper_id: p.id.clone(), fts, vectors });
        }
    }
    for r in rows {
        if !live.contains(r.paper_id.as_str()) {
            out.deindex.push(r.paper_id.clone());
        }
    }
    out
}

/// After a failure, wait 60s · 2^(attempts−1), capped at one hour.
fn backoff_elapsed(row: Option<&IndexRow>, now: DateTime<Utc>) -> bool {
    let Some(r) = row else { return true };
    if r.attempts == 0 {
        return true;
    }
    let Some(last) = r
        .last_attempt_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    else {
        return true;
    };
    let exp = (r.attempts - 1).clamp(0, 6) as u32;
    let wait = (60i64 << exp).min(3600);
    now.signed_duration_since(last.with_timezone(&Utc)) >= chrono::Duration::seconds(wait)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib search::planner`
Expected: PASS (7 tests)

- [ ] **Step 5: Commit**

```bash
git add src/search/planner.rs src/search/mod.rs
git commit -m "feat(search): pure staleness/tombstone planner with retry backoff"
```

---

### Task 6: RRF fusion (pure)

**Files:**
- Create: `src/search/fusion.rs`
- Modify: `src/search/mod.rs` (add `pub mod fusion;`)

**Interfaces:**
- Produces: `pub fn rrf(lists: &[Vec<String>], k: f32) -> Vec<(String, f32)>` — descending score, ties broken by id for determinism. Callers pass `k = 60.0`.

- [ ] **Step 1: Write the failing tests** — `src/search/fusion.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[(String, f32)]) -> Vec<&str> {
        v.iter().map(|(id, _)| id.as_str()).collect()
    }

    #[test]
    fn single_list_preserves_order() {
        let out = rrf(&[vec!["a".into(), "b".into(), "c".into()]], 60.0);
        assert_eq!(ids(&out), vec!["a", "b", "c"]);
    }

    #[test]
    fn paper_in_both_lists_outranks_single_list_leaders() {
        // "x" is rank 2 in both lists; "a" and "b" lead one list each.
        let out = rrf(
            &[
                vec!["a".into(), "x".into(), "c".into()],
                vec!["b".into(), "x".into(), "d".into()],
            ],
            60.0,
        );
        assert_eq!(ids(&out)[0], "x"); // 2/(60+2) beats 1/(60+1)
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(rrf(&[], 60.0).is_empty());
        assert!(rrf(&[vec![], vec![]], 60.0).is_empty());
    }

    #[test]
    fn ties_break_by_id_for_determinism() {
        let out = rrf(&[vec!["b".into()], vec!["a".into()]], 60.0);
        assert_eq!(ids(&out), vec!["a", "b"]); // equal scores → lexicographic
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib search::fusion`
Expected: FAIL — `rrf` not defined (add `pub mod fusion;` to `src/search/mod.rs` first)

- [ ] **Step 3: Implement** — above the tests:

```rust
use std::collections::HashMap;

/// Reciprocal Rank Fusion: score(id) = Σ over lists 1/(k + rank), rank 1-based.
/// Items appearing in several lists rise; no score normalization needed.
pub fn rrf(lists: &[Vec<String>], k: f32) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (i, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_default() += 1.0 / (k + (i as f32) + 1.0);
        }
    }
    let mut out: Vec<(String, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib search::fusion`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/search/fusion.rs src/search/mod.rs
git commit -m "feat(search): reciprocal rank fusion"
```

---

### Task 7: Tantivy index wrapper

**Files:**
- Modify: `Cargo.toml` (add `tantivy = "0.24"` to `[dependencies]`)
- Create: `src/search/fts.rs`
- Modify: `src/search/mod.rs` (add `pub mod fts;`)

**Interfaces:**
- Produces:
  - `pub struct FieldSel { pub title: bool, pub authors: bool, pub abstract_text: bool, pub body: bool }` with `impl FieldSel { pub fn all() -> Self; pub fn parse(csv: Option<&str>) -> Self; pub fn authors_only(&self) -> bool; pub fn any(&self) -> bool }` — `parse(None)`/unknown-only input → `all()`.
  - `pub struct PaperDoc { pub id: String, pub title: String, pub authors: String, pub venue: String, pub abstract_text: String, pub body: String }`
  - `pub struct FtsHit { pub paper_id: String, pub score: f32, pub field: String, pub snippet_html: String }`
  - `pub struct FtsIndex` with:
    - `pub fn open(dir: &Path) -> Result<(Self, bool)>` — bool = "created fresh" (new or wiped-after-corruption); caller must clear FTS stamps when true. Thread-safe (`Send + Sync`), writer created lazily so read-only processes don't take Tantivy's writer lock.
    - `pub fn upsert(&self, doc: &PaperDoc) -> Result<()>` (delete-by-id + add + commit + reader reload)
    - `pub fn delete(&self, paper_id: &str) -> Result<()>`
    - `pub fn search(&self, q: &str, sel: &FieldSel, limit: usize) -> Result<Vec<FtsHit>>`
  - `pub fn html_escape(s: &str) -> String` (shared with semantic snippets later)

- [ ] **Step 1: Add the dependency.** In `Cargo.toml` `[dependencies]`:

```toml
tantivy = "0.24"
```

Run: `cargo build` — expect success (just fetches/compiles the crate).

- [ ] **Step 2: Write the failing tests** — bottom of `src/search/fts.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> PaperDoc {
        PaperDoc {
            id: id.into(),
            title: title.into(),
            authors: "Ada Lovelace ; Alan Turing".into(),
            venue: "USENIX Security".into(),
            abstract_text: "We defend binaries against automated analysis.".into(),
            body: body.into(),
        }
    }

    fn open_tmp() -> (FtsIndex, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
        (idx, dir)
    }

    #[test]
    fn parse_field_selection() {
        assert!(FieldSel::parse(None).title);
        let s = FieldSel::parse(Some("authors,body"));
        assert!(!s.title && s.authors && !s.abstract_text && s.body);
        // Unknown-only input falls back to all (never an error).
        assert!(FieldSel::parse(Some("bogus")).title);
        assert!(FieldSel::parse(Some("authors")).authors_only());
        assert!(!FieldSel::parse(Some("authors,title")).authors_only());
    }

    #[test]
    fn upsert_search_and_snippet() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "AntiFuzz: Impeding Fuzzing Audits", "fuzzing resistance techniques")).unwrap();
        idx.upsert(&doc("p2", "Unrelated Paper", "nothing to see here")).unwrap();

        let hits = idx.search("fuzzing", &FieldSel::all(), 10).unwrap();
        assert_eq!(hits[0].paper_id, "p1");
        assert!(hits[0].snippet_html.contains("<mark>"), "got: {}", hits[0].snippet_html);
        assert!(!hits.iter().any(|h| h.paper_id == "p2"));
    }

    #[test]
    fn field_selection_restricts_matching() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "A Title", "the body mentions quicksort")).unwrap();
        let sel = FieldSel { title: true, authors: false, abstract_text: false, body: false };
        assert!(idx.search("quicksort", &sel, 10).unwrap().is_empty());
        let sel = FieldSel { title: false, authors: false, abstract_text: false, body: true };
        let hits = idx.search("quicksort", &sel, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].field, "body");
    }

    #[test]
    fn title_hit_outranks_body_hit() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("in-title", "Quicksort Analysis", "some text")).unwrap();
        idx.upsert(&doc("in-body", "Sorting Survey", "quicksort quicksort quicksort")).unwrap();
        let hits = idx.search("quicksort", &FieldSel::all(), 10).unwrap();
        assert_eq!(hits[0].paper_id, "in-title");
    }

    #[test]
    fn upsert_replaces_and_delete_removes() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "Old Title", "b")).unwrap();
        idx.upsert(&doc("p1", "New Title", "b")).unwrap();
        assert!(idx.search("old", &FieldSel::all(), 10).unwrap().is_empty());
        assert_eq!(idx.search("new", &FieldSel::all(), 10).unwrap().len(), 1);
        idx.delete("p1").unwrap();
        assert!(idx.search("new", &FieldSel::all(), 10).unwrap().is_empty());
    }

    #[test]
    fn corrupt_dir_is_wiped_and_reports_created() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("meta.json"), b"not json").unwrap();
        let (_idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
    }

    #[test]
    fn escapes_html() {
        assert_eq!(html_escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib search::fts`
Expected: FAIL — types not defined (add `pub mod fts;` to `src/search/mod.rs` first)

- [ ] **Step 4: Implement** — above the tests in `src/search/fts.rs`:

```rust
use anyhow::Result;
use std::path::Path;
use std::sync::Mutex;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument, Term};

/// Which paper fields a query runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSel {
    pub title: bool,
    pub authors: bool,
    pub abstract_text: bool,
    pub body: bool,
}

impl FieldSel {
    pub fn all() -> Self {
        Self { title: true, authors: true, abstract_text: true, body: true }
    }

    /// Parse a `fields=title,body` CSV. Absent, empty, or all-unknown input
    /// falls back to every field (unknown values are ignored, never an error).
    pub fn parse(csv: Option<&str>) -> Self {
        let mut sel = Self { title: false, authors: false, abstract_text: false, body: false };
        for part in csv.unwrap_or("").split(',').map(str::trim) {
            match part {
                "title" => sel.title = true,
                "authors" => sel.authors = true,
                "abstract" => sel.abstract_text = true,
                "body" => sel.body = true,
                _ => {}
            }
        }
        if sel.any() {
            sel
        } else {
            Self::all()
        }
    }

    pub fn any(&self) -> bool {
        self.title || self.authors || self.abstract_text || self.body
    }

    /// Authors is the only selected field — semantic search is meaningless.
    pub fn authors_only(&self) -> bool {
        self.authors && !self.title && !self.abstract_text && !self.body
    }
}

/// One paper as a Tantivy document (all fields stored for snippets).
#[derive(Debug, Clone)]
pub struct PaperDoc {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub venue: String,
    pub abstract_text: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct FtsHit {
    pub paper_id: String,
    pub score: f32,
    /// Which field the snippet came from: title|authors|abstract|body.
    pub field: String,
    /// HTML-safe: escaped text with <mark> highlights only.
    pub snippet_html: String,
}

struct FtsFields {
    id: Field,
    title: Field,
    authors: Field,
    venue: Field,
    abstract_text: Field,
    body: Field,
}

pub struct FtsIndex {
    index: Index,
    /// Lazy: read-only users (CLI search while `serve` runs) must not take
    /// Tantivy's single-writer lock.
    writer: Mutex<Option<IndexWriter>>,
    reader: IndexReader,
    f: FtsFields,
}

impl FtsIndex {
    /// Open (or create) the index at `dir`. On corruption the directory is
    /// wiped and recreated — it is derived data. Returns `(index, created)`;
    /// when `created` the caller must clear all FTS stamps so the sweep
    /// re-indexes everything.
    pub fn open(dir: &Path) -> Result<(Self, bool)> {
        std::fs::create_dir_all(dir)?;
        let fresh = !dir.join("meta.json").exists();
        match Self::try_open(dir) {
            Ok(idx) => Ok((idx, fresh)),
            Err(e) => {
                tracing::warn!("tantivy index at {} unusable ({e}); rebuilding", dir.display());
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                Ok((Self::try_open(dir)?, true))
            }
        }
    }

    fn try_open(dir: &Path) -> Result<Self> {
        let mut b = Schema::builder();
        let id = b.add_text_field("paper_id", STRING | STORED);
        let title = b.add_text_field("title", TEXT | STORED);
        let authors = b.add_text_field("authors", TEXT | STORED);
        let venue = b.add_text_field("venue", TEXT | STORED);
        let abstract_text = b.add_text_field("abstract", TEXT | STORED);
        let body = b.add_text_field("body", TEXT | STORED);
        let schema = b.build();
        let index = Index::open_or_create(MmapDirectory::open(dir)?, schema)?;
        let reader = index.reader()?;
        Ok(Self {
            index,
            writer: Mutex::new(None),
            reader,
            f: FtsFields { id, title, authors, venue, abstract_text, body },
        })
    }

    fn with_writer<T>(&self, op: impl FnOnce(&mut IndexWriter) -> Result<T>) -> Result<T> {
        let mut guard = self.writer.lock().expect("fts writer lock poisoned");
        if guard.is_none() {
            *guard = Some(self.index.writer(50_000_000)?);
        }
        let out = op(guard.as_mut().expect("writer just created"))?;
        // Make the change visible to the next search immediately (personal
        // scale: commit cost is negligible).
        self.reader.reload()?;
        Ok(out)
    }

    pub fn upsert(&self, d: &PaperDoc) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, &d.id));
            w.add_document(doc!(
                self.f.id => d.id.clone(),
                self.f.title => d.title.clone(),
                self.f.authors => d.authors.clone(),
                self.f.venue => d.venue.clone(),
                self.f.abstract_text => d.abstract_text.clone(),
                self.f.body => d.body.clone(),
            ))?;
            w.commit()?;
            Ok(())
        })
    }

    pub fn delete(&self, paper_id: &str) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, paper_id));
            w.commit()?;
            Ok(())
        })
    }

    pub fn search(&self, q: &str, sel: &FieldSel, limit: usize) -> Result<Vec<FtsHit>> {
        let q = q.trim();
        if q.is_empty() || !sel.any() {
            return Ok(Vec::new());
        }
        let mut fields = Vec::new();
        if sel.title { fields.push(self.f.title); }
        if sel.authors { fields.push(self.f.authors); }
        if sel.abstract_text { fields.push(self.f.abstract_text); }
        if sel.body { fields.push(self.f.body); }

        let mut parser = QueryParser::for_index(&self.index, fields);
        parser.set_field_boost(self.f.title, 3.0);
        parser.set_field_boost(self.f.authors, 2.0);
        parser.set_field_boost(self.f.abstract_text, 1.5);
        // Lenient: user input must never be a query syntax error.
        let (query, _errors) = parser.parse_query_lenient(q);

        let searcher = self.reader.searcher();
        let top = searcher.search(&query, &TopDocs::with_limit(limit))?;
        let mut out = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let paper_id = doc
                .get_first(self.f.id)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (field, snippet_html) = self.best_snippet(&searcher, query.as_ref(), &doc, sel)?;
            out.push(FtsHit { paper_id, score, field, snippet_html });
        }
        Ok(out)
    }

    /// The first selected field (title > authors > abstract > body) with a
    /// highlighted fragment; falls back to the escaped title text.
    fn best_snippet(
        &self,
        searcher: &tantivy::Searcher,
        query: &dyn tantivy::query::Query,
        doc: &TantivyDocument,
        sel: &FieldSel,
    ) -> Result<(String, String)> {
        let candidates: [(&str, Field, bool); 4] = [
            ("title", self.f.title, sel.title),
            ("authors", self.f.authors, sel.authors),
            ("abstract", self.f.abstract_text, sel.abstract_text),
            ("body", self.f.body, sel.body),
        ];
        for (name, field, enabled) in candidates {
            if !enabled {
                continue;
            }
            let mut gen = SnippetGenerator::create(searcher, query, field)?;
            gen.set_max_num_chars(200);
            let snip = gen.snippet_from_doc(doc);
            if !snip.highlighted().is_empty() {
                let html = snip.to_html().replace("<b>", "<mark>").replace("</b>", "</mark>");
                return Ok((name.to_string(), html));
            }
        }
        let title = doc
            .get_first(self.f.title)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(("title".to_string(), html_escape(title)))
    }
}

/// Minimal HTML escaping for snippet text we assemble ourselves.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib search::fts`
Expected: PASS (7 tests). Remember the Global Constraints note: adjust to the installed tantivy 0.24 API if a method name differs, without changing `FtsIndex`'s public interface.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/search/fts.rs src/search/mod.rs
git commit -m "feat(search): embedded Tantivy index with field boosts and snippets"
```

---

### Task 8: Embedding API client (OpenAI-compatible)

**Files:**
- Create: `src/search/embedder.rs`
- Modify: `src/search/mod.rs` (add `pub mod embedder;`)

**Interfaces:**
- Consumes: `crate::config::EmbeddingConfig`.
- Produces:
  - `pub struct Embedder` with:
    - `pub fn from_config(cfg: &EmbeddingConfig) -> Option<Embedder>` — `None` (with a `tracing::warn!`) when no API key is resolvable from `api_key` / `$api_key_env`.
    - `pub fn model(&self) -> &str`, `pub fn dims(&self) -> usize`
    - `pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>` — batches of 64 per request; 3 attempts with exponential backoff on 429/5xx/network errors; validates every vector length against `dims`.
  - Test hook: `pub fn for_tests(base_url: &str, model: &str, dims: usize) -> Embedder` (`#[cfg(test)]` NOT used — the e2e/indexer tests in other files need it; make it a normal `pub fn` documented as test support, with no API key).

- [ ] **Step 1: Write the failing tests** — bottom of `src/search/embedder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn embedding_response(n: usize, dims: usize) -> serde_json::Value {
        let data: Vec<_> = (0..n)
            .map(|i| json!({"index": i, "embedding": vec![0.1f32; dims]}))
            .collect();
        json!({"data": data})
    }

    #[tokio::test]
    async fn embeds_with_bearer_auth_and_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({"model": "text-embedding-3-small"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(2, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let cfg = crate::config::EmbeddingConfig {
            base_url: format!("{}/v1", server.uri()),
            model: "text-embedding-3-small".into(),
            dims: 4,
            api_key: Some("sk-test".into()),
            api_key_env: "UNSET_VAR_FOR_TEST".into(),
        };
        let e = Embedder::from_config(&cfg).unwrap();
        let out = e.embed(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 4);
    }

    #[tokio::test]
    async fn batches_requests_of_64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(move |req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().unwrap().len();
                assert!(n <= 64, "batch too large: {n}");
                ResponseTemplate::new(200).set_body_json(embedding_response(n, 4))
            })
            .expect(2) // 100 texts -> 64 + 36
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let texts: Vec<String> = (0..100).map(|i| format!("t{i}")).collect();
        let out = e.embed(&texts).await.unwrap();
        assert_eq!(out.len(), 100);
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let out = e.embed(&["a".into()]).await.unwrap();
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn wrong_dims_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 3)))
            .mount(&server)
            .await;
        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let err = e.embed(&["a".into()]).await.unwrap_err().to_string();
        assert!(err.contains("dims"), "got: {err}");
    }

    #[test]
    fn from_config_without_key_is_none() {
        let cfg = crate::config::EmbeddingConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "m".into(),
            dims: 4,
            api_key: None,
            api_key_env: "XUEWEN_TEST_KEY_THAT_IS_NOT_SET".into(),
        };
        assert!(Embedder::from_config(&cfg).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib search::embedder`
Expected: FAIL — `Embedder` not defined (add `pub mod embedder;` to `src/search/mod.rs` first)

- [ ] **Step 3: Implement** — above the tests:

```rust
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::time::Duration;

use crate::config::EmbeddingConfig;

const BATCH: usize = 64;
const ATTEMPTS: u32 = 3;

/// Client for an OpenAI-compatible `/embeddings` endpoint.
pub struct Embedder {
    http: reqwest::Client,
    base_url: String,
    model: String,
    dims: usize,
    api_key: Option<String>,
}

impl Embedder {
    /// `None` when no API key is resolvable — semantic search is then
    /// unavailable, but nothing fails.
    pub fn from_config(cfg: &EmbeddingConfig) -> Option<Self> {
        let key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var(&cfg.api_key_env).ok())
            .filter(|k| !k.trim().is_empty());
        let Some(key) = key else {
            tracing::warn!(
                "[search.embedding] configured but no API key (set api_key or ${})  — semantic search disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self {
            http: reqwest::Client::new(),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            dims: cfg.dims,
            api_key: Some(key),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str, dims: usize) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dims,
            api_key: None,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            out.extend(self.embed_batch(batch).await?);
        }
        Ok(out)
    }

    async fn embed_batch(&self, batch: &[String]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Body {
            data: Vec<Item>,
        }

        let url = format!("{}/embeddings", self.base_url);
        let mut delay = Duration::from_millis(500);
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            let mut req = self
                .http
                .post(&url)
                .json(&serde_json::json!({ "model": self.model, "input": batch }));
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut body: Body = resp.json().await?;
                    if body.data.len() != batch.len() {
                        bail!(
                            "embedding API returned {} vectors for {} inputs",
                            body.data.len(),
                            batch.len()
                        );
                    }
                    body.data.sort_by_key(|d| d.index);
                    for d in &body.data {
                        if d.embedding.len() != self.dims {
                            bail!(
                                "embedding dims mismatch: API returned {}, config says {} — fix [search.embedding].dims",
                                d.embedding.len(),
                                self.dims
                            );
                        }
                    }
                    return Ok(body.data.into_iter().map(|d| d.embedding).collect());
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retriable = status.as_u16() == 429 || status.is_server_error();
                    let text = resp.text().await.unwrap_or_default();
                    let err = anyhow!("embedding API {status}: {}", text.chars().take(200).collect::<String>());
                    if !retriable || attempt == ATTEMPTS {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
                Err(e) => {
                    if attempt == ATTEMPTS {
                        return Err(e.into());
                    }
                    last_err = Some(e.into());
                }
            }
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        Err(last_err.expect("loop ran at least once"))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib search::embedder`
Expected: PASS (5 tests)

- [ ] **Step 5: Commit**

```bash
git add src/search/embedder.rs src/search/mod.rs
git commit -m "feat(search): batched OpenAI-compatible embedding client with backoff"
```

---

### Task 9: Qdrant REST client

**Files:**
- Modify: `Cargo.toml` (uuid features: `["v7"]` → `["v7", "v5"]`)
- Create: `src/search/vector.rs`
- Modify: `src/search/mod.rs` (add `pub mod vector;`)

**Interfaces:**
- Produces:
  - `pub struct ChunkPoint { pub paper_id: String, pub seq: i64, pub page: Option<i64>, pub vector: Vec<f32> }`
  - `pub struct VecHit { pub paper_id: String, pub seq: i64, pub page: Option<i64>, pub score: f32 }`
  - `pub enum SeqFilter { All, OnlySummary, OnlyBody }`
  - `pub fn point_id(paper_id: &str, seq: i64) -> String` — deterministic UUIDv5 of `"{paper_id}:{seq}"` (idempotent upserts)
  - `pub struct QdrantStore` with:
    - `pub fn new(base_url: &str, collection: &str, dims: usize) -> Result<Self>`
    - `pub async fn ensure_collection(&self) -> Result<()>` — creates if 404; errors if the existing vector size ≠ `dims`
    - `pub async fn recreate_collection(&self) -> Result<()>` — DELETE then create (vector rebuild)
    - `pub async fn upsert(&self, points: &[ChunkPoint]) -> Result<()>` (batches of 64, `?wait=true`)
    - `pub async fn search(&self, vector: &[f32], limit: usize, filter: SeqFilter) -> Result<Vec<VecHit>>`
    - `pub async fn delete_paper(&self, paper_id: &str) -> Result<()>`

- [ ] **Step 1: Enable UUIDv5.** In `Cargo.toml` change the uuid line to:

```toml
uuid = { version = "1", features = ["v7", "v5"] }
```

- [ ] **Step 2: Write the failing tests** — bottom of `src/search/vector.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn store(server: &MockServer) -> QdrantStore {
        QdrantStore::new(&server.uri(), "xuewen", 4).unwrap()
    }

    #[test]
    fn point_ids_are_deterministic_uuids() {
        let a = point_id("p1", 0);
        assert_eq!(a, point_id("p1", 0));
        assert_ne!(a, point_id("p1", 1));
        assert_ne!(a, point_id("p2", 0));
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }

    #[tokio::test]
    async fn ensure_creates_missing_collection() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen"))
            .and(body_partial_json(json!({"vectors": {"size": 4, "distance": "Cosine"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).ensure_collection().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_rejects_dims_mismatch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 8, "distance": "Cosine"}}}}
            })))
            .mount(&server)
            .await;
        let err = store(&server).ensure_collection().await.unwrap_err().to_string();
        assert!(err.contains("rebuild --vectors-only"), "got: {err}");
    }

    #[tokio::test]
    async fn upsert_sends_points_with_payload() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .and(body_partial_json(json!({"points": [{"payload": {"paper_id": "p1", "seq": 0}}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        let pts = vec![ChunkPoint { paper_id: "p1".into(), seq: 0, page: None, vector: vec![0.1; 4] }];
        store(&server).upsert(&pts).await.unwrap();
    }

    #[tokio::test]
    async fn search_parses_hits_and_applies_seq_filter() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .and(body_partial_json(json!({"filter": {"must": [{"key": "seq", "range": {"gte": 1}}]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [
                    {"id": "x", "score": 0.9, "payload": {"paper_id": "p1", "seq": 3, "page": 7}},
                    {"id": "y", "score": 0.5, "payload": {"paper_id": "p2", "seq": 1, "page": 2}}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let hits = store(&server).search(&[0.1; 4], 10, SeqFilter::OnlyBody).await.unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].paper_id, "p1");
        assert_eq!(hits[0].seq, 3);
        assert_eq!(hits[0].page, Some(7));
        assert!(hits[0].score > hits[1].score);
    }

    #[tokio::test]
    async fn delete_paper_filters_on_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/delete"))
            .and(body_partial_json(json!({"filter": {"must": [{"key": "paper_id", "match": {"value": "p1"}}]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1)
            .mount(&server)
            .await;
        store(&server).delete_paper("p1").await.unwrap();
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib search::vector`
Expected: FAIL — types not defined (add `pub mod vector;` to `src/search/mod.rs` first)

- [ ] **Step 4: Implement** — above the tests:

```rust
use anyhow::{bail, Result};
use serde_json::json;

const UPSERT_BATCH: usize = 64;

/// One chunk's embedding, ready for Qdrant. Chunk text stays in SQLite.
#[derive(Debug, Clone)]
pub struct ChunkPoint {
    pub paper_id: String,
    pub seq: i64,
    pub page: Option<i64>,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct VecHit {
    pub paper_id: String,
    pub seq: i64,
    pub page: Option<i64>,
    pub score: f32,
}

/// Restrict semantic search by chunk kind (seq 0 = title+abstract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqFilter {
    All,
    OnlySummary,
    OnlyBody,
}

/// Deterministic point id: UUIDv5 of "paper_id:seq" — re-upserts overwrite.
pub fn point_id(paper_id: &str, seq: i64) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("{paper_id}:{seq}").as_bytes())
        .to_string()
}

/// Qdrant over its REST API (the official crate would pull in the whole
/// tonic/prost gRPC stack for four calls).
pub struct QdrantStore {
    http: reqwest::Client,
    base_url: String,
    collection: String,
    dims: usize,
}

impl QdrantStore {
    pub fn new(base_url: &str, collection: &str, dims: usize) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            collection: collection.to_string(),
            dims,
        })
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/collections/{}{suffix}", self.base_url, self.collection)
    }

    /// Create the collection if missing; verify vector size if present.
    pub async fn ensure_collection(&self) -> Result<()> {
        let resp = self.http.get(self.url("")).send().await?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await?;
            let size = body["result"]["config"]["params"]["vectors"]["size"]
                .as_u64()
                .unwrap_or(0) as usize;
            if size != self.dims {
                bail!(
                    "qdrant collection '{}' has vector size {size} but config dims = {} — \
                     run: xuewen index rebuild --vectors-only",
                    self.collection,
                    self.dims
                );
            }
            return Ok(());
        }
        if resp.status().as_u16() != 404 {
            bail!("qdrant GET collection: {}", resp.status());
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", resp.status());
        }
        Ok(())
    }

    /// Drop and recreate the collection (vector rebuild after a dims change).
    pub async fn recreate_collection(&self) -> Result<()> {
        let resp = self.http.delete(self.url("")).send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            bail!("qdrant delete collection: {}", resp.status());
        }
        let resp = self
            .http
            .put(self.url(""))
            .json(&json!({"vectors": {"size": self.dims, "distance": "Cosine"}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant create collection: {}", resp.status());
        }
        Ok(())
    }

    pub async fn upsert(&self, points: &[ChunkPoint]) -> Result<()> {
        for batch in points.chunks(UPSERT_BATCH) {
            let body = json!({
                "points": batch.iter().map(|p| json!({
                    "id": point_id(&p.paper_id, p.seq),
                    "vector": p.vector,
                    "payload": {"paper_id": p.paper_id, "seq": p.seq, "page": p.page},
                })).collect::<Vec<_>>()
            });
            let resp = self
                .http
                .put(format!("{}?wait=true", self.url("/points")))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                bail!("qdrant upsert: {}", resp.status());
            }
        }
        Ok(())
    }

    pub async fn search(
        &self,
        vector: &[f32],
        limit: usize,
        filter: SeqFilter,
    ) -> Result<Vec<VecHit>> {
        let mut body = json!({"vector": vector, "limit": limit, "with_payload": true});
        match filter {
            SeqFilter::All => {}
            SeqFilter::OnlySummary => {
                body["filter"] = json!({"must": [{"key": "seq", "match": {"value": 0}}]});
            }
            SeqFilter::OnlyBody => {
                body["filter"] = json!({"must": [{"key": "seq", "range": {"gte": 1}}]});
            }
        }
        let resp = self
            .http
            .post(self.url("/points/search"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant search: {}", resp.status());
        }
        let body: serde_json::Value = resp.json().await?;
        let hits = body["result"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| {
                        Some(VecHit {
                            paper_id: h["payload"]["paper_id"].as_str()?.to_string(),
                            seq: h["payload"]["seq"].as_i64()?,
                            page: h["payload"]["page"].as_i64(),
                            score: h["score"].as_f64()? as f32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(hits)
    }

    pub async fn delete_paper(&self, paper_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}?wait=true", self.url("/points/delete")))
            .json(&json!({"filter": {"must": [{"key": "paper_id", "match": {"value": paper_id}}]}}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("qdrant delete: {}", resp.status());
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib search::vector`
Expected: PASS (6 tests)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/search/vector.rs src/search/mod.rs
git commit -m "feat(search): Qdrant REST client with deterministic point ids"
```

---

### Task 10: `SearchService` — open, search, fuse, hydrate, status

**Files:**
- Modify: `src/search/mod.rs` (the service lives here, above the `pub mod` lines)

**Interfaces:**
- Consumes: everything from Tasks 3–9 plus `crate::db`, `crate::config::SearchConfig`.
- Produces:
  - `pub struct SearchRequest { pub q: String, pub fields: fts::FieldSel, pub keyword: bool, pub semantic: bool, pub status: Option<String>, pub project: Option<String> }`
  - `pub struct SemanticState { pub available: bool, pub reason: Option<String> }`
  - `pub struct MatchInfo { pub engine: String /* "keyword"|"semantic"|"both" */, pub field: String, pub snippet: String, pub page: Option<i64> }`
  - `pub struct SearchOutcome { pub semantic: SemanticState, pub results: Vec<(crate::models::Paper, MatchInfo)> }`
  - `pub struct TierCounts { pub indexed: i64, pub pending: i64, pub failed: i64 }`
  - `pub struct IndexStatus { pub fts: TierCounts, pub vectors: TierCounts, pub semantic_available: bool, pub reason: Option<String> }`
  - `pub struct SearchService` with:
    - `pub async fn open(pool: SqlitePool, cfg: &SearchConfig) -> Result<Arc<SearchService>>`
    - `pub fn open_with(pool: SqlitePool, fts: fts::FtsIndex, vectors: vector::QdrantStore, embedder: Option<embedder::Embedder>) -> Arc<SearchService>` — test/DI constructor used by indexer tests and the e2e test
    - `pub fn wake(&self)` and `pub async fn wait_work(&self, tick: std::time::Duration)`
    - `pub async fn search(&self, req: &SearchRequest) -> Result<SearchOutcome>`
    - `pub async fn status(&self) -> Result<IndexStatus>`
    - `pub async fn paper_states(&self) -> Result<Vec<planner::PaperState>>` (shared by search/status/indexer)
    - public fields for the indexer: `pub pool: SqlitePool, pub fts: fts::FtsIndex, pub vectors: vector::QdrantStore, pub embedder: Option<embedder::Embedder>`

- [ ] **Step 1: Write the failing tests** — add a `tests` module at the bottom of `src/search/mod.rs`. These use a temp SQLite pool, a temp Tantivy dir, and wiremock for Qdrant + embeddings:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(id: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    /// Service with keyword tier real (temp Tantivy), semantic unavailable.
    async fn keyword_only_service(pool: sqlx::SqlitePool) -> std::sync::Arc<SearchService> {
        let dir = tempfile::tempdir().unwrap();
        let (fts, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        SearchService::open_with(pool, fts, vectors, None)
    }

    #[tokio::test]
    async fn keyword_search_returns_papers_with_snippets_in_rank_order() {
        let pool = pool().await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Sorting Networks")] {
            crate::db::insert_paper(&pool, &paper(id, title)).await.unwrap();
        }
        let svc = keyword_only_service(pool).await;
        svc.fts.upsert(&fts::PaperDoc {
            id: "a".into(), title: "Fuzzing Firmware".into(), authors: "Ada Lovelace".into(),
            venue: String::new(), abstract_text: String::new(), body: "we fuzz routers".into(),
        }).unwrap();
        svc.fts.upsert(&fts::PaperDoc {
            id: "b".into(), title: "Sorting Networks".into(), authors: "Ada Lovelace".into(),
            venue: String::new(), abstract_text: String::new(), body: "batcher merge".into(),
        }).unwrap();

        let out = svc.search(&SearchRequest {
            q: "fuzzing".into(), fields: fts::FieldSel::all(),
            keyword: true, semantic: true, status: None, project: None,
        }).await.unwrap();

        assert!(!out.semantic.available); // no embedder configured
        assert!(out.semantic.reason.is_some());
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].0.id, "a");
        assert_eq!(out.results[0].1.engine, "keyword");
        assert!(out.results[0].1.snippet.contains("<mark>"));
    }

    #[tokio::test]
    async fn trashed_papers_are_filtered_at_hydration() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware")).await.unwrap();
        crate::db::soft_delete(&pool, "a").await.unwrap();
        let svc = keyword_only_service(pool).await;
        svc.fts.upsert(&fts::PaperDoc {
            id: "a".into(), title: "Fuzzing Firmware".into(), authors: String::new(),
            venue: String::new(), abstract_text: String::new(), body: String::new(),
        }).unwrap();
        let out = svc.search(&SearchRequest {
            q: "fuzzing".into(), fields: fts::FieldSel::all(),
            keyword: true, semantic: false, status: None, project: None,
        }).await.unwrap();
        assert!(out.results.is_empty(), "trashed paper leaked through hydration");
    }

    #[tokio::test]
    async fn hybrid_search_fuses_and_marks_both() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware")).await.unwrap();
        // Chunk for the semantic snippet lookup.
        crate::search::store::replace_chunks(
            &pool, "a",
            &[crate::search::chunker::Chunk { seq: 1, page: Some(7), text: "router fuzz harness details".into() }],
            "hash-a", "mh",
        ).await.unwrap();

        // Wiremock plays both Qdrant and the embedding API.
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [{"id": "x", "score": 0.9, "payload": {"paper_id": "a", "seq": 1, "page": 7}}]
            })))
            .mount(&server).await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        fts_idx.upsert(&fts::PaperDoc {
            id: "a".into(), title: "Fuzzing Firmware".into(), authors: String::new(),
            venue: String::new(), abstract_text: String::new(), body: "we fuzz routers".into(),
        }).unwrap();
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc.search(&SearchRequest {
            q: "fuzzing".into(), fields: fts::FieldSel::all(),
            keyword: true, semantic: true, status: None, project: None,
        }).await.unwrap();

        assert!(out.semantic.available);
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].1.engine, "both");
        assert!(out.results[0].1.snippet.contains("<mark>"), "keyword snippet preferred");
    }

    #[tokio::test]
    async fn semantic_only_hit_uses_chunk_text_snippet() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Some Paper")).await.unwrap();
        crate::search::store::replace_chunks(
            &pool, "a",
            &[crate::search::chunker::Chunk { seq: 2, page: Some(3), text: "novel <escaping> content".into() }],
            "hash-a", "mh",
        ).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [{"id": "x", "score": 0.9, "payload": {"paper_id": "a", "seq": 2, "page": 3}}]
            })))
            .mount(&server).await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc.search(&SearchRequest {
            q: "different words entirely".into(), fields: fts::FieldSel::all(),
            keyword: true, semantic: true, status: None, project: None,
        }).await.unwrap();

        assert_eq!(out.results.len(), 1);
        let m = &out.results[0].1;
        assert_eq!(m.engine, "semantic");
        assert_eq!(m.field, "body");
        assert_eq!(m.page, Some(3));
        assert!(m.snippet.contains("&lt;escaping&gt;"), "chunk text must be HTML-escaped: {}", m.snippet);
    }

    #[tokio::test]
    async fn semantic_failure_degrades_with_reason() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware")).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        fts_idx.upsert(&fts::PaperDoc {
            id: "a".into(), title: "Fuzzing Firmware".into(), authors: String::new(),
            venue: String::new(), abstract_text: String::new(), body: String::new(),
        }).unwrap();
        // Embedder points at a dead port -> semantic path errors.
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc.search(&SearchRequest {
            q: "fuzzing".into(), fields: fts::FieldSel::all(),
            keyword: true, semantic: true, status: None, project: None,
        }).await.unwrap();

        assert!(!out.semantic.available);
        assert!(out.semantic.reason.is_some());
        assert_eq!(out.results.len(), 1, "keyword results still returned");
    }

    #[tokio::test]
    async fn authors_only_selection_skips_semantic() {
        let pool = pool().await;
        let svc = keyword_only_service(pool).await;
        let out = svc.search(&SearchRequest {
            q: "lovelace".into(),
            fields: fts::FieldSel { title: false, authors: true, abstract_text: false, body: false },
            keyword: true, semantic: true, status: None, project: None,
        }).await.unwrap();
        // Semantic was requested but is meaningless for authors-only.
        assert!(!out.semantic.available);
    }

    #[tokio::test]
    async fn status_counts_pending_and_failed() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "T")).await.unwrap();
        let svc = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.pending, 1); // never indexed
        assert_eq!(st.fts.failed, 0);
        assert!(!st.semantic_available);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib "search::tests"`
Expected: FAIL — `SearchService` not defined

- [ ] **Step 3: Implement** — replace the top of `src/search/mod.rs` (keep the `pub mod` lines at the top, then add):

```rust
pub mod chunker;
pub mod embedder;
pub mod fts;
pub mod fusion;
pub mod indexer; // added in Task 11; leave the line out until then
pub mod planner;
pub mod store;
pub mod vector;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;

use crate::config::SearchConfig;
use crate::models::Paper;

const KEYWORD_LIMIT: usize = 100;
const SEMANTIC_LIMIT: usize = 50;
const RRF_K: f32 = 60.0;
const SEMANTIC_SNIPPET_CHARS: usize = 200;

pub struct SearchRequest {
    pub q: String,
    pub fields: fts::FieldSel,
    pub keyword: bool,
    pub semantic: bool,
    pub status: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SemanticState {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchInfo {
    /// "keyword" | "semantic" | "both"
    pub engine: String,
    pub field: String,
    /// HTML-safe (escaped text, <mark> highlights only).
    pub snippet: String,
    pub page: Option<i64>,
}

pub struct SearchOutcome {
    pub semantic: SemanticState,
    pub results: Vec<(Paper, MatchInfo)>,
}

#[derive(Debug, Clone, Copy)]
pub struct TierCounts {
    pub indexed: i64,
    pub pending: i64,
    pub failed: i64,
}

#[derive(Debug, Clone)]
pub struct IndexStatus {
    pub fts: TierCounts,
    pub vectors: TierCounts,
    pub semantic_available: bool,
    pub reason: Option<String>,
}

/// Owns the three search backends. SQLite remains the source of truth;
/// Tantivy and Qdrant are derived and rebuildable.
pub struct SearchService {
    pub pool: SqlitePool,
    pub fts: fts::FtsIndex,
    pub vectors: vector::QdrantStore,
    pub embedder: Option<embedder::Embedder>,
    notify: tokio::sync::Notify,
}

impl SearchService {
    pub async fn open(pool: SqlitePool, cfg: &SearchConfig) -> Result<Arc<Self>> {
        let (fts_idx, created) = fts::FtsIndex::open(&cfg.index_dir)?;
        if created {
            // Fresh/recreated index: force the sweep to refill it from SQLite.
            store::clear_stamps(&pool, true, false).await?;
        }
        let embedder = cfg.embedding.as_ref().and_then(embedder::Embedder::from_config);
        let dims = cfg.embedding.as_ref().map(|e| e.dims).unwrap_or(1536);
        let vectors = vector::QdrantStore::new(&cfg.qdrant_url, &cfg.qdrant_collection, dims)?;
        Ok(Arc::new(Self { pool, fts: fts_idx, vectors, embedder, notify: tokio::sync::Notify::new() }))
    }

    /// Dependency-injection constructor for tests.
    pub fn open_with(
        pool: SqlitePool,
        fts: fts::FtsIndex,
        vectors: vector::QdrantStore,
        embedder: Option<embedder::Embedder>,
    ) -> Arc<Self> {
        Arc::new(Self { pool, fts, vectors, embedder, notify: tokio::sync::Notify::new() })
    }

    /// Nudge the indexer to sweep now (harmless if nothing is stale).
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Wait for a wake() or the periodic tick, whichever comes first.
    pub async fn wait_work(&self, tick: Duration) {
        tokio::select! {
            _ = self.notify.notified() => {}
            _ = tokio::time::sleep(tick) => {}
        }
    }

    fn semantic_config_state(&self) -> SemanticState {
        match &self.embedder {
            Some(_) => SemanticState { available: true, reason: None },
            None => SemanticState {
                available: false,
                reason: Some(
                    "embedding API not configured (set [search.embedding] and an API key)".into(),
                ),
            },
        }
    }

    pub async fn search(&self, req: &SearchRequest) -> Result<SearchOutcome> {
        let q = req.q.trim();
        let mut semantic = self.semantic_config_state();
        if req.fields.authors_only() && semantic.available {
            semantic = SemanticState {
                available: false,
                reason: Some("semantic search does not apply to an authors-only query".into()),
            };
        }

        let keyword_hits = if req.keyword {
            self.fts.search(q, &req.fields, KEYWORD_LIMIT)?
        } else {
            Vec::new()
        };

        // Best chunk per paper, in Qdrant score order.
        let mut semantic_best: Vec<vector::VecHit> = Vec::new();
        if req.semantic && semantic.available && !q.is_empty() {
            match self.semantic_search(q, &req.fields).await {
                Ok(hits) => {
                    let mut seen = std::collections::HashSet::new();
                    for h in hits {
                        if seen.insert(h.paper_id.clone()) {
                            semantic_best.push(h);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("semantic search failed: {e}");
                    semantic = SemanticState { available: false, reason: Some(e.to_string()) };
                }
            }
        }

        let keyword_ids: Vec<String> = keyword_hits.iter().map(|h| h.paper_id.clone()).collect();
        let semantic_ids: Vec<String> = semantic_best.iter().map(|h| h.paper_id.clone()).collect();
        let fused: Vec<String> = match (keyword_ids.is_empty(), semantic_ids.is_empty()) {
            (false, true) => keyword_ids.clone(),
            (true, false) => semantic_ids.clone(),
            _ => fusion::rrf(&[keyword_ids.clone(), semantic_ids.clone()], RRF_K)
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
        };

        let papers = store::papers_by_ids_ordered(
            &self.pool,
            &fused,
            req.status.as_deref(),
            req.project.as_deref(),
        )
        .await?;

        let kw_by_id: std::collections::HashMap<&str, &fts::FtsHit> =
            keyword_hits.iter().map(|h| (h.paper_id.as_str(), h)).collect();
        let sem_by_id: std::collections::HashMap<&str, &vector::VecHit> =
            semantic_best.iter().map(|h| (h.paper_id.as_str(), h)).collect();

        let mut results = Vec::with_capacity(papers.len());
        for p in papers {
            let kw = kw_by_id.get(p.id.as_str());
            let sem = sem_by_id.get(p.id.as_str());
            let info = match (kw, sem) {
                (Some(k), Some(_)) => MatchInfo {
                    engine: "both".into(),
                    field: k.field.clone(),
                    snippet: k.snippet_html.clone(),
                    page: None,
                },
                (Some(k), None) => MatchInfo {
                    engine: "keyword".into(),
                    field: k.field.clone(),
                    snippet: k.snippet_html.clone(),
                    page: None,
                },
                (None, Some(s)) => self.semantic_match_info(s).await,
                (None, None) => continue, // cannot happen: fused ⊆ union
            };
            results.push((p, info));
        }
        Ok(SearchOutcome { semantic, results })
    }

    async fn semantic_search(&self, q: &str, sel: &fts::FieldSel) -> Result<Vec<vector::VecHit>> {
        let embedder = self.embedder.as_ref().expect("caller checked availability");
        let vecs = embedder.embed(&[q.to_string()]).await?;
        let filter = match (sel.title || sel.abstract_text, sel.body) {
            (true, true) => vector::SeqFilter::All,
            (false, true) => vector::SeqFilter::OnlyBody,
            (true, false) => vector::SeqFilter::OnlySummary,
            (false, false) => vector::SeqFilter::All, // authors-only never reaches here
        };
        self.vectors.search(&vecs[0], SEMANTIC_LIMIT, filter).await
    }

    /// Snippet for a semantic-only hit: the matching chunk's text (escaped, trimmed).
    async fn semantic_match_info(&self, hit: &vector::VecHit) -> MatchInfo {
        let (field, page) = if hit.seq == 0 { ("abstract", None) } else { ("body", hit.page) };
        let text = store::chunk_text(&self.pool, &hit.paper_id, hit.seq)
            .await
            .ok()
            .flatten()
            .map(|c| c.text)
            .unwrap_or_default();
        let trimmed: String = text.chars().take(SEMANTIC_SNIPPET_CHARS).collect();
        let ellipsis = if text.chars().count() > SEMANTIC_SNIPPET_CHARS { "…" } else { "" };
        MatchInfo {
            engine: "semantic".into(),
            field: field.into(),
            snippet: format!("{}{}", fts::html_escape(&trimmed), ellipsis),
            page,
        }
    }

    /// Live papers as planner input (meta hashes computed here).
    pub async fn paper_states(&self) -> Result<Vec<planner::PaperState>> {
        let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers")
            .fetch_all(&self.pool)
            .await?;
        Ok(papers
            .iter()
            .map(|p| planner::PaperState {
                id: p.id.clone(),
                content_hash: p.content_hash.clone(),
                meta_hash: store::meta_hash(p),
                trashed: p.deleted_at.is_some(),
            })
            .collect())
    }

    pub async fn status(&self) -> Result<IndexStatus> {
        let papers = self.paper_states().await?;
        let rows = store::all_index_rows(&self.pool).await?;
        let plan = planner::plan(
            &papers,
            &rows,
            self.embedder.as_ref().map(|e| e.model()),
            chrono::Utc::now(),
        );
        let live = papers.iter().filter(|p| !p.trashed).count() as i64;
        let failed = rows.iter().filter(|r| r.last_error.is_some()).count() as i64;
        let fts_pending = plan.index.iter().filter(|w| w.fts).count() as i64;
        let vec_pending = plan.index.iter().filter(|w| w.vectors).count() as i64;
        let sem = self.semantic_config_state();
        Ok(IndexStatus {
            fts: TierCounts { indexed: live - fts_pending, pending: fts_pending, failed },
            vectors: TierCounts { indexed: live - vec_pending, pending: vec_pending, failed },
            semantic_available: sem.available,
            reason: sem.reason,
        })
    }
}
```

Note: leave `pub mod indexer;` OUT of the module list until Task 11 creates the file, or `cargo` will fail to compile.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib "search::"`
Expected: PASS (all search module tests so far)

- [ ] **Step 5: Commit**

```bash
git add src/search/mod.rs
git commit -m "feat(search): SearchService with hybrid search, RRF fusion, and status"
```

---

### Task 11: Background indexer

**Files:**
- Create: `src/search/indexer.rs`
- Modify: `src/search/mod.rs` (add `pub mod indexer;`)

**Interfaces:**
- Consumes: `SearchService` (public fields), `planner`, `store`, `chunker`, `crate::pdf::extract_text_all`, `crate::db`.
- Produces:
  - `#[derive(Debug, Default)] pub struct SweepSummary { pub indexed: usize, pub deindexed: usize, pub failed: usize }`
  - `pub async fn sweep(svc: &SearchService) -> Result<SweepSummary>` — one full pass: process every tombstone and every stale paper.
  - `pub async fn run(svc: Arc<SearchService>, tick: Duration)` — loop `sweep` → `wait_work(tick)` forever.

- [ ] **Step 1: Write the failing tests** — bottom of `src/search/indexer.rs`. Papers need real PDFs on disk for extraction; write them with `printpdf` like `src/watcher.rs` tests do:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};
    use crate::search::{embedder, fts, store, vector, SearchService};
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use serde_json::json;
    use std::io::BufWriter;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_pdf(path: &Path, line: &str) {
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap())).unwrap();
    }

    struct Fixture {
        svc: std::sync::Arc<SearchService>,
        library_root: std::path::PathBuf,
        _dirs: Vec<tempfile::TempDir>,
    }

    /// Temp SQLite + temp Tantivy + wiremock Qdrant/embeddings (when given).
    async fn fixture(server: Option<&MockServer>) -> Fixture {
        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let idx_dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx_dir.path()).unwrap();
        let lib_dir = tempfile::tempdir().unwrap();
        let library_root = lib_dir.path().to_path_buf();
        let (vectors, embed) = match server {
            Some(s) => (
                vector::QdrantStore::new(&s.uri(), "xuewen", 4).unwrap(),
                Some(embedder::Embedder::for_tests(&format!("{}/v1", s.uri()), "m1", 4)),
            ),
            None => (vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(), None),
        };
        let svc = SearchService::open_with(pool, fts_idx, vectors, embed);
        Fixture { svc, library_root, _dirs: vec![db_dir, idx_dir, lib_dir] }
    }

    async fn insert_paper_with_pdf(f: &Fixture, id: &str, title: &str, body_line: &str) {
        let rel = format!("{id}.pdf");
        write_pdf(&f.library_root.join(&rel), body_line);
        let p = Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: rel,
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: Some("An abstract.".into()),
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        };
        crate::db::insert_paper(&f.svc.pool, &p).await.unwrap();
    }

    #[tokio::test]
    async fn sweep_indexes_fts_even_without_embedder() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body mentions dictionaries").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);

        let hits = f.svc.fts.search("dictionaries", &fts::FieldSel::all(), 10).unwrap();
        assert_eq!(hits.len(), 1, "body text searchable after sweep");
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some());
        assert!(rows[0].vectors_indexed_at.is_none(), "no embedder -> no vector stamp");

        // Second sweep is a no-op.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
    }

    #[tokio::test]
    async fn sweep_embeds_and_upserts_vectors_when_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
                let data: Vec<_> = (0..n)
                    .map(|i| json!({"index": i, "embedding": [0.1, 0.2, 0.3, 0.4]}))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data": data}))
            })
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1..)
            .mount(&server).await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].vectors_indexed_at.is_some());
        assert_eq!(rows[0].embed_model.as_deref(), Some("m1"));
    }

    #[tokio::test]
    async fn embedding_failure_keeps_fts_and_records_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server).await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some(), "FTS tier survived");
        assert!(rows[0].vectors_indexed_at.is_none());
        assert!(rows[0].last_error.is_some());
        assert_eq!(rows[0].attempts, 1);
    }

    #[tokio::test]
    async fn trashed_paper_is_deindexed_everywhere() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        assert_eq!(f.svc.fts.search("fuzzing", &fts::FieldSel::all(), 10).unwrap().len(), 1);

        crate::db::soft_delete(&f.svc.pool, "p1").await.unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1);
        assert!(f.svc.fts.search("fuzzing", &fts::FieldSel::all(), 10).unwrap().is_empty());
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(store::chunks_for_paper(&f.svc.pool, "p1").await.unwrap().is_empty());
        // Qdrant delete for a no-embedder service is skipped, not an error.
    }

    // Helper used by every test: sweep against the fixture's library root.
    async fn sweep_in(f: &Fixture) -> anyhow::Result<SweepSummary> {
        sweep(&f.svc, &f.library_root).await
    }
}
```

Note the signature discovered by the tests: `sweep` needs the library root to resolve `rel_path` → PDF path. Final signatures:
- `pub async fn sweep(svc: &SearchService, library_root: &Path) -> Result<SweepSummary>`
- `pub async fn run(svc: Arc<SearchService>, library_root: PathBuf, tick: Duration)`

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib search::indexer`
Expected: FAIL — `sweep` not defined (add `pub mod indexer;` to `src/search/mod.rs` first)

- [ ] **Step 3: Implement** — above the tests in `src/search/indexer.rs`:

```rust
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::search::{chunker, fts, planner, store, vector, SearchService};

#[derive(Debug, Default)]
pub struct SweepSummary {
    pub indexed: usize,
    pub deindexed: usize,
    pub failed: usize,
}

/// One full pass: remove tombstones, (re)index every stale paper.
/// Individual paper failures are recorded (with backoff) and never abort
/// the sweep.
pub async fn sweep(svc: &SearchService, library_root: &Path) -> Result<SweepSummary> {
    let papers = svc.paper_states().await?;
    let rows = store::all_index_rows(&svc.pool).await?;
    let plan = planner::plan(
        &papers,
        &rows,
        svc.embedder.as_ref().map(|e| e.model()),
        chrono::Utc::now(),
    );
    let mut summary = SweepSummary::default();

    for paper_id in &plan.deindex {
        match deindex_paper(svc, paper_id).await {
            Ok(()) => summary.deindexed += 1,
            Err(e) => {
                tracing::warn!("deindex {paper_id}: {e}");
                summary.failed += 1;
            }
        }
    }
    for work in &plan.index {
        match index_paper(svc, library_root, work).await {
            Ok(()) => summary.indexed += 1,
            Err(e) => {
                tracing::warn!("index {}: {e}", work.paper_id);
                store::record_error(&svc.pool, &work.paper_id, &e.to_string())
                    .await
                    .ok();
                summary.failed += 1;
            }
        }
    }
    Ok(summary)
}

async fn index_paper(svc: &SearchService, library_root: &Path, work: &planner::Work) -> Result<()> {
    let Some(paper) = crate::db::get_by_id(&svc.pool, &work.paper_id).await? else {
        return Ok(()); // purged since the plan was computed; tombstone next sweep
    };

    let chunks = if work.fts {
        // Full re-extract + re-chunk + Tantivy doc.
        let pdf_path = library_root.join(&paper.rel_path);
        let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf_path))
            .await
            .context("pdftotext task panicked")??;
        let chunks = chunker::chunk_paper(
            paper.meta.title.as_deref(),
            paper.meta.abstract_text.as_deref(),
            &text,
        );
        store::replace_chunks(
            &svc.pool,
            &paper.id,
            &chunks,
            &paper.content_hash,
            &store::meta_hash(&paper),
        )
        .await?;
        let body: String = chunks
            .iter()
            .filter(|c| c.seq >= 1)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        svc.fts.upsert(&fts::PaperDoc {
            id: paper.id.clone(),
            title: paper.meta.title.clone().unwrap_or_default(),
            authors: paper.meta.authors.0.join(" ; "),
            venue: paper.meta.venue.clone().unwrap_or_default(),
            abstract_text: paper.meta.abstract_text.clone().unwrap_or_default(),
            body,
        })?;
        store::mark_fts_done(&svc.pool, &paper.id).await?;
        chunks
    } else {
        store::chunks_for_paper(&svc.pool, &paper.id).await?
    };

    if work.vectors {
        let Some(embedder) = &svc.embedder else {
            return Ok(()); // planner only schedules vectors when configured
        };
        if !chunks.is_empty() {
            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            let vectors = embedder.embed(&texts).await?;
            svc.vectors.ensure_collection().await?;
            let points: Vec<vector::ChunkPoint> = chunks
                .iter()
                .zip(vectors)
                .map(|(c, v)| vector::ChunkPoint {
                    paper_id: paper.id.clone(),
                    seq: c.seq,
                    page: c.page,
                    vector: v,
                })
                .collect();
            svc.vectors.upsert(&points).await?;
        }
        store::mark_vectors_done(&svc.pool, &paper.id, embedder_model(svc)).await?;
    }
    Ok(())
}

fn embedder_model(svc: &SearchService) -> &str {
    svc.embedder.as_ref().map(|e| e.model()).unwrap_or_default()
}

async fn deindex_paper(svc: &SearchService, paper_id: &str) -> Result<()> {
    svc.fts.delete(paper_id)?;
    if svc.embedder.is_some() {
        // Qdrant cleanup only matters when vectors were ever written; a dead
        // Qdrant here must not wedge the tombstone forever.
        if let Err(e) = svc.vectors.delete_paper(paper_id).await {
            tracing::warn!("qdrant delete {paper_id}: {e} (index row removed anyway; \
                            orphan points are overwritten if the paper returns)");
        }
    }
    store::remove_index_entry(&svc.pool, paper_id).await?;
    Ok(())
}

/// Indexer loop: sweep, then sleep until woken or the tick elapses.
pub async fn run(svc: Arc<SearchService>, library_root: PathBuf, tick: Duration) {
    loop {
        match sweep(&svc, &library_root).await {
            Ok(s) if s.indexed + s.deindexed + s.failed > 0 => {
                tracing::info!(
                    "search index sweep: {} indexed, {} removed, {} failed",
                    s.indexed,
                    s.deindexed,
                    s.failed
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("search index sweep failed: {e}"),
        }
        svc.wait_work(tick).await;
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib search::indexer`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/search/indexer.rs src/search/mod.rs
git commit -m "feat(search): background indexer sweep with per-tier recovery"
```

---

### Task 12: Web API — `/api/search`, `/api/search/status`

**Files:**
- Modify: `src/web/dto.rs` (search DTOs)
- Modify: `src/web/api.rs` (two handlers + wake calls)
- Modify: `src/web/mod.rs` (AppState field, routes, `serve` signature)

**Interfaces:**
- Consumes: `SearchService::{search, status, wake}`, `FieldSel::parse`.
- Produces:
  - `AppState.search: Option<Arc<crate::search::SearchService>>` and `impl AppState { pub fn wake_search(&self) }`
  - `pub fn build_router_with_search(pool: SqlitePool, library_root: PathBuf, search: Arc<SearchService>) -> Router` (test helper; existing builders set `search: None`)
  - `web::serve(...)` gains a final `search: Option<Arc<SearchService>>` parameter.
  - `GET /api/search?q&fields&engines&status&project` → `{ semantic: {available, reason}, results: [{paper: PaperSummary, match: {engine, field, snippet, page}}] }`
  - `GET /api/search/status` → `{ fts: {indexed, pending, failed}, vectors: {...}, semantic_available, reason }`
  - Both return `503 {"error": "search not configured"}` when `AppState.search` is `None`.

- [ ] **Step 1: Write the failing tests** — append to `tests/web_test.rs` (follow its existing `axum_test::TestServer` setup helpers):

```rust
mod search_api {
    use super::*; // reuse the file's pool/server helpers
    use xuewen::search::{fts, vector, SearchService};

    async fn server_with_search(pool: sqlx::SqlitePool) -> axum_test::TestServer {
        let idx = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx.path()).unwrap();
        std::mem::forget(idx);
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        let svc = SearchService::open_with(pool.clone(), fts_idx, vectors, None);
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "p1".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "router dictionaries".into(),
            })
            .unwrap();
        let router = xuewen::web::build_router_with_search(
            pool,
            std::path::PathBuf::from("/nonexistent"),
            svc,
        );
        axum_test::TestServer::new(router).unwrap()
    }

    #[tokio::test]
    async fn search_returns_papers_with_match_info() {
        let pool = test_pool().await; // the file's existing helper
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await; // existing helper or add one
        let server = server_with_search(pool).await;

        let resp = server.get("/api/search").add_query_param("q", "fuzzing").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["semantic"]["available"], false);
        assert_eq!(body["results"][0]["paper"]["id"], "p1");
        assert_eq!(body["results"][0]["match"]["engine"], "keyword");
        assert!(body["results"][0]["match"]["snippet"].as_str().unwrap().contains("<mark>"));
    }

    #[tokio::test]
    async fn fields_param_restricts_search() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "dictionaries")
            .add_query_param("fields", "title")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn status_reports_tiers() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server.get("/api/search/status").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert!(body["fts"]["pending"].as_i64().unwrap() >= 1);
        assert_eq!(body["semantic_available"], false);
    }

    #[tokio::test]
    async fn search_without_service_is_503() {
        let pool = test_pool().await;
        let router = xuewen::web::build_router(pool, std::path::PathBuf::from("/nonexistent"));
        let server = axum_test::TestServer::new(router).unwrap();
        let resp = server.get("/api/search").add_query_param("q", "x").await;
        resp.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
```

If `tests/web_test.rs` lacks an `insert_sample_paper` helper, add one using the `Paper` construction pattern from `src/db.rs` tests (id + content_hash + title, `status: Resolved`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test web_test search_api`
Expected: FAIL — `build_router_with_search` not found

- [ ] **Step 3: Implement DTOs** — append to `src/web/dto.rs`:

```rust
/// Why a paper matched a search query.
#[derive(Serialize)]
pub struct SearchMatch {
    pub engine: String,
    pub field: String,
    /// HTML-safe: escaped text with <mark> highlights only.
    pub snippet: String,
    pub page: Option<i64>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub paper: PaperSummary,
    #[serde(rename = "match")]
    pub match_info: SearchMatch,
}

#[derive(Serialize)]
pub struct SemanticAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub semantic: SemanticAvailability,
    pub results: Vec<SearchResult>,
}

#[derive(Serialize)]
pub struct TierCounts {
    pub indexed: i64,
    pub pending: i64,
    pub failed: i64,
}

#[derive(Serialize)]
pub struct SearchStatus {
    pub fts: TierCounts,
    pub vectors: TierCounts,
    pub semantic_available: bool,
    pub reason: Option<String>,
}
```

- [ ] **Step 4: Implement handlers** — append to `src/web/api.rs` (imports: add `use crate::search::fts::FieldSel;` and the new DTOs to the existing `use super::dto::{...}` line):

```rust
#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub fields: Option<String>,
    pub engines: Option<String>,
    pub status: Option<String>,
    pub project: Option<String>,
}

/// Hybrid search. `fields`/`engines` are CSV lists; absent or unknown-only
/// values fall back to "all" (mirrors the whitelisting style elsewhere).
pub async fn search_papers(State(app): State<AppState>, Query(p): Query<SearchParams>) -> Response {
    let Some(svc) = &app.search else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "search not configured"})),
        )
            .into_response();
    };
    let (keyword, semantic) = parse_engines(p.engines.as_deref());
    let req = crate::search::SearchRequest {
        q: p.q.unwrap_or_default(),
        fields: FieldSel::parse(p.fields.as_deref()),
        keyword,
        semantic,
        status: p.status,
        project: p.project,
    };
    match svc.search(&req).await {
        Ok(out) => {
            let results: Vec<super::dto::SearchResult> = out
                .results
                .iter()
                .map(|(paper, m)| super::dto::SearchResult {
                    paper: super::dto::PaperSummary::from(paper),
                    match_info: super::dto::SearchMatch {
                        engine: m.engine.clone(),
                        field: m.field.clone(),
                        snippet: m.snippet.clone(),
                        page: m.page,
                    },
                })
                .collect();
            Json(super::dto::SearchResponse {
                semantic: super::dto::SemanticAvailability {
                    available: out.semantic.available,
                    reason: out.semantic.reason,
                },
                results,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("search: {e}");
            internal_error()
        }
    }
}

fn parse_engines(csv: Option<&str>) -> (bool, bool) {
    let (mut keyword, mut semantic) = (false, false);
    for part in csv.unwrap_or("").split(',').map(str::trim) {
        match part {
            "keyword" => keyword = true,
            "semantic" => semantic = true,
            _ => {}
        }
    }
    if keyword || semantic {
        (keyword, semantic)
    } else {
        (true, true) // absent/unknown-only -> both
    }
}

pub async fn search_status(State(app): State<AppState>) -> Response {
    let Some(svc) = &app.search else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "search not configured"})),
        )
            .into_response();
    };
    match svc.status().await {
        Ok(st) => Json(super::dto::SearchStatus {
            fts: super::dto::TierCounts {
                indexed: st.fts.indexed,
                pending: st.fts.pending,
                failed: st.fts.failed,
            },
            vectors: super::dto::TierCounts {
                indexed: st.vectors.indexed,
                pending: st.vectors.pending,
                failed: st.vectors.failed,
            },
            semantic_available: st.semantic_available,
            reason: st.reason,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("search status: {e}");
            internal_error()
        }
    }
}
```

Wake the indexer after successful mutations — in the same file:
- in `import_paper` and `import_url`: after the `stage_and_ingest(...)` call returns, add `app.wake_search();` before returning the response (bind the response first: `let resp = stage_and_ingest(...).await; app.wake_search(); resp`).
- in `identify_paper`: after a successful apply (the `IdentifyOutcome::Applied` arm), add `app.wake_search();`.
- in `delete_paper`: in the `Ok(_)` arm after `soft_delete` succeeds, add `app.wake_search();`.

- [ ] **Step 5: Wire state and routes** — in `src/web/mod.rs`:

```rust
// AppState gains:
    /// Present when a search index/service was opened (serve). `None` in
    /// read-only test routers -> /api/search answers 503.
    pub search: Option<Arc<crate::search::SearchService>>,

impl AppState {
    /// Nudge the background indexer after a mutation. No-op without search.
    pub fn wake_search(&self) {
        if let Some(s) = &self.search {
            s.wake();
        }
    }
}
```

Set `search: None` in `build_router`, `build_router_with_ingest`, and `build_router_with_ingest_proxy`. Add:

```rust
/// Read-only router plus a live search service. Used by tests.
pub fn build_router_with_search(
    pool: SqlitePool,
    library_root: PathBuf,
    search: Arc<crate::search::SearchService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: Some(search),
    })
}
```

Routes in `router_with`:

```rust
        .route("/api/search", get(api::search_papers))
        .route("/api/search/status", get(api::search_status))
```

`serve` gains the parameter and threads it through:

```rust
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
    search: Option<Arc<crate::search::SearchService>>,
) -> Result<()> {
    let app = router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
        proxy_login_url,
        search,
    });
    // ...bind/listen exactly as before...
}
```

(`build_router_with_ingest_proxy` stays for existing tests; `serve` now builds its state directly.) `main.rs` will not compile until Task 13 updates the `web::serve` call — do Tasks 12 and 13 in one sitting, or temporarily pass `None` in `main.rs` now.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --test web_test` (temporarily add `, None` to the `web::serve` call in `src/main.rs` if the build needs it)
Expected: PASS (new `search_api` tests + all existing web tests)

- [ ] **Step 7: Commit**

```bash
git add src/web src/main.rs tests/web_test.rs
git commit -m "feat(web): /api/search and /api/search/status endpoints"
```

---

### Task 13: CLI (`xuewen index`, `xuewen search`) + serve/watch wiring

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `SearchService::{open, search, status}`, `indexer::{run, sweep}`, `store::clear_stamps`, `QdrantStore::recreate_collection`, `FieldSel::parse`.
- Produces CLI:
  - `xuewen index status`
  - `xuewen index rebuild [--fts-only | --vectors-only]`
  - `xuewen search <QUERY> [--fields title,authors,abstract,body] [--keyword-only | --semantic-only]`
  - `Serve` and `Watch` spawn `indexer::run(svc, library_root, 30s)`.

- [ ] **Step 1: Add the subcommands.** In the `Command` enum:

```rust
    /// Search the library from the terminal.
    Search {
        query: String,
        /// Comma-separated fields: title,authors,abstract,body (default all).
        #[arg(long)]
        fields: Option<String>,
        /// Keyword (BM25) engine only.
        #[arg(long, conflicts_with = "semantic_only")]
        keyword_only: bool,
        /// Semantic (embedding) engine only.
        #[arg(long)]
        semantic_only: bool,
    },
    /// Inspect or rebuild the search indexes.
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
```

And below the other subcommand enums:

```rust
#[derive(Subcommand)]
enum IndexCmd {
    /// Show per-tier indexing counts.
    Status,
    /// Drop and re-derive the search indexes from SQLite + PDFs.
    Rebuild {
        /// Rebuild only the Tantivy full-text index.
        #[arg(long, conflicts_with = "vectors_only")]
        fts_only: bool,
        /// Rebuild only the Qdrant vectors (recreates the collection).
        #[arg(long)]
        vectors_only: bool,
    },
}
```

- [ ] **Step 2: Implement the match arms.** Add to the `match cli.command` block (imports at top: `use xuewen::search::{indexer, SearchService};` plus `use xuewen::search::fts::FieldSel;`):

```rust
        Command::Search {
            query,
            fields,
            keyword_only,
            semantic_only,
        } => {
            let svc = SearchService::open(pool.clone(), &cfg.search).await?;
            let req = xuewen::search::SearchRequest {
                q: query,
                fields: FieldSel::parse(fields.as_deref()),
                keyword: !semantic_only,
                semantic: !keyword_only,
                status: None,
                project: None,
            };
            let out = svc.search(&req).await?;
            if let Some(reason) = &out.semantic.reason {
                if !keyword_only {
                    eprintln!("note: semantic search unavailable — {reason}");
                }
            }
            if out.results.is_empty() {
                println!("no matches");
            }
            for (i, (p, m)) in out.results.iter().enumerate() {
                let label = p.cite_key.as_deref().unwrap_or(&p.id);
                println!(
                    "{:2}. {}  {}",
                    i + 1,
                    label,
                    p.meta.title.as_deref().unwrap_or("(untitled)")
                );
                let loc = match m.page {
                    Some(pg) => format!("{} p.{pg}", m.field),
                    None => m.field.clone(),
                };
                println!("      [{loc}] {}", strip_snippet_html(&m.snippet));
            }
        }
        Command::Index { cmd } => match cmd {
            IndexCmd::Status => {
                let svc = SearchService::open(pool.clone(), &cfg.search).await?;
                let st = svc.status().await?;
                println!(
                    "full-text: {} indexed, {} pending, {} failed",
                    st.fts.indexed, st.fts.pending, st.fts.failed
                );
                println!(
                    "vectors:   {} indexed, {} pending, {} failed",
                    st.vectors.indexed, st.vectors.pending, st.vectors.failed
                );
                match st.reason {
                    None => println!("semantic search: available"),
                    Some(r) => println!("semantic search: unavailable — {r}"),
                }
            }
            IndexCmd::Rebuild {
                fts_only,
                vectors_only,
            } => {
                let do_fts = !vectors_only;
                let do_vectors = !fts_only;
                if do_fts {
                    // Wipe before opening: SearchService::open detects the
                    // fresh directory and clears the FTS stamps itself.
                    let _ = std::fs::remove_dir_all(&cfg.search.index_dir);
                }
                let svc = SearchService::open(pool.clone(), &cfg.search).await?;
                xuewen::search::store::clear_stamps(&pool, do_fts, do_vectors).await?;
                if do_vectors && svc.embedder.is_some() {
                    svc.vectors.recreate_collection().await?;
                }
                let s = indexer::sweep(&svc, &cfg.library_root).await?;
                println!(
                    "rebuild: {} indexed, {} removed, {} failed",
                    s.indexed, s.deindexed, s.failed
                );
                if s.failed > 0 {
                    anyhow::bail!("some papers failed to index — see the log; re-run to retry");
                }
            }
        },
```

Add the snippet-stripping helper next to `author_line`:

```rust
/// Terminal output: drop <mark> tags and undo the snippet's HTML escaping.
fn strip_snippet_html(s: &str) -> String {
    s.replace("<mark>", "")
        .replace("</mark>", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}
```

- [ ] **Step 3: Spawn the indexer in Serve and Watch.** In the `Command::Serve` arm, before `web::serve(...)`:

```rust
            let search = match SearchService::open(pool.clone(), &cfg.search).await {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("search disabled: {e}");
                    None
                }
            };
            if let Some(s) = &search {
                tokio::spawn(indexer::run(
                    s.clone(),
                    cfg.library_root.clone(),
                    std::time::Duration::from_secs(30),
                ));
            }
```

and pass `search` as the new last argument of `web::serve(...)` (replace the temporary `None` from Task 12 if present).

In the `Command::Watch` arm, before `watcher::run`:

```rust
            match SearchService::open(pool.clone(), &cfg.search).await {
                Ok(s) => {
                    tokio::spawn(indexer::run(
                        s,
                        cfg.library_root.clone(),
                        std::time::Duration::from_secs(30),
                    ));
                }
                Err(e) => tracing::warn!("search indexing disabled: {e}"),
            }
```

- [ ] **Step 4: Verify by hand**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

Run: `./target/debug/xuewen index status`
Expected: three lines of counts; `semantic search: unavailable — embedding API not configured…` (no key set). With the 3 existing library papers: `full-text: 0 indexed, 3 pending, 0 failed`.

Run: `./target/debug/xuewen index rebuild --fts-only && ./target/debug/xuewen search "fuzzing"`
Expected: rebuild reports `3 indexed`; search prints ranked results with `[body p.N]`-style snippets.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): xuewen search and xuewen index; indexer runs with serve/watch"
```

---

### Task 14: Frontend — types and API client

**Files:**
- Modify: `frontend/src/lib/types.ts`
- Modify: `frontend/src/lib/api.ts`
- Create: `frontend/src/lib/search.test.ts`

**Interfaces:**
- Produces (types):
  - `export interface SearchOpts { title: boolean; authors: boolean; abstract: boolean; body: boolean; keyword: boolean; semantic: boolean }`
  - `export interface SearchMatch { engine: 'keyword' | 'semantic' | 'both'; field: string; snippet: string; page: number | null }`
  - `export interface SearchResultItem { paper: PaperSummary; match: SearchMatch }`
  - `export interface SearchResponse { semantic: { available: boolean; reason: string | null }; results: SearchResultItem[] }`
  - `export interface TierCounts { indexed: number; pending: number; failed: number }`
  - `export interface SearchStatus { fts: TierCounts; vectors: TierCounts; semantic_available: boolean; reason: string | null }`
- Produces (api):
  - `export function searchParams(q: string, opts: SearchOpts, f: Filters, keywordOnly?: boolean): URLSearchParams` (exported pure helper so it is unit-testable)
  - `export async function searchPapers(q, opts, f, keywordOnly?): Promise<SearchResponse>` → `GET /api/search?...`
  - `export async function getSearchStatus(): Promise<SearchStatus>` → `GET /api/search/status`

- [ ] **Step 1: Write the failing test** — `frontend/src/lib/search.test.ts`:

```typescript
import { describe, expect, it } from 'vitest';
import { searchParams } from './api';
import type { Filters, SearchOpts } from './types';

const allOpts: SearchOpts = {
  title: true,
  authors: true,
  abstract: true,
  body: true,
  keyword: true,
  semantic: true,
};
const filters: Filters = { q: '', status: 'all', sort: 'year_desc', project: 'all' };

describe('searchParams', () => {
  it('omits fields/engines when everything is selected', () => {
    const p = searchParams('fuzzing', allOpts, filters);
    expect(p.get('q')).toBe('fuzzing');
    expect(p.get('fields')).toBeNull();
    expect(p.get('engines')).toBeNull();
    expect(p.get('project')).toBeNull();
    expect(p.get('status')).toBeNull();
  });

  it('lists only the selected fields and engines', () => {
    const p = searchParams(
      'x',
      { ...allOpts, title: false, abstract: false, semantic: false },
      filters,
    );
    expect(p.get('fields')).toBe('authors,body');
    expect(p.get('engines')).toBe('keyword');
  });

  it('keywordOnly overrides the engine selection', () => {
    const p = searchParams('x', allOpts, filters, true);
    expect(p.get('engines')).toBe('keyword');
  });

  it('carries status and project filters', () => {
    const p = searchParams('x', allOpts, { ...filters, status: 'resolved', project: 'proj1' });
    expect(p.get('status')).toBe('resolved');
    expect(p.get('project')).toBe('proj1');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npm test`
Expected: FAIL — `searchParams` is not exported

- [ ] **Step 3: Implement.** Append the interfaces from the block above to `frontend/src/lib/types.ts` verbatim. Append to `frontend/src/lib/api.ts` (and add `SearchOpts`, `SearchResponse`, `SearchStatus` to the type import list):

```typescript
/// Query string for /api/search. Omits fields/engines when everything is
/// selected (the server default), so URLs stay short and cacheable.
export function searchParams(
  q: string,
  opts: SearchOpts,
  f: Filters,
  keywordOnly = false,
): URLSearchParams {
  const params = new URLSearchParams();
  params.set('q', q);
  const fields = (['title', 'authors', 'abstract', 'body'] as const).filter((k) => opts[k]);
  if (fields.length > 0 && fields.length < 4) params.set('fields', fields.join(','));
  const engines = keywordOnly
    ? ['keyword']
    : (['keyword', 'semantic'] as const).filter((k) => opts[k]);
  if (engines.length > 0 && engines.length < 2) params.set('engines', engines.join(','));
  if (f.status !== 'all') params.set('status', f.status);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  return params;
}

export async function searchPapers(
  q: string,
  opts: SearchOpts,
  f: Filters,
  keywordOnly = false,
): Promise<SearchResponse> {
  const res = await fetch(`/api/search?${searchParams(q, opts, f, keywordOnly).toString()}`);
  if (!res.ok) throw new Error(`search failed: ${res.status}`);
  return res.json();
}

export async function getSearchStatus(): Promise<SearchStatus> {
  const res = await fetch('/api/search/status');
  if (!res.ok) throw new Error(`search status failed: ${res.status}`);
  return res.json();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npm test`
Expected: PASS (new + existing suites)

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/types.ts frontend/src/lib/api.ts frontend/src/lib/search.test.ts
git commit -m "feat(web): search types and api client"
```

---

### Task 15: Frontend — search state, toggle chips, snippets

**Files:**
- Modify: `frontend/src/lib/state.svelte.ts`
- Modify: `frontend/src/components/Sidebar.svelte`
- Modify: `frontend/src/components/PaperRow.svelte`
- Modify: `frontend/src/App.svelte` (call `loadSearchStatus()` in `onMount`)
- Create: `frontend/src/lib/searchState.test.ts`

**Interfaces:**
- Consumes: `searchPapers`, `getSearchStatus` from Task 14.
- Produces (in `state.svelte.ts`):
  - `export const searchOpts = $state<SearchOpts>({ title: true, authors: true, abstract: true, body: true, keyword: true, semantic: true })`
  - `export const searchMeta = $state<{ byId: Record<string, SearchMatch>; semantic: { available: boolean; reason: string | null }; pending: number }>(...)` — `pending` drives the "indexing N papers…" note
  - `export function toggleSearchField(k: 'title'|'authors'|'abstract'|'body'): void` — refuses to turn off the last field, reloads
  - `export function toggleSearchEngine(k: 'keyword'|'semantic'): void` — refuses to turn off the last engine, reloads
  - `export function semanticBlocked(): boolean` — true when authors-only or `!searchMeta.semantic.available`
  - `export async function loadSearchStatus(): Promise<void>`
  - `setSearch` becomes a two-stage debounce (150 ms keyword-only, 600 ms full)
  - `loadPapers` branches: empty `q` → existing `listPapers`; non-empty → `searchPapers`, filling `library.papers` + `searchMeta`

- [ ] **Step 1: Write the failing tests** — `frontend/src/lib/searchState.test.ts` (mock the api module like `projects.test.ts` does):

```typescript
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    searchPapers: vi.fn(async () => ({
      semantic: { available: true, reason: null },
      results: [
        {
          paper: { id: 'p1', title: 'T', authors: [], venue: null, year: null, doi: null,
                   arxiv_id: null, dblp_key: null, cite_key: null, url: null, source: null,
                   status: 'resolved', added_at: '' },
          match: { engine: 'keyword', field: 'body', snippet: 'a <mark>hit</mark>', page: 7 },
        },
      ],
    })),
    getSearchStatus: vi.fn(async () => ({
      fts: { indexed: 1, pending: 0, failed: 0 },
      vectors: { indexed: 0, pending: 3, failed: 0 },
      semantic_available: false,
      reason: 'no key',
    })),
  };
});

import * as api from './api';
import {
  filters, library, loadPapers, loadSearchStatus, searchMeta, searchOpts,
  semanticBlocked, setSearch, toggleSearchEngine, toggleSearchField,
} from './state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  filters.q = '';
  Object.assign(searchOpts, {
    title: true, authors: true, abstract: true, body: true, keyword: true, semantic: true,
  });
  searchMeta.byId = {};
  searchMeta.semantic = { available: true, reason: null };
  searchMeta.pending = 0;
});

describe('search state', () => {
  it('loadPapers uses searchPapers when q is set and stores match info', async () => {
    filters.q = 'fuzz';
    await loadPapers();
    expect(api.searchPapers).toHaveBeenCalledOnce();
    expect(library.papers.map((p) => p.id)).toEqual(['p1']);
    expect(searchMeta.byId['p1'].snippet).toContain('<mark>');
  });

  it('loadPapers uses listPapers and clears match info when q is empty', async () => {
    searchMeta.byId = { p1: { engine: 'keyword', field: 'body', snippet: 'x', page: null } };
    await loadPapers();
    expect(api.listPapers).toHaveBeenCalledOnce();
    expect(Object.keys(searchMeta.byId)).toHaveLength(0);
  });

  it('setSearch debounces: keyword-only first, then full', async () => {
    vi.useFakeTimers();
    setSearch('fuzz');
    expect(api.searchPapers).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(200);
    expect(api.searchPapers).toHaveBeenCalledTimes(1);
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[0][3]).toBe(true); // keywordOnly
    await vi.advanceTimersByTimeAsync(600);
    expect(api.searchPapers).toHaveBeenCalledTimes(2);
    expect((api.searchPapers as ReturnType<typeof vi.fn>).mock.calls[1][3]).toBe(false);
    vi.useRealTimers();
  });

  it('cannot turn off the last field or engine', () => {
    Object.assign(searchOpts, { title: false, authors: false, abstract: false });
    toggleSearchField('body'); // body is the last field
    expect(searchOpts.body).toBe(true);
    searchOpts.semantic = false;
    toggleSearchEngine('keyword'); // keyword is the last engine
    expect(searchOpts.keyword).toBe(true);
  });

  it('semanticBlocked for authors-only or unavailable backend', async () => {
    expect(semanticBlocked()).toBe(false);
    Object.assign(searchOpts, { title: false, abstract: false, body: false });
    expect(semanticBlocked()).toBe(true);
    Object.assign(searchOpts, { title: true, abstract: true, body: true });
    await loadSearchStatus();
    expect(searchMeta.semantic.available).toBe(false);
    expect(semanticBlocked()).toBe(true);
    expect(searchMeta.pending).toBe(3); // max(fts.pending, vectors.pending)
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && npm test`
Expected: FAIL — missing exports

- [ ] **Step 3: Implement the state.** In `frontend/src/lib/state.svelte.ts`:

Add to the imports from `./api`: `getSearchStatus, searchPapers`; from `./types`: `SearchMatch, SearchOpts`.

Add near `filters`:

```typescript
export const searchOpts = $state<SearchOpts>({
  title: true,
  authors: true,
  abstract: true,
  body: true,
  keyword: true,
  semantic: true,
});

/// Match info per paper id for the current search, plus the semantic tier's
/// availability (from the last response or /api/search/status).
export const searchMeta = $state<{
  byId: Record<string, SearchMatch>;
  semantic: { available: boolean; reason: string | null };
  /// Papers still waiting for a tier to index (drives "indexing N papers…").
  pending: number;
}>({ byId: {}, semantic: { available: true, reason: null }, pending: 0 });

/// Semantic chip is disabled when the backend can't serve it or the field
/// selection makes it meaningless (authors-only).
export function semanticBlocked(): boolean {
  const authorsOnly =
    searchOpts.authors && !searchOpts.title && !searchOpts.abstract && !searchOpts.body;
  return authorsOnly || !searchMeta.semantic.available;
}

export function toggleSearchField(k: 'title' | 'authors' | 'abstract' | 'body'): void {
  const on = ['title', 'authors', 'abstract', 'body'].filter(
    (f) => searchOpts[f as keyof SearchOpts],
  );
  if (searchOpts[k] && on.length === 1) return; // keep at least one field
  searchOpts[k] = !searchOpts[k];
  if (filters.q.trim()) void loadPapers();
}

export function toggleSearchEngine(k: 'keyword' | 'semantic'): void {
  const other = k === 'keyword' ? 'semantic' : 'keyword';
  if (searchOpts[k] && !searchOpts[other]) return; // keep at least one engine
  searchOpts[k] = !searchOpts[k];
  if (filters.q.trim()) void loadPapers();
}

export async function loadSearchStatus(): Promise<void> {
  try {
    const st = await getSearchStatus();
    searchMeta.semantic = { available: st.semantic_available, reason: st.reason };
    searchMeta.pending = Math.max(st.fts.pending, st.vectors.pending);
  } catch (e) {
    console.error(e); // e.g. 503 search not configured -> leave defaults
  }
}
```

Replace `loadPapers` (keep the `seq` superseding guard):

```typescript
let seq = 0;
export async function loadPapers(opts?: { keywordOnly?: boolean }): Promise<void> {
  const my = ++seq;
  library.loading = true;
  library.error = null;
  try {
    const q = filters.q.trim();
    if (!q) {
      const papers = await listPapers({ ...filters });
      if (my !== seq) return;
      library.papers = papers;
      searchMeta.byId = {};
    } else {
      const keywordOnly = Boolean(opts?.keywordOnly) || !searchOpts.semantic;
      const resp = await searchPapers(q, { ...searchOpts }, { ...filters }, keywordOnly);
      if (my !== seq) return;
      library.papers = resp.results.map((r) => r.paper);
      searchMeta.byId = Object.fromEntries(resp.results.map((r) => [r.paper.id, r.match]));
      searchMeta.semantic = { available: resp.semantic.available, reason: resp.semantic.reason };
    }
  } catch (e) {
    if (my === seq) library.error = (e as Error).message;
  } finally {
    if (my === seq) library.loading = false;
  }
}
```

Replace `setSearch` (two-stage debounce; the embedding API is only hit once per settled query):

```typescript
let kwDebounce: ReturnType<typeof setTimeout> | undefined;
let fullDebounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  if (!q.trim()) {
    void loadPapers();
    return;
  }
  // Fast keyword-only pass while typing; the full (semantic) pass once settled.
  if (searchOpts.keyword) {
    kwDebounce = setTimeout(() => void loadPapers({ keywordOnly: true }), 150);
  }
  if (searchOpts.semantic && !semanticBlocked()) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  } else if (!searchOpts.keyword) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npm test`
Expected: PASS

- [ ] **Step 5: Wire the UI.** In `frontend/src/components/Sidebar.svelte`, import the new state and add a chip row directly under the search `<div class="relative">` block:

```svelte
    <div class="flex flex-wrap gap-1 text-[11px]">
      {#each [['title', 'Title'], ['authors', 'Authors'], ['abstract', 'Abstract'], ['body', 'Body']] as [key, label] (key)}
        <button
          type="button"
          aria-pressed={searchOpts[key as 'title' | 'authors' | 'abstract' | 'body']}
          onclick={() => toggleSearchField(key as 'title' | 'authors' | 'abstract' | 'body')}
          class={`rounded-full border px-2 py-0.5 ${
            searchOpts[key as 'title' | 'authors' | 'abstract' | 'body']
              ? 'border-indigo-300 bg-indigo-50 text-indigo-700 dark:border-indigo-700 dark:bg-indigo-950 dark:text-indigo-300'
              : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
          }`}
        >
          {label}
        </button>
      {/each}
      <span class="mx-1 border-l border-slate-200 dark:border-slate-700"></span>
      <button
        type="button"
        aria-pressed={searchOpts.keyword}
        onclick={() => toggleSearchEngine('keyword')}
        class={`rounded-full border px-2 py-0.5 ${
          searchOpts.keyword
            ? 'border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-300'
            : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
        }`}
      >
        Keyword
      </button>
      <button
        type="button"
        aria-pressed={searchOpts.semantic && !semanticBlocked()}
        disabled={semanticBlocked()}
        title={searchMeta.semantic.reason ?? undefined}
        onclick={() => toggleSearchEngine('semantic')}
        class={`rounded-full border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-40 ${
          searchOpts.semantic && !semanticBlocked()
            ? 'border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-300'
            : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
        }`}
      >
        Semantic
      </button>
    </div>
    {#if searchMeta.pending > 0}
      <p class="text-[11px] text-slate-400 dark:text-slate-500">
        indexing {searchMeta.pending} paper{searchMeta.pending === 1 ? '' : 's'}…
      </p>
    {/if}
```

Extend the Sidebar imports accordingly (`searchMeta, searchOpts, semanticBlocked, toggleSearchEngine, toggleSearchField`), and change the search input placeholder to `"Search library…"`.

In `frontend/src/components/PaperRow.svelte`, import `searchMeta` and add after the authors line:

```svelte
  {#if searchMeta.byId[paper.id]}
    {@const m = searchMeta.byId[paper.id]}
    <div class="mt-1 text-xs text-slate-600 dark:text-slate-300">
      <span class="mr-1 rounded bg-slate-100 px-1 py-px text-[10px] uppercase tracking-wide text-slate-500 dark:bg-slate-800 dark:text-slate-400">
        {m.field}{#if m.page != null}&nbsp;p.{m.page}{/if}
      </span>
      <!-- Server contract: snippet text is HTML-escaped; only <mark> tags. -->
      <span class="[&_mark]:rounded [&_mark]:bg-amber-200 [&_mark]:px-0.5 dark:[&_mark]:bg-amber-500/40">
        {@html m.snippet}
      </span>
    </div>
  {/if}
```

In `frontend/src/App.svelte`, add `loadSearchStatus` to the state import and call it in `onMount` after `loadPapers();`.

- [ ] **Step 6: Verify the frontend builds and tests pass**

Run: `cd frontend && npm test && npm run build`
Expected: PASS + clean build. Optionally `cargo run -- serve` and try the box: chips toggle, snippets render, Semantic chip greyed with a tooltip when no embedding key is configured.

- [ ] **Step 7: Commit**

```bash
git add frontend/src
git commit -m "feat(web): hybrid search box with field/engine chips and snippets"
```

---

### Task 16: End-to-end test

**Files:**
- Create: `tests/search_test.rs`

**Interfaces:**
- Consumes: the full pipeline — `IngestCtx::ingest_file` (see `tests/pipeline_test.rs` / `src/watcher.rs` tests for the offline-resolver setup), `SearchService::open_with`, `indexer::sweep`, `SearchService::search`.

- [ ] **Step 1: Write the test** — `tests/search_test.rs`:

```rust
//! End-to-end: import a PDF -> background sweep -> find it by a body phrase.

use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::io::BufWriter;
use std::path::Path;

use xuewen::pipeline::{IngestCtx, Libraries};
use xuewen::resolve::Resolver;
use xuewen::search::{fts, indexer, vector, SearchRequest, SearchService};

/// Two text lines: a title-looking first line, then the body phrase. If the
/// ingest heuristics take the first line as the title, the search phrase
/// still only exists in the body — so the snippet's field must be "body".
fn write_pdf(path: &Path, title_line: &str, body_line: &str) {
    let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let layer = doc.get_page(page1).get_layer(layer1);
    layer.use_text(title_line, 14.0, Mm(15.0), Mm(280.0), &font);
    layer.use_text(body_line, 11.0, Mm(15.0), Mm(250.0), &font);
    doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap()))
        .unwrap();
}

// Upstreams refuse instantly -> the paper lands as needs_review, offline.
fn offline_resolver() -> Resolver {
    Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string())
}

#[tokio::test]
async fn imported_pdf_becomes_keyword_searchable_by_body_text() {
    let dir = tempfile::tempdir().unwrap();
    let library_root = dir.path().join("library");
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = xuewen::db::connect(&url).await.unwrap();

    // 1. Ingest a PDF whose body holds a distinctive phrase.
    let pdf = inbox.join("paper.pdf");
    write_pdf(
        &pdf,
        "A Study of Distributed Authorization",
        "we evaluate the zanzibar consistency protocol",
    );
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries {
            library_root: library_root.clone(),
            processed_dir: inbox.join("_processed"),
        },
        resolver: offline_resolver(),
        grobid: None,
    };
    ctx.ingest_file(&pdf).await.unwrap();

    // 2. One indexer sweep (keyword tier only; no embedder configured).
    let idx_dir = dir.path().join("search-index");
    let (fts_idx, _) = fts::FtsIndex::open(&idx_dir).unwrap();
    let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
    let svc = SearchService::open_with(pool, fts_idx, vectors, None);
    let summary = indexer::sweep(&svc, &library_root).await.unwrap();
    assert_eq!(summary.indexed, 1);
    assert_eq!(summary.failed, 0);

    // 3. A body phrase finds the paper, with an evidence snippet.
    let out = svc
        .search(&SearchRequest {
            q: "zanzibar".into(),
            fields: fts::FieldSel::all(),
            keyword: true,
            semantic: true,
            status: None,
            project: None,
        })
        .await
        .unwrap();
    assert!(!out.semantic.available, "no embedder configured");
    assert_eq!(out.results.len(), 1);
    let (paper, m) = &out.results[0];
    assert!(paper.rel_path.ends_with(".pdf"));
    assert_eq!(m.field, "body");
    assert!(m.snippet.contains("<mark>zanzibar</mark>"), "got: {}", m.snippet);

    // 4. Status agrees everything is indexed.
    let st = svc.status().await.unwrap();
    assert_eq!(st.fts.pending, 0);
    assert_eq!(st.fts.failed, 0);
}
```

Check `Resolver::with_bases` / `with_dblp_base` signatures against `src/watcher.rs` tests; if the integration-test crate can't reach them, use the same construction `tests/pipeline_test.rs` uses for an offline resolver.

- [ ] **Step 2: Run the test**

Run: `cargo test --test search_test`
Expected: PASS

- [ ] **Step 3: Full suite + frontend**

Run: `cargo test && cd frontend && npm test && npm run build`
Expected: everything green.

- [ ] **Step 4: Commit**

```bash
git add tests/search_test.rs
git commit -m "test(search): end-to-end import -> index -> keyword search"
```

---

## Verification checklist (manual, after all tasks)

1. `docker run -p 6333:6333 qdrant/qdrant` (or however Qdrant is run), set `OPENAI_API_KEY`, add `[search.embedding]` to `xuewen.toml`.
2. `xuewen index status` → vectors pending; `xuewen index rebuild` → all indexed.
3. `xuewen serve` → search box: as-you-type keyword results; on pause, semantic results merge in; chips restrict fields/engines; snippets show `body · p.N` tags.
4. Kill Qdrant → search still works keyword-only, Semantic chip greyed with a tooltip reason.
5. Trash a paper → it disappears from search results within a sweep (~30 s or immediately after another mutation).






