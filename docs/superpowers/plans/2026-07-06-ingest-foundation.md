# Ingest Foundation Implementation Plan (Slice 1, Plan 1 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the offline foundation of Xuewen's ingest pipeline: given a PDF, compute its content hash, dedup against SQLite, extract text, pull out any DOI/arXiv identifier and a provisional title, file the PDF into a managed library, and store a `needs_review` record — all driven by an `ingest <file>` CLI.

**Architecture:** A single Rust binary (`xuewen`) using tokio. Small, focused modules — `config`, `db`, `hash`, `pdf`, `identify`, `models`, `pipeline` — each independently testable. SQLite (via `sqlx`) is the store; papers get a UUIDv7 primary key and a library-relative path. No network or GROBID in this plan; metadata resolution (Plan 2) and the watcher daemon (Plan 3) build on top.

**Tech Stack:** Rust (edition 2021), tokio, sqlx (SQLite), uuid (v7), sha2, regex, clap, serde/toml, tracing, chrono. Dev: tempfile, printpdf. System dep: `pdftotext` (poppler-utils), provided by the Nix dev shell.

---

## Plan set context

This is **Plan 1 of 3** for slice 1 (see `docs/superpowers/specs/2026-07-06-pdf-ingest-metadata-pipeline-design.md`).

- Plan 1 (this file): offline foundation — a PDF becomes a stored `needs_review` record.
- Plan 2: metadata resolvers (arXiv/Crossref/DBLP/GROBID) + routing + confidence gate → records become `resolved`.
- Plan 3: `notify` watcher daemon + debounce + catch-up + retry/backoff.

The schema created here already includes every column later plans need (`venue`, `year`, `authors`, `abstract`, `dblp_key`, `url`, `source`, `status`), so no identity migration is required later.

## File structure

```
Cargo.toml                     # crate manifest + deps
flake.nix                      # Nix dev shell: rust toolchain + poppler_utils + sqlite
xuewen.example.toml            # sample config
migrations/
  0001_init.sql                # papers table + indexes
src/
  main.rs                      # clap CLI: `ingest <path>`
  config.rs                    # Config struct + TOML load
  models.rs                    # Identifier, PaperStatus, Paper
  db.rs                        # pool, migrations, exists_by_hash, insert_paper, get_by_id
  hash.rs                      # sha256_file
  pdf.rs                       # extract_text via pdftotext
  identify.rs                  # DOI/arXiv regex + provisional title guess
  pipeline.rs                  # ingest_file orchestration
tests/
  common/mod.rs                # test helper: write_test_pdf (printpdf)
  pipeline_test.rs             # end-to-end ingest test
```

**Module responsibilities (each has one job):**
- `hash`: bytes → SHA-256 hex. No knowledge of DB or PDFs.
- `pdf`: path → extracted text. Only IO wrapper over `pdftotext`.
- `identify`: text → `Identifier` + provisional title. Pure, no IO.
- `models`: shared domain types. No logic beyond trivial conversions.
- `db`: all SQL. Nothing else touches the database.
- `pipeline`: orchestrates the above; owns file movement policy.
- `config`/`main`: wiring.

---

## Task 0: Project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `flake.nix`
- Create: `src/main.rs`
- Create: `.gitignore` (already exists from spec commit — verify contents)

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "xuewen"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "macros", "migrate"] }
uuid = { version = "1", features = ["v7"] }
sha2 = "0.10"
hex = "0.4"
regex = "1"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = "0.4"
anyhow = "1"

[dev-dependencies]
tempfile = "3"
printpdf = "0.7"
```

- [ ] **Step 2: Create `flake.nix`**

```nix
{
  description = "Xuewen — self-hosted reference manager";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo rustc rustfmt clippy rust-analyzer
            poppler_utils   # provides `pdftotext`
            sqlite
            pkg-config
          ];
        };
      });
    };
}
```

- [ ] **Step 3: Create a minimal `src/main.rs` so the crate builds**

```rust
fn main() {
    println!("xuewen");
}
```

- [ ] **Step 4: Verify `.gitignore` covers build artifacts**

Confirm `.gitignore` contains `/target` and `library.db*` (created during the spec commit). If missing, add them.

- [ ] **Step 5: Build to verify the toolchain + deps resolve**

Run: `nix develop -c cargo build`
Expected: dependencies download and compile; final line `Finished \`dev\` profile`. (First build is slow.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock flake.nix src/main.rs .gitignore
git commit -m "chore: project scaffold (cargo + nix dev shell)"
```

---

## Task 1: Domain models

**Files:**
- Create: `src/models.rs`
- Modify: `src/main.rs` (declare `mod models;`)
- Test: unit tests inside `src/models.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/models.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Arxiv(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperStatus {
    Resolved,
    NeedsReview,
}

impl PaperStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaperStatus::Resolved => "resolved",
            PaperStatus::NeedsReview => "needs_review",
        }
    }
}

/// A stored bibliographic record. Column names match `migrations/0001_init.sql`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Paper {
    pub id: String,
    pub content_hash: String,
    pub rel_path: String,
    pub title: Option<String>,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Option<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: String,
    pub added_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_match_schema() {
        assert_eq!(PaperStatus::Resolved.as_str(), "resolved");
        assert_eq!(PaperStatus::NeedsReview.as_str(), "needs_review");
    }

    #[test]
    fn identifier_equality() {
        assert_eq!(Identifier::Doi("10.1/x".into()), Identifier::Doi("10.1/x".into()));
        assert_ne!(Identifier::Doi("10.1/x".into()), Identifier::None);
    }
}
```

- [ ] **Step 2: Declare the module**

In `src/main.rs`, add at the top (above `fn main`):

```rust
mod models;
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `nix develop -c cargo test models::tests`
Expected: `status_strings_match_schema` and `identifier_equality` PASS. (These are straightforward, so they pass immediately once the code compiles — the value here is establishing the shared types other tasks depend on.)

- [ ] **Step 4: Commit**

```bash
git add src/models.rs src/main.rs
git commit -m "feat: domain models (Identifier, PaperStatus, Paper)"
```

---

## Task 2: Configuration loading

**Files:**
- Create: `src/config.rs`
- Create: `xuewen.example.toml`
- Modify: `src/main.rs` (declare `mod config;`)
- Test: unit tests inside `src/config.rs`

- [ ] **Step 1: Write the failing test**

Create `src/config.rs`:

```rust
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub inbox_dir: PathBuf,
    pub library_root: PathBuf,
    pub database_url: String,
    #[serde(default)]
    pub grobid_url: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_minimal_config() {
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
        assert_eq!(cfg.inbox_dir, PathBuf::from("/data/inbox"));
        assert_eq!(cfg.library_root, PathBuf::from("/data/library"));
        assert_eq!(cfg.database_url, "sqlite:/data/library.db");
        assert_eq!(cfg.grobid_url, None);
    }
}
```

- [ ] **Step 2: Declare the module**

In `src/main.rs`, add:

```rust
mod config;
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `nix develop -c cargo test config::tests::loads_minimal_config`
Expected: FAIL to compile first if `tempfile` unused elsewhere — it is a dev-dependency, so it compiles. Test should PASS once code compiles. If you wrote the test before the struct, it fails with "cannot find type `Config`". Add the struct (already in Step 1) so it passes.

- [ ] **Step 4: Create `xuewen.example.toml`**

```toml
# Copy to xuewen.toml and edit.
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./library.db"

# Used in later plans (Plan 2/3). Optional now.
# grobid_url    = "http://localhost:8070"
# contact_email = "you@example.com"
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `nix develop -c cargo test config::tests::loads_minimal_config`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/main.rs xuewen.example.toml
git commit -m "feat: TOML configuration loading"
```

---

## Task 3: Database (schema, pool, dedup, insert)

**Files:**
- Create: `migrations/0001_init.sql`
- Create: `src/db.rs`
- Modify: `src/main.rs` (declare `mod db;`)
- Test: unit tests inside `src/db.rs`

- [ ] **Step 1: Create the migration**

Create `migrations/0001_init.sql`:

```sql
CREATE TABLE papers (
  id            TEXT PRIMARY KEY,
  content_hash  TEXT UNIQUE,
  rel_path      TEXT,
  title         TEXT,
  abstract      TEXT,
  authors       TEXT,
  venue         TEXT,
  year          INTEGER,
  doi           TEXT UNIQUE,
  arxiv_id      TEXT UNIQUE,
  dblp_key      TEXT,
  url           TEXT,
  source        TEXT,
  status        TEXT NOT NULL,
  added_at      TEXT NOT NULL
);

CREATE INDEX idx_papers_status ON papers(status);
CREATE INDEX idx_papers_year   ON papers(year);
```

- [ ] **Step 2: Write the failing test + implementation**

Create `src/db.rs`:

```rust
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::models::Paper;

/// Open (creating if needed) the SQLite database and run migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn exists_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<bool> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM papers WHERE content_hash = ?")
            .bind(content_hash)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

pub async fn insert_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "INSERT INTO papers \
         (id, content_hash, rel_path, title, abstract, authors, venue, year, \
          doi, arxiv_id, dblp_key, url, source, status, added_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&p.id)
    .bind(&p.content_hash)
    .bind(&p.rel_path)
    .bind(&p.title)
    .bind(&p.abstract_text)
    .bind(&p.authors)
    .bind(&p.venue)
    .bind(p.year)
    .bind(&p.doi)
    .bind(&p.arxiv_id)
    .bind(&p.dblp_key)
    .bind(&p.url)
    .bind(&p.source)
    .bind(&p.status)
    .bind(&p.added_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PaperStatus;

    fn sample_paper(id: &str, hash: &str) -> Paper {
        Paper {
            id: id.to_string(),
            content_hash: hash.to_string(),
            rel_path: format!("{hash}.pdf"),
            title: Some("A Title".into()),
            abstract_text: None,
            authors: None,
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview.as_str().to_string(),
            added_at: "2026-07-06T00:00:00Z".to_string(),
        }
    }

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let url = format!("sqlite:{}", db_path.display());
        let pool = connect(&url).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn insert_then_fetch_and_dedup() {
        let (_dir, pool) = temp_pool().await;

        assert!(!exists_by_hash(&pool, "abc").await.unwrap());

        let p = sample_paper("01890000-0000-7000-8000-000000000000", "abc");
        insert_paper(&pool, &p).await.unwrap();

        assert!(exists_by_hash(&pool, "abc").await.unwrap());

        let got = get_by_id(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.content_hash, "abc");
        assert_eq!(got.title.as_deref(), Some("A Title"));
        assert_eq!(got.status, "needs_review");
    }
}
```

- [ ] **Step 3: Declare the module**

In `src/main.rs`, add:

```rust
mod db;
```

- [ ] **Step 4: Run the test to verify it fails, then passes**

Run: `nix develop -c cargo test db::tests::insert_then_fetch_and_dedup`
Expected: If the migration file is missing or misnamed, `sqlx::migrate!` fails to compile with a clear path error — fix by ensuring `migrations/0001_init.sql` exists. Once compiling, the test PASSES (insert, dedup, and fetch all succeed).

- [ ] **Step 5: Commit**

```bash
git add migrations/0001_init.sql src/db.rs src/main.rs
git commit -m "feat: SQLite schema, pool, dedup, and insert"
```

---

## Task 4: Content hashing

**Files:**
- Create: `src/hash.rs`
- Modify: `src/main.rs` (declare `mod hash;`)
- Test: unit tests inside `src/hash.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `src/hash.rs`:

```rust
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;

/// SHA-256 of a file's bytes, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hashes_known_bytes() {
        // Known: SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        let h = sha256_file(f.path()).unwrap();
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
```

- [ ] **Step 2: Declare the module**

In `src/main.rs`, add:

```rust
mod hash;
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `nix develop -c cargo test hash::tests::hashes_known_bytes`
Expected: PASS (matches the well-known SHA-256 of `"hello"`).

- [ ] **Step 4: Commit**

```bash
git add src/hash.rs src/main.rs
git commit -m "feat: SHA-256 content hashing"
```

---

## Task 5: Identifier extraction + provisional title

**Files:**
- Create: `src/identify.rs`
- Modify: `src/main.rs` (declare `mod identify;`)
- Test: unit tests inside `src/identify.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `src/identify.rs`:

```rust
use crate::models::Identifier;
use regex::Regex;
use std::sync::LazyLock;

static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());
static ARXIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap());

pub fn extract_doi(text: &str) -> Option<String> {
    DOI_RE
        .find(text)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ')', ';']).to_string())
}

pub fn extract_arxiv(text: &str) -> Option<String> {
    ARXIV_RE.captures(text).map(|c| c[1].to_string())
}

/// Prefer a DOI (published record) over an arXiv id (preprint) when both appear.
pub fn identify(text: &str) -> Identifier {
    if let Some(doi) = extract_doi(text) {
        return Identifier::Doi(doi);
    }
    if let Some(id) = extract_arxiv(text) {
        return Identifier::Arxiv(id);
    }
    Identifier::None
}

/// Best-effort provisional title: the first substantive line of the header text.
/// Overwritten by authoritative metadata in Plan 2.
pub fn guess_title(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.len() >= 8
            && !t.to_lowercase().starts_with("arxiv")
            && !t.contains('@')
            && !DOI_RE.is_match(t)
            && t.chars().any(|c| c.is_alphabetic())
        {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_doi() {
        let text = "See https://doi.org/10.1145/3292500.3330701 for details.";
        assert_eq!(extract_doi(text).as_deref(), Some("10.1145/3292500.3330701"));
    }

    #[test]
    fn finds_arxiv() {
        assert_eq!(extract_arxiv("arXiv:1706.03762v5").as_deref(), Some("1706.03762v5"));
        assert_eq!(extract_arxiv("arXiv: 2001.00001").as_deref(), Some("2001.00001"));
    }

    #[test]
    fn doi_wins_over_arxiv() {
        let text = "arXiv:1706.03762  doi:10.1145/3292500.3330701";
        assert_eq!(identify(text), Identifier::Doi("10.1145/3292500.3330701".into()));
    }

    #[test]
    fn no_identifier() {
        assert_eq!(identify("Just some prose with no ids."), Identifier::None);
    }

    #[test]
    fn guesses_title_skipping_arxiv_banner() {
        let text = "arXiv:1706.03762v5 [cs.CL] 6 Dec 2017\nAttention Is All You Need\nAshish Vaswani";
        assert_eq!(guess_title(text).as_deref(), Some("Attention Is All You Need"));
    }
}
```

- [ ] **Step 2: Declare the module**

In `src/main.rs`, add:

```rust
mod identify;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `nix develop -c cargo test identify::tests`
Expected: all five tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/identify.rs src/main.rs
git commit -m "feat: DOI/arXiv extraction and provisional title guess"
```

---

## Task 6: PDF text extraction

**Files:**
- Create: `src/pdf.rs`
- Create: `tests/common/mod.rs` (shared test helper)
- Modify: `src/main.rs` (declare `mod pdf;`)
- Test: `src/pdf.rs` unit test (uses the helper via a small inline copy) — see note below

**Note on the test helper:** integration tests in `tests/` and unit tests in `src/` cannot share a module directly. We put the reusable `write_test_pdf` helper in `tests/common/mod.rs` for the Task 7 integration test, and use a small local helper inside `src/pdf.rs`'s unit test. Both use `printpdf`.

- [ ] **Step 1: Create the shared PDF test helper**

Create `tests/common/mod.rs`:

```rust
use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Write a one-page PDF whose lines are `lines`, top-to-bottom.
/// pdftotext reliably extracts built-in Helvetica text.
pub fn write_test_pdf(path: &Path, lines: &[&str]) {
    let (doc, page1, layer1) =
        PdfDocument::new("test", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let layer = doc.get_page(page1).get_layer(layer1);
    let mut y = 280.0;
    for line in lines {
        layer.use_text(*line, 12.0, Mm(15.0), Mm(y), &font);
        y -= 8.0;
    }
    doc.save(&mut BufWriter::new(File::create(path).unwrap()))
        .unwrap();
}
```

- [ ] **Step 2: Write the implementation + unit test**

Create `src/pdf.rs`:

```rust
use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

/// Extract text from pages 1..=`last_page` using the `pdftotext` binary.
pub fn extract_text(path: &Path, last_page: u32) -> Result<String> {
    let out = Command::new("pdftotext")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg(last_page.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use std::fs::File;
    use std::io::BufWriter;

    fn write_pdf(path: &Path, line: &str) {
        let (doc, page1, layer1) =
            PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(File::create(path).unwrap()))
            .unwrap();
    }

    #[test]
    fn extracts_known_text() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("sample.pdf");
        write_pdf(&pdf, "Attention Is All You Need");
        let text = extract_text(&pdf, 1).unwrap();
        assert!(
            text.contains("Attention Is All You Need"),
            "extracted text was: {text:?}"
        );
    }
}
```

- [ ] **Step 3: Declare the module**

In `src/main.rs`, add:

```rust
mod pdf;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `nix develop -c cargo test pdf::tests::extracts_known_text`
Expected: PASS. Requires `pdftotext` on PATH — the `nix develop` shell provides it via `poppler_utils`. If run outside the dev shell and it errors with "failed to run pdftotext", re-run inside `nix develop`.

- [ ] **Step 5: Commit**

```bash
git add src/pdf.rs tests/common/mod.rs src/main.rs
git commit -m "feat: PDF text extraction via pdftotext"
```

---

## Task 7: Ingest pipeline (end-to-end, offline)

**Files:**
- Create: `src/pipeline.rs`
- Create: `tests/pipeline_test.rs`
- Modify: `src/main.rs` (declare `mod pipeline;` and make modules visible to the integration test — see Step 4)
- Test: `tests/pipeline_test.rs`

- [ ] **Step 1: Write the implementation**

Create `src/pipeline.rs`:

```rust
use anyhow::{anyhow, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{Identifier, Paper, PaperStatus};
use crate::{db, hash, identify, pdf};

/// Directories the pipeline manages.
pub struct Libraries {
    pub library_root: PathBuf,
    pub processed_dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String), // new paper id
    Duplicate,
}

/// Ingest a single PDF: hash, dedup, extract, identify, file, and store.
pub async fn ingest_file(pool: &SqlitePool, dirs: &Libraries, path: &Path) -> Result<Outcome> {
    let path = path.to_path_buf();

    // 1. Hash (blocking IO off the async runtime).
    let content_hash = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await??
    };

    // 2. Dedup.
    if db::exists_by_hash(pool, &content_hash).await? {
        move_to(&path, &dirs.processed_dir)?;
        return Ok(Outcome::Duplicate);
    }

    // 3. Extract first-page text and identify.
    let text = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
    };
    let ident = identify::identify(&text);
    let title = identify::guess_title(&text);

    // 4. File the PDF into the managed library as <hash>.pdf.
    std::fs::create_dir_all(&dirs.library_root)?;
    let rel_path = format!("{content_hash}.pdf");
    std::fs::copy(&path, dirs.library_root.join(&rel_path))?;

    // 5. Build and store the record (needs_review until Plan 2 resolves metadata).
    let (doi, arxiv_id) = match &ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    let paper = Paper {
        id: Uuid::now_v7().to_string(),
        content_hash,
        rel_path,
        title,
        abstract_text: None,
        authors: None,
        venue: None,
        year: None,
        doi,
        arxiv_id,
        dblp_key: None,
        url: None,
        source: None,
        status: PaperStatus::NeedsReview.as_str().to_string(),
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    db::insert_paper(pool, &paper).await?;

    // 6. Move the original out of the inbox.
    move_to(&path, &dirs.processed_dir)?;
    Ok(Outcome::Ingested(paper.id))
}

/// Move `src` into `dir`, falling back to copy+remove across filesystems.
fn move_to(src: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = src.file_name().ok_or_else(|| anyhow!("path has no file name"))?;
    let dest = dir.join(name);
    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}
```

- [ ] **Step 2: Write the failing integration test**

Create `tests/pipeline_test.rs`:

```rust
mod common;

use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};

#[tokio::test]
async fn ingests_pdf_and_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // A PDF whose header carries a title and a DOI.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &[
            "Attention Is All You Need",
            "https://doi.org/10.1145/3292500.3330701",
        ],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    // First ingest: stored, filed, moved.
    let out = ingest_file(&pool, &dirs, &pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(paper.doi.as_deref(), Some("10.1145/3292500.3330701"));
    assert_eq!(paper.status, "needs_review");

    // File was copied into the library and the original moved to _processed.
    assert!(library.join(format!("{}.pdf", paper.content_hash)).exists());
    assert!(!pdf_path.exists());
    assert!(processed.join("paper.pdf").exists());

    // Re-ingest identical content (from processed copy) → Duplicate.
    let again = processed.join("paper.pdf");
    let out2 = ingest_file(&pool, &dirs, &again).await.unwrap();
    assert_eq!(out2, Outcome::Duplicate);
}
```

- [ ] **Step 3: Convert the crate to a lib + bin so integration tests can import it**

Integration tests in `tests/` can only import a **library** crate. Create `src/lib.rs` exposing the modules, and have `main.rs` use the library.

Create `src/lib.rs`:

```rust
pub mod config;
pub mod db;
pub mod hash;
pub mod identify;
pub mod models;
pub mod pdf;
pub mod pipeline;
```

- [ ] **Step 4: Update `src/main.rs` to use the library crate**

Replace the `mod ...;` declarations in `src/main.rs` with `use xuewen::...;`. The full `main.rs` is written in Task 8; for now, make `main.rs` reference the lib so the crate still builds:

```rust
fn main() {
    // CLI wired up in Task 8.
    println!("xuewen {}", env!("CARGO_PKG_VERSION"));
}
```

Remove any now-duplicate `mod config;`, `mod db;`, etc. from `main.rs` (they live in `lib.rs` now).

- [ ] **Step 5: Run the test to verify it fails, then passes**

Run: `nix develop -c cargo test --test pipeline_test`
Expected first run (before `lib.rs` exists): FAIL to compile — `unresolved import xuewen`. After Steps 3–4: PASS — asserts the stored fields, the filed library copy, the moved original, and the dedup outcome.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs src/lib.rs src/main.rs tests/pipeline_test.rs
git commit -m "feat: offline ingest pipeline (hash, dedup, extract, file, store)"
```

---

## Task 8: CLI wiring (`ingest <path>`)

**Files:**
- Modify: `src/main.rs`
- Test: manual smoke test (documented) + `cargo test` regression

- [ ] **Step 1: Write the full CLI**

Replace `src/main.rs` with:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};

#[derive(Parser)]
#[command(name = "xuewen", version)]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, default_value = "xuewen.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ingest a single PDF file.
    Ingest { path: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    let pool = db::connect(&cfg.database_url).await?;
    let dirs = Libraries {
        library_root: cfg.library_root.clone(),
        processed_dir: cfg.inbox_dir.join("_processed"),
    };

    match cli.command {
        Command::Ingest { path } => match ingest_file(&pool, &dirs, &path).await? {
            Outcome::Ingested(id) => println!("ingested {id}"),
            Outcome::Duplicate => println!("duplicate, skipped"),
        },
    }
    Ok(())
}
```

- [ ] **Step 2: Build**

Run: `nix develop -c cargo build`
Expected: compiles clean.

- [ ] **Step 3: Manual smoke test**

```bash
cd "$(mktemp -d)"
cat > xuewen.toml <<'EOF'
inbox_dir    = "./inbox"
library_root = "./library"
database_url = "sqlite:./library.db"
EOF
mkdir -p inbox
# Put any real PDF at inbox/test.pdf (e.g. copy one you have), then:
nix develop /home/jie/Repos/Xuewen -c cargo run --manifest-path /home/jie/Repos/Xuewen/Cargo.toml -- --config ./xuewen.toml ingest ./inbox/test.pdf
```
Expected: prints `ingested <uuid>`; `library/<hash>.pdf` exists; `inbox/_processed/test.pdf` exists. Running the same file again from `_processed` prints `duplicate, skipped`.

- [ ] **Step 4: Run the whole test suite (regression)**

Run: `nix develop -c cargo test`
Expected: all unit + integration tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: ingest CLI command"
```

---

## Definition of done (Plan 1)

- `cargo test` passes (models, config, db, hash, identify, pdf, pipeline).
- `xuewen ingest <file>` files a PDF into the library, stores a `needs_review`
  record with `content_hash`, `rel_path`, provisional `title`, and any
  `doi`/`arxiv_id`, moves the original to `inbox/_processed/`, and dedups on
  re-ingest.
- Schema carries all columns Plan 2/3 need; no identity migration pending.

## What Plan 2 will add (not in scope here)

- `src/resolve/{mod,arxiv,crossref,dblp,grobid}.rs`: source clients with
  fixture-based tests.
- Routing by `Identifier` + GROBID fallback for the title-only path.
- `src/matching.rs`: title normalization + fuzzy confidence gate (concrete
  threshold decided there; `strsim` added then).
- Pipeline change: after identify, call the resolver; on a confident match set
  `status = resolved` and fill `title/abstract/authors/venue/year/dblp_key/url/source`.
```
