# Cite-Key Filenames Implementation Plan (Plan A of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** File each newly ingested PDF under a flat Google-Scholar-style cite key — `library/<surname><year><titleword>.pdf` — instead of `library/<hash>.pdf`; store the key in a new `cite_key` column; fall back to `library/_unsorted/<hash>.pdf` when a key can't be formed.

**Architecture:** A new pure `naming` module builds/validates cite keys and library paths. The pipeline's field-selection logic is factored into `resolve_fields` (shared with the future `refresh` command), then the pipeline computes the cite key, resolves collisions against the `cite_key` DB column, and files the PDF at the derived path. `content_hash` stays the dedup identity; only `rel_path` changes.

**Tech Stack:** Rust, adds `unicode-normalization` (diacritic folding). Reuses sqlx/SQLite, serde_json, chrono, uuid, tokio from the existing crate.

---

## Plan set context

Spec: `docs/superpowers/specs/2026-07-07-cite-key-naming-and-refresh-design.md`.
- **Plan A (this file):** cite-key naming at ingest — naming module, `cite_key` column, `resolve_fields` refactor, pipeline filing.
- Plan B (next): `xuewen refresh` command — re-resolve `needs_review`, re-file all; reuses the naming module + `resolve_fields`.

### Current state (on `main`)
- `pipeline::ingest_file(pool, dirs, resolver, grobid, path)` files the PDF at `library/<hash>.pdf` (step 4), then `build_paper(content_hash, rel_path, provisional_title, extracted, &ident, resolution) -> Paper` assembles the record (step 5).
- `models::Paper { id, content_hash, rel_path, title, abstract_text(#[sqlx(rename="abstract")]), authors: Option<String>(JSON), venue, year: Option<i64>, doi, arxiv_id, dblp_key, url, source, status, added_at }`.
- `resolve::ResolvedMetadata { title, abstract_text, authors: Vec<String>, venue, year, doi, arxiv_id, dblp_key, url, source }` with `authors_json()`.
- `db::{connect, exists_by_hash, insert_paper(15 cols), get_by_id}`.
- Run cargo via `nix develop -c '<command>'` (`$IN_NIX_SHELL` not set).

## File structure

```
Cargo.toml                     # + unicode-normalization
migrations/0002_add_cite_key.sql
src/
  naming.rs                    # cite-key + path helpers (pure)
  lib.rs                       # + pub mod naming;
  models.rs                    # Paper gains cite_key
  db.rs                        # insert_paper +cite_key; new cite_keys_with_base
  pipeline.rs                  # resolve_fields + ResolvedFields; cite-key filing
tests/
  pipeline_test.rs             # updated paths + a collision test
```

---

## Task 1: Add the `unicode-normalization` dependency

**Files:** Modify `Cargo.toml`.

- [ ] **Step 1: Add to `Cargo.toml` `[dependencies]`**

```toml
unicode-normalization = "0.1"
```

- [ ] **Step 2: Build**

Run: `nix develop -c cargo build`
Expected: resolves and compiles; `Finished`.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): add unicode-normalization for cite-key folding"
```

---

## Task 2: The `naming` module

**Files:** Create `src/naming.rs`; modify `src/lib.rs`.
**Test:** unit tests inside `src/naming.rs`.

- [ ] **Step 1: Create `src/naming.rs`**

```rust
use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

/// Leading title words to skip when choosing the cite-key title word.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "on", "of", "in", "for", "to", "and", "or", "with", "at",
    "by", "from", "as", "is", "are", "be", "this", "that",
];

/// NFKD-fold to lowercase ASCII alphanumerics, joined (drops spaces, punctuation,
/// and diacritics). `"Müller-Groß"` → `"mullergro"`, `"Kaiming He!"` → `"kaiminghe"`.
pub fn fold_ascii_alnum(s: &str) -> String {
    s.nfkd()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Split into lowercase ASCII-alphanumeric runs (any non-alnum char is a boundary).
/// `"On Large-Batch Training"` → `["on", "large", "batch", "training"]`.
fn alnum_words(s: &str) -> Vec<String> {
    let decomposed: String = s.nfkd().collect();
    decomposed
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Surname component: the folded last whitespace token of a full name.
/// `"Kaiming He"` → `Some("he")`, `"Laurens van der Maaten"` → `Some("maaten")`.
pub fn surname(full_name: &str) -> Option<String> {
    let last = full_name.split_whitespace().last()?;
    let folded = fold_ascii_alnum(last);
    (!folded.is_empty()).then_some(folded)
}

/// First title word after skipping leading stop words; if every word is a stop
/// word, falls back to the first word.
pub fn first_title_word(title: &str) -> Option<String> {
    let words = alnum_words(title);
    if let Some(w) = words.iter().find(|w| !STOP_WORDS.contains(&w.as_str())) {
        return Some(w.clone());
    }
    words.into_iter().next()
}

/// The base cite key `{surname}{year}{titleword}`, or `None` if the first author,
/// the year, or a usable title word is missing.
pub fn cite_key_base(authors: &[String], year: Option<i64>, title: Option<&str>) -> Option<String> {
    let surname = surname(authors.first()?)?;
    let year = year?;
    let word = first_title_word(title?)?;
    Some(format!("{surname}{year}{word}"))
}

/// A free cite key: `base` if untaken, else `base` + `a`..`z`, then numeric.
pub fn disambiguate(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    for c in b'a'..=b'z' {
        let cand = format!("{base}{}", c as char);
        if !taken.contains(&cand) {
            return cand;
        }
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}{n}");
        if !taken.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Relative library path: `<citekey>.pdf`, or `_unsorted/<hash>.pdf` when no key.
pub fn library_rel_path(cite_key: Option<&str>, content_hash: &str) -> String {
    match cite_key {
        Some(key) => format!("{key}.pdf"),
        None => format!("_unsorted/{content_hash}.pdf"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_diacritics_and_punctuation() {
        assert_eq!(fold_ascii_alnum("Müller"), "muller");
        assert_eq!(fold_ascii_alnum("Kaiming He!"), "kaiminghe");
    }

    #[test]
    fn surname_is_last_token() {
        assert_eq!(surname("Kaiming He").as_deref(), Some("he"));
        assert_eq!(surname("Laurens van der Maaten").as_deref(), Some("maaten"));
        assert_eq!(surname("   ").as_deref(), None);
    }

    #[test]
    fn title_word_skips_stop_words() {
        assert_eq!(first_title_word("A Neural Probabilistic Language Model").as_deref(), Some("neural"));
        assert_eq!(first_title_word("Attention Is All You Need").as_deref(), Some("attention"));
        assert_eq!(first_title_word("On Large-Batch Training Methods").as_deref(), Some("large"));
        assert_eq!(first_title_word("Deep Residual Learning").as_deref(), Some("deep"));
    }

    #[test]
    fn builds_and_requires_all_parts() {
        let authors = vec!["Kaiming He".to_string()];
        assert_eq!(
            cite_key_base(&authors, Some(2016), Some("Deep Residual Learning for Image Recognition")).as_deref(),
            Some("he2016deep")
        );
        assert_eq!(cite_key_base(&[], Some(2016), Some("x")), None); // no author
        assert_eq!(cite_key_base(&authors, None, Some("x")), None); // no year
        assert_eq!(cite_key_base(&authors, Some(2016), None), None); // no title
    }

    #[test]
    fn disambiguation_appends_letters() {
        let mut taken = HashSet::new();
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deep");
        taken.insert("he2016deep".to_string());
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deepa");
        taken.insert("he2016deepa".to_string());
        assert_eq!(disambiguate("he2016deep", &taken), "he2016deepb");
    }

    #[test]
    fn rel_path_keyed_vs_unsorted() {
        assert_eq!(library_rel_path(Some("he2016deep"), "abc"), "he2016deep.pdf");
        assert_eq!(library_rel_path(None, "abc123"), "_unsorted/abc123.pdf");
    }
}
```

- [ ] **Step 2:** In `src/lib.rs` add `pub mod naming;`.

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test naming::tests`
Expected: all six PASS. If `folds_diacritics_and_punctuation` fails, confirm `UnicodeNormalization::nfkd` is in scope.

- [ ] **Step 4: Commit**

```bash
git add src/naming.rs src/lib.rs
git commit -m "feat(naming): cite-key + library-path helpers"
```

---

## Task 3: Schema — `cite_key` column + DB support

**Files:** Create `migrations/0002_add_cite_key.sql`; modify `src/models.rs`, `src/db.rs`, `src/pipeline.rs`.
**Test:** unit tests inside `src/db.rs`.

- [ ] **Step 1: Create `migrations/0002_add_cite_key.sql`**

```sql
ALTER TABLE papers ADD COLUMN cite_key TEXT;
```

- [ ] **Step 2: Add the field to `Paper`**

In `src/models.rs`, add `cite_key` after the `dblp_key` field:
```rust
    pub dblp_key: Option<String>,
    pub cite_key: Option<String>,
    pub url: Option<String>,
```

- [ ] **Step 3: Update `insert_paper` and add `cite_keys_with_base` in `src/db.rs`**

Change the `insert_paper` SQL + binds to include `cite_key` (add it right after `dblp_key`):
```rust
pub async fn insert_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "INSERT INTO papers \
         (id, content_hash, rel_path, title, abstract, authors, venue, year, \
          doi, arxiv_id, dblp_key, cite_key, url, source, status, added_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
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
    .bind(&p.cite_key)
    .bind(&p.url)
    .bind(&p.source)
    .bind(&p.status)
    .bind(&p.added_at)
    .execute(pool)
    .await?;
    Ok(())
}
```

Add this function (after `get_by_id`), plus a `use std::collections::HashSet;` at the top of `src/db.rs`:
```rust
/// Cite keys already taken by other papers that share `base` as a prefix.
/// `exclude_id` skips a paper's own key (used when re-filing during refresh).
pub async fn cite_keys_with_base(
    pool: &SqlitePool,
    base: &str,
    exclude_id: Option<&str>,
) -> Result<HashSet<String>> {
    let pattern = format!("{base}%");
    let rows: Vec<(String,)> = match exclude_id {
        Some(id) => {
            sqlx::query_as(
                "SELECT cite_key FROM papers \
                 WHERE cite_key IS NOT NULL AND cite_key LIKE ? AND id <> ?",
            )
            .bind(&pattern)
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT cite_key FROM papers WHERE cite_key IS NOT NULL AND cite_key LIKE ?",
            )
            .bind(&pattern)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows.into_iter().map(|(k,)| k).collect())
}
```

- [ ] **Step 4: Keep the crate compiling — set `cite_key: None` in `build_paper`**

In `src/pipeline.rs`, `build_paper` constructs two `Paper` literals. Add `cite_key: None,` after `dblp_key: ...,` in BOTH (this is temporary — Task 4 replaces `build_paper`):
```rust
                dblp_key: md.dblp_key,
                cite_key: None,
                url: md.url,
```
and
```rust
                dblp_key: None,
                cite_key: None,
                url: None,
```

- [ ] **Step 5: Update the `db.rs` test's `sample_paper`**

In `src/db.rs` `mod tests`, add `cite_key: None,` after `dblp_key: None,` in `sample_paper`, and extend the test with a `cite_keys_with_base` assertion. Replace the `insert_then_fetch_and_dedup` test body's tail (after the existing asserts) and the helper as follows — add the field and a new test:
```rust
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
            cite_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview.as_str().to_string(),
            added_at: "2026-07-06T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn cite_keys_with_base_returns_prefix_matches() {
        let (_dir, pool) = temp_pool().await;

        let mut a = sample_paper("01890000-0000-7000-8000-00000000000a", "ha");
        a.cite_key = Some("he2016deep".into());
        insert_paper(&pool, &a).await.unwrap();

        let mut b = sample_paper("01890000-0000-7000-8000-00000000000b", "hb");
        b.cite_key = Some("he2016deepa".into());
        insert_paper(&pool, &b).await.unwrap();

        let taken = cite_keys_with_base(&pool, "he2016deep", None).await.unwrap();
        assert!(taken.contains("he2016deep"));
        assert!(taken.contains("he2016deepa"));

        // Excluding paper A's id drops its key.
        let taken_excl = cite_keys_with_base(&pool, "he2016deep", Some(&a.id)).await.unwrap();
        assert!(!taken_excl.contains("he2016deep"));
        assert!(taken_excl.contains("he2016deepa"));
    }
```
(Keep the existing `insert_then_fetch_and_dedup` test and the `temp_pool` helper as they are.)

- [ ] **Step 6: Run the db tests**

Run: `nix develop -c cargo test db::tests`
Expected: `insert_then_fetch_and_dedup` and `cite_keys_with_base_returns_prefix_matches` PASS. (Migration `0002` runs automatically after `0001` on the temp DBs.)

- [ ] **Step 7: Run the whole suite (regression)**

Run: `nix develop -c cargo test`
Expected: all pass — existing pipeline tests still file at `<hash>.pdf` (unchanged in this task; `cite_key` is `None` everywhere so far).

- [ ] **Step 8: Commit**

```bash
git add migrations/0002_add_cite_key.sql src/models.rs src/db.rs src/pipeline.rs
git commit -m "feat(db): add cite_key column, insert binding, and prefix lookup"
```

---

## Task 4: Pipeline — file at the cite-key path

**Files:** Modify `src/pipeline.rs`, `tests/pipeline_test.rs`.
**Test:** `tests/pipeline_test.rs`.

- [ ] **Step 1: Replace `build_paper` with `resolve_fields` + `ResolvedFields::into_paper` in `src/pipeline.rs`**

Add `use crate::naming;` to the imports. Delete the entire `build_paper` function and add in its place:
```rust
/// The metadata a paper should store, decided from the resolution outcome and any
/// GROBID extraction. Shared by ingest (and, later, the `refresh` command).
pub struct ResolvedFields {
    pub title: Option<String>,
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: String,
}

/// Decide the stored fields. A confident resolution yields `resolved` (with a
/// GROBID abstract backfilled if the source lacked one); otherwise `needs_review`,
/// enriched with GROBID's title/abstract/authors when present.
pub(crate) fn resolve_fields(
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
    ident: &Identifier,
    resolution: Resolution,
) -> ResolvedFields {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    match resolution {
        Resolution::Resolved(md) => {
            let abstract_text = md
                .abstract_text
                .or_else(|| extracted.and_then(|g| g.abstract_text));
            ResolvedFields {
                title: md.title.or(provisional_title),
                abstract_text,
                authors: md.authors,
                venue: md.venue,
                year: md.year,
                doi: md.doi.or(ext_doi),
                arxiv_id: md.arxiv_id.or(ext_arxiv),
                dblp_key: md.dblp_key,
                url: md.url,
                source: Some(md.source),
                status: PaperStatus::Resolved.as_str().to_string(),
            }
        }
        Resolution::Unresolved => match extracted {
            Some(g) => ResolvedFields {
                title: g.title.or(provisional_title),
                abstract_text: g.abstract_text,
                authors: g.authors,
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: Some(g.source),
                status: PaperStatus::NeedsReview.as_str().to_string(),
            },
            None => ResolvedFields {
                title: provisional_title,
                abstract_text: None,
                authors: Vec::new(),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview.as_str().to_string(),
            },
        },
    }
}

impl ResolvedFields {
    /// Assemble a full `Paper` with a fresh id/timestamp and the given location.
    pub(crate) fn into_paper(
        self,
        content_hash: String,
        rel_path: String,
        cite_key: Option<String>,
    ) -> Paper {
        let authors = if self.authors.is_empty() {
            None
        } else {
            serde_json::to_string(&self.authors).ok()
        };
        Paper {
            id: Uuid::now_v7().to_string(),
            content_hash,
            rel_path,
            title: self.title,
            abstract_text: self.abstract_text,
            authors,
            venue: self.venue,
            year: self.year,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            dblp_key: self.dblp_key,
            cite_key,
            url: self.url,
            source: self.source,
            status: self.status,
            added_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}
```

- [ ] **Step 2: Rewrite steps 4–6 of `ingest_file`**

Replace the current step 4 + step 5 blocks (lines that create `rel_path = format!("{content_hash}.pdf")`, copy, and call `build_paper`, through the `insert_paper` error handling) with:
```rust
    // 4. Decide the stored fields, then the cite-key filename.
    let fields = resolve_fields(heuristic_title, extracted, &ident, resolution);
    let cite_key = match naming::cite_key_base(&fields.authors, fields.year, fields.title.as_deref())
    {
        Some(base) => {
            let taken = db::cite_keys_with_base(pool, &base, None).await?;
            Some(naming::disambiguate(&base, &taken))
        }
        None => None,
    };
    let rel_path = naming::library_rel_path(cite_key.as_deref(), &content_hash);

    // 5. File the PDF into the managed library.
    let dest = dirs.library_root.join(&rel_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&path, &dest)?;

    // 6. Build and store the record.
    let paper = fields.into_paper(content_hash, rel_path, cite_key);
    if let Err(e) = db::insert_paper(pool, &paper).await {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
```
Leave the final "move the original out of the inbox" step and `Ok(Outcome::Ingested(paper.id))` unchanged.

- [ ] **Step 3: Build**

Run: `nix develop -c cargo build`
Expected: compiles (the old `build_paper` is gone; `resolve_fields`/`into_paper` replace it). If a borrow/move error appears, ensure `cite_key_base` borrows `fields` before `into_paper` consumes it, and `library_rel_path` borrows `content_hash` before `into_paper` consumes it (the ordering above already does this).

- [ ] **Step 4: Update existing assertions + add a collision test in `tests/pipeline_test.rs`**

The filing behavior changed, so update the two location assertions and add cite-key checks. Apply these edits:

**(a)** In `ingests_pdf_and_dedups` (needs_review — resolver is a stub-less mock), the file now lands under `_unsorted/`. Change:
```rust
    assert!(library.join(format!("{}.pdf", paper.content_hash)).exists());
```
to:
```rust
    assert!(library.join(format!("_unsorted/{}.pdf", paper.content_hash)).exists());
    assert_eq!(paper.cite_key, None);
```

**(b)** In `same_doi_different_bytes_errors_without_orphan`, both papers are needs_review → filed under `_unsorted/`. Change the orphan check from counting `library/` to counting `library/_unsorted/`:
```rust
    let count = std::fs::read_dir(&library).unwrap().count();
    assert_eq!(count, 1, "library should contain only paper A, no orphan");
```
to:
```rust
    let count = std::fs::read_dir(library.join("_unsorted")).unwrap().count();
    assert_eq!(count, 1, "library should contain only paper A, no orphan");
```

**(c)** In `ingest_with_doi_resolves_via_crossref`, add after the existing asserts:
```rust
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
```

**(d)** In `ingest_with_arxiv_resolves_via_api`, add after the existing asserts:
```rust
    assert_eq!(paper.cite_key.as_deref(), Some("vaswani2017attention"));
    assert!(library.join("vaswani2017attention.pdf").exists());
```

**(e)** In `ingest_without_identifier_resolves_via_dblp`, add after the existing asserts:
```rust
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
```

**(f)** Append a new collision test (it pre-seeds a paper holding the base key, then ingests a DOI paper that resolves to the same base):
```rust
#[tokio::test]
async fn colliding_cite_key_gets_letter_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["Header", &format!("https://doi.org/{doi}")]);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Pre-seed a different paper that already owns the base key "wang2019kgat".
    let seed = xuewen::models::Paper {
        id: "01890000-0000-7000-8000-0000000000ff".to_string(),
        content_hash: "seedhash".to_string(),
        rel_path: "wang2019kgat.pdf".to_string(),
        title: Some("Seed".to_string()),
        abstract_text: None,
        authors: None,
        venue: None,
        year: Some(2019),
        doi: None, // no DOI -> no UNIQUE clash with the ingested paper
        arxiv_id: None,
        dblp_key: None,
        cite_key: Some("wang2019kgat".to_string()),
        url: None,
        source: Some("crossref".to_string()),
        status: "resolved".to_string(),
        added_at: "2026-07-07T00:00:00Z".to_string(),
    };
    db::insert_paper(&pool, &seed).await.unwrap();

    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };
    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgata"));
    assert!(library.join("wang2019kgata.pdf").exists());
}
```
(The imports `MockServer/Mock/ResponseTemplate/method/wm_path`, `Resolver`, `db`, `Libraries`, `Outcome`, `common`, and the `CROSSREF_FIXTURE` const already exist in this file from earlier plans.)

- [ ] **Step 5: Run the whole suite + clippy**

Run: `nix develop -c cargo test`
Expected: all pass — the 5 updated pipeline tests + the new collision test + everything else.
Run: `nix develop -c cargo clippy --all-targets 2>&1 | tail -20`
Expected: no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs tests/pipeline_test.rs
git commit -m "feat(pipeline): file ingested PDFs at cite-key paths"
```

---

## Definition of done (Plan A)

- A resolved paper is filed at `library/<citekey>.pdf` with `cite_key` set (e.g. `he2016deep`, `vaswani2017attention`, `wang2019kgat`).
- A `needs_review` paper (no author/year/title) is filed at `library/_unsorted/<hash>.pdf` with `cite_key = NULL`.
- A cite-key collision with a *different* paper gets a letter suffix (`…a`).
- `content_hash` dedup and the insert-failure orphan cleanup still hold.
- `resolve_fields` is public-in-crate and reused by ingest — ready for Plan B's `refresh`.
- All tests pass; clippy clean.

## What Plan B will add (not here)

- `db::{update_paper, all_papers, papers_by_status, find_by_id_prefix}`.
- A `resolve_pdf` helper (extract→identify→GROBID→resolve) factored out of `ingest_file` so `refresh` can re-resolve a stored PDF.
- `xuewen refresh [ID] [--all]`: re-resolve `needs_review` (or all/one), re-file every paper (collision `taken` set excludes self), update rows in place.
