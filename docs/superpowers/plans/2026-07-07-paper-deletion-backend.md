# Paper Deletion Plan A — Backend + CLI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Logical soft-delete for papers: a `deleted_at` flag hides a paper from every active view (sidebar list, stats, refresh) without moving its PDF; `xuewen delete <ID>` trashes, `xuewen purge` permanently removes (row + file).

**Architecture:** Add a nullable `deleted_at` column (migration `0003`) + `Paper.deleted_at`. New `db` mutations (`soft_delete`, `trashed_papers`, `delete_row`) and a relocated `db::find_one` (exact-or-prefix id lookup, moved out of `refresh`). Active-view queries (`list_papers`, `stats`, `all_papers`) filter `deleted_at IS NULL`. Two CLI subcommands (`delete`, `purge`) orchestrate lookup + confirmation + the file removal for purge.

**Tech Stack:** Rust, sqlx (SQLite), clap, anyhow, chrono, tokio.

**Environment:** `$IN_NIX_SHELL` is not set — run every cargo command through the flake dev shell with SEPARATE args: `nix develop -c cargo test` (NOT a single quoted string). Commit with `git -c commit.gpgsign=false commit -m "..."` (SSH signing unavailable). Conventional Commits, scope required, types feat/fix/docs/chore/ci. Run `cargo fmt` before each commit. Spec: `docs/superpowers/specs/2026-07-07-paper-deletion-design.md`.

---

## File Structure

- **Create** `migrations/0003_add_deleted_at.sql`.
- **Modify** `src/models.rs` — add `Paper.deleted_at`.
- **Modify** `src/pipeline.rs` — `into_paper` sets `deleted_at: None`.
- **Modify** `src/db.rs` — `insert_paper`/`update_paper` include `deleted_at`; add `soft_delete`, `trashed_papers`, `delete_row`, `find_one`; filter `deleted_at IS NULL` in `list_papers`/`stats`/`all_papers`; sample_paper test helper adds the field.
- **Modify** `src/refresh.rs` — drop the private `find_one`, call `db::find_one`.
- **Modify** `tests/web_test.rs`, `tests/refresh_test.rs` — their `paper()`/`seed_paper()` helpers add `deleted_at: None`.
- **Modify** `src/main.rs` — `Delete` + `Purge` subcommands + a `confirm` helper.

---

## Task 1: `deleted_at` column plumbing

Adding a field to `Paper` breaks every `Paper { … }` literal until each adds `deleted_at`. This task adds the column + field + updates ALL constructors atomically so the tree compiles and existing tests pass (all defaulting to `NULL`/`None`).

**Files:** `migrations/0003_add_deleted_at.sql`, `src/models.rs`, `src/pipeline.rs`, `src/db.rs`, `tests/web_test.rs`, `tests/refresh_test.rs`.

- [ ] **Step 1: Create the migration**

`migrations/0003_add_deleted_at.sql`:
```sql
-- NULL = active; an RFC-3339 timestamp = trashed (soft-deleted).
ALTER TABLE papers ADD COLUMN deleted_at TEXT;
```

- [ ] **Step 2: Add the field to `Paper` (`src/models.rs`)**

In the `Paper` struct, add the field after `added_at`:
```rust
    pub added_at: String,
    pub deleted_at: Option<String>,
}
```

- [ ] **Step 3: Update the four `Paper` constructors**

`src/pipeline.rs` — in `ResolvedFields::into_paper`, the returned `Paper { … }` literal: add `deleted_at: None,` after `added_at: chrono::Utc::now().to_rfc3339(),`.

`src/db.rs` — in the `#[cfg(test)] mod tests` `sample_paper`, add `deleted_at: None,` after the `added_at:` line.

`tests/web_test.rs` — in `fn paper(...)`, add `deleted_at: None,` after `added_at: "2026-07-07T00:00:00Z".into(),`.

`tests/refresh_test.rs` — in `fn seed_paper(...)`, add `deleted_at: None,` after `added_at: "2026-07-07T00:00:00Z".into(),`.

- [ ] **Step 4: Include `deleted_at` in `insert_paper` and `update_paper` (`src/db.rs`)**

`insert_paper` — add the column and a placeholder and a bind. The column list becomes `… status, added_at, deleted_at)` and `VALUES (?,?,…,?)` gains one more `?` (17 total). After `.bind(&p.added_at)` add:
```rust
    .bind(&p.deleted_at)
```
So the SQL string's columns end `… status, added_at, deleted_at)` with `VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)` (17 placeholders).

`update_paper` — add `deleted_at = ?` to the SET list (after `status = ?`) and bind `&p.deleted_at` after the `.bind(&p.status)` line (before `.bind(&p.id)`):
```rust
         status = ?, deleted_at = ? \
         WHERE id = ?",
```
and the bind:
```rust
    .bind(&p.status)
    .bind(&p.deleted_at)
    .bind(&p.id)
```

- [ ] **Step 5: Write a round-trip test (`src/db.rs`)**

In the `#[cfg(test)] mod tests` block, append:
```rust
    #[tokio::test]
    async fn deleted_at_round_trips() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000d0", "hd");
        insert_paper(&pool, &p).await.unwrap();
        // Fresh insert is active.
        assert_eq!(get_by_id(&pool, &p.id).await.unwrap().unwrap().deleted_at, None);
        // update_paper persists a set deleted_at.
        p.deleted_at = Some("2026-07-07T12:00:00Z".into());
        update_paper(&pool, &p).await.unwrap();
        assert_eq!(
            get_by_id(&pool, &p.id).await.unwrap().unwrap().deleted_at.as_deref(),
            Some("2026-07-07T12:00:00Z")
        );
    }
```

- [ ] **Step 6: Build + full test suite**

Run: `nix develop -c cargo build` then `nix develop -c cargo test`
Expected: compiles (all `Paper` literals updated); whole suite passes (the new column defaults NULL everywhere, no behaviour change) plus `deleted_at_round_trips`.

- [ ] **Step 7: Format + commit**

```bash
nix develop -c cargo fmt
git add migrations/0003_add_deleted_at.sql src/models.rs src/pipeline.rs src/db.rs tests/web_test.rs tests/refresh_test.rs
git -c commit.gpgsign=false commit -m "feat(delete): add deleted_at column and Paper field"
```

---

## Task 2: Trash queries + active-view filtering + `find_one` relocation

**Files:** `src/db.rs`, `src/refresh.rs`.

- [ ] **Step 1: Write failing db tests**

In `src/db.rs`'s test module, append:
```rust
    #[tokio::test]
    async fn soft_delete_hides_and_purge_removes() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.status = PaperStatus::Resolved.as_str().to_string();
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        // Soft-delete a: hidden from list/stats/all_papers; b remains.
        assert!(soft_delete(&pool, &a.id).await.unwrap());
        assert!(!soft_delete(&pool, &a.id).await.unwrap()); // idempotent: already trashed
        let listed = list_papers(&pool, None, None, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, b.id);
        assert_eq!(stats(&pool).await.unwrap().0, 1); // total counts only active
        assert_eq!(all_papers(&pool).await.unwrap().len(), 1);

        // trashed_papers sees a.
        let trashed = trashed_papers(&pool).await.unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].id, a.id);

        // find_one still resolves a trashed paper (by prefix), and get_by_id sees it.
        let found = find_one(&pool, "01890000-0000-7000-8000-0000000000a").await.unwrap();
        assert_eq!(found.id, a.id);

        // purge (delete_row) removes it entirely.
        delete_row(&pool, &a.id).await.unwrap();
        assert!(get_by_id(&pool, &a.id).await.unwrap().is_none());
        assert!(trashed_papers(&pool).await.unwrap().is_empty());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `nix develop -c cargo test --lib db::tests::soft_delete_hides_and_purge_removes`
Expected: FAIL to compile — `cannot find function soft_delete`/`trashed_papers`/`delete_row`/`find_one`.

- [ ] **Step 3: Add `deleted_at IS NULL` filtering to active-view queries (`src/db.rs`)**

`list_papers` — the `QueryBuilder` currently starts `"SELECT * FROM papers"` and conditionally adds `WHERE`. Change the base to always exclude trashed, and make the optional filters always `AND`:
```rust
    let mut qb: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL");
    if let Some(term) = q.map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{term}%");
        qb.push(" AND (title LIKE ")
            .push_bind(like.clone())
            .push(" OR authors LIKE ")
            .push_bind(like)
            .push(")");
    }
    if let Some(st) = status.filter(|s| matches!(*s, "resolved" | "needs_review")) {
        qb.push(" AND status = ").push_bind(st.to_string());
    }
```
(Remove the now-unused `has_where` toggle entirely.)

`stats` — add the filter:
```rust
         COALESCE(SUM(status = 'needs_review'), 0) \
         FROM papers WHERE deleted_at IS NULL",
```

`all_papers` — add the filter:
```rust
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NULL ORDER BY added_at",
    )
```

- [ ] **Step 4: Add the trash mutations + `find_one` (`src/db.rs`)**

Change the import at the top from `use anyhow::Result;` to `use anyhow::{bail, Result};`. After `find_by_id_prefix` (before the `#[cfg(test)]` block), add:
```rust
/// Mark a paper as trashed (soft-delete). Returns true if a row was newly
/// trashed (false if it didn't exist or was already trashed).
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let ts = chrono::Utc::now().to_rfc3339();
    let res = sqlx::query("UPDATE papers SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(ts)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Every trashed paper, oldest-trashed first.
pub async fn trashed_papers(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NOT NULL ORDER BY deleted_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(papers)
}

/// Permanently remove a paper row (the caller removes the PDF file).
pub async fn delete_row(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM papers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Find a paper by exact id, else by unique id prefix (active or trashed).
pub async fn find_one(pool: &SqlitePool, id: &str) -> Result<Paper> {
    if let Some(p) = get_by_id(pool, id).await? {
        return Ok(p);
    }
    let mut matches = find_by_id_prefix(pool, id).await?;
    match matches.len() {
        0 => bail!("no paper with id or prefix {id:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous id prefix {id:?} matches {n} papers"),
    }
}
```

- [ ] **Step 5: Relocate `find_one` out of `refresh` (`src/refresh.rs`)**

`src/refresh.rs` has a private `async fn find_one`. Delete that function entirely. Then change its one call site in `run` from `find_one(pool, &id)` to `db::find_one(pool, &id)`:
```rust
        RefreshTarget::One(id) => (vec![db::find_one(pool, &id).await?], true),
```
`db` is already imported (`use crate::db;`). Since `find_one` was the only user of `bail!` in `refresh.rs`, change its import `use anyhow::{bail, Result};` to `use anyhow::Result;`.

- [ ] **Step 6: Run the db tests + full suite + clippy**

Run: `nix develop -c cargo test --lib db::tests` then `nix develop -c cargo test` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: the new db test passes; the whole suite (incl. refresh tests, which still exercise the relocated `find_one` via `refresh <id>`) passes; clippy clean.

- [ ] **Step 7: Format + commit**

```bash
nix develop -c cargo fmt
git add src/db.rs src/refresh.rs
git -c commit.gpgsign=false commit -m "feat(delete): soft-delete/purge queries + active-view filtering"
```

---

## Task 3: `delete` and `purge` CLI subcommands

**Files:** `src/main.rs`.

- [ ] **Step 1: Add a stdin confirmation helper**

In `src/main.rs`, add near the top (after the imports) a small helper:
```rust
/// Ask a yes/no question on the terminal; returns true only on an explicit yes.
fn confirm(prompt: &str) -> anyhow::Result<bool> {
    use std::io::Write;
    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}
```

- [ ] **Step 2: Add the `Delete` and `Purge` subcommands**

Add these variants to the `Command` enum (after `Serve { … }`):
```rust
    /// Soft-delete a paper: hide it from the library (recoverable).
    Delete {
        /// Paper id (exact or unique prefix).
        id: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Permanently remove trashed papers and their PDF files.
    Purge {
        /// A trashed paper id (exact or unique prefix) to purge.
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Purge every trashed paper.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
```

- [ ] **Step 3: Add the match arms**

In `main`, after the `Command::Serve` arm, add:
```rust
        Command::Delete { id, yes } => {
            let paper = db::find_one(&pool, &id).await?;
            if paper.deleted_at.is_some() {
                println!("already deleted: {}", paper.id);
            } else {
                let title = paper.title.as_deref().unwrap_or("(untitled)");
                if yes || confirm(&format!("Delete {title:?}?"))? {
                    db::soft_delete(&pool, &paper.id).await?;
                    println!("deleted {}", paper.id);
                } else {
                    println!("cancelled");
                }
            }
        }
        Command::Purge { id, all, yes } => {
            let targets = match (id, all) {
                (Some(id), _) => {
                    let p = db::find_one(&pool, &id).await?;
                    if p.deleted_at.is_none() {
                        anyhow::bail!("{} is not in the trash (delete it first)", p.id);
                    }
                    vec![p]
                }
                (None, true) => db::trashed_papers(&pool).await?,
                (None, false) => anyhow::bail!("specify an <ID> or --all"),
            };
            if targets.is_empty() {
                println!("trash is empty");
            } else if yes
                || confirm(&format!(
                    "Permanently delete {} paper(s) and their files?",
                    targets.len()
                ))?
            {
                for p in &targets {
                    let path = cfg.library_root.join(&p.rel_path);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
                    }
                    db::delete_row(&pool, &p.id).await?;
                }
                println!("purged {} paper(s)", targets.len());
            } else {
                println!("cancelled");
            }
        }
```

- [ ] **Step 4: Build + verify CLI wiring**

Run: `nix develop -c cargo build`
Expected: compiles.

Run: `nix develop -c cargo run -- delete --help` and `nix develop -c cargo run -- purge --help`
Expected: `delete` shows `<ID>` + `--yes`; `purge` shows `[ID]` + `--all` + `--yes`. Also `nix develop -c cargo run -- purge x --all` → clap conflict error.

- [ ] **Step 5: Manual smoke (delete → hidden → purge)**

Confirm the CLI wiring and error paths against a fresh (empty) library. The `db::connect` in `main` runs the migrations (incl. `0003`), so `purge`/`delete` work against an empty DB:
```bash
SM=$(mktemp -d); mkdir -p "$SM/library"
printf 'inbox_dir="%s/inbox"\nlibrary_root="%s/library"\ndatabase_url="sqlite:%s/library.db"\n' "$SM" "$SM" "$SM" > "$SM/xuewen.toml"
nix develop -c bash -c "
  ./target/debug/xuewen --config '$SM/xuewen.toml' purge --all --yes
  echo '--- delete of a bogus id (should error, exit non-zero) ---'
  ./target/debug/xuewen --config '$SM/xuewen.toml' delete deadbeef; echo \"exit=\$?\"
"
```
Expected: `purge --all --yes` prints `trash is empty`; `delete deadbeef` prints `Error: no paper with id or prefix "deadbeef"` and `exit=1`. (A full delete→hidden→purge on real data is already exercised by the Task 2 db tests; this smoke just confirms CLI wiring + the error path. Manual check — `main`'s arms aren't unit-tested, consistent with `serve`/`refresh`.)

- [ ] **Step 6: Full verification + commit**

Run: `nix develop -c cargo fmt -- --check` then `nix develop -c cargo clippy --all-targets -- -D warnings` then `nix develop -c cargo test`
Expected: fmt clean, clippy clean, whole suite green.
```bash
git add src/main.rs
git -c commit.gpgsign=false commit -m "feat(delete): add delete and purge CLI subcommands"
```

---

## Verification (Definition of Done)

- `nix develop -c cargo test` — whole suite green, including `deleted_at_round_trips` and `soft_delete_hides_and_purge_removes`.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` — clean.
- `xuewen delete <id>` hides a paper from `list_papers`/`stats`/`refresh`; `xuewen purge --all` removes trashed rows + files; both confirm unless `--yes`.
- `refresh` skips trashed papers (they're not in `all_papers`); re-ingesting a trashed paper's content is a `Duplicate`.
- `find_one` lives in `db` and is used by both `refresh` and the delete/purge commands (no duplication).

## Notes for the executor

- This is a **logical** soft-delete: `delete` never moves or touches the PDF; only `purge` deletes the file. Manual recovery is `UPDATE papers SET deleted_at = NULL WHERE id = '…';` — do not add a restore command.
- `get_by_id`/`find_one` intentionally return trashed rows (so delete/purge can act on them); only the *active-view* queries filter `deleted_at IS NULL`.
- `purge` file removal ignores a missing file (already gone) but warns on any other error, then still removes the row.
- Do NOT expose `purge` on the web (that's Plan B's `DELETE` = soft-delete only).
- Every commit uses `git -c commit.gpgsign=false`.
