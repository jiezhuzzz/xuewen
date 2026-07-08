# Design: Paper Deletion (logical soft-delete + trash)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-07
**Status:** Approved (design phase)

## 1. Purpose

Let a user remove papers they don't want (bad ingests, duplicates, junk) from
both the **CLI** and the **web UI**. Deletion is a **logical soft-delete**: a
trashed paper is flagged, not physically moved, so it drops out of every active
view but remains trivially recoverable. A separate **purge** is the only
permanent removal (row + PDF file).

Deletion is the first *mutation* the web UI exposes; the read-only design is
relaxed for exactly this one operation, per the user's choice. Because the server
has no authentication and may be bound to a LAN, the web delete is deliberately
**soft** (recoverable) so an accidental or hostile delete lands in the trash, not
oblivion.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Surfaces | **CLI + web UI**, web delete open to any client |
| Semantics | **Soft delete** (recoverable), **logical** (a flag; no file move) |
| Restore | **No command** — manual (one SQL update), documented |
| Purge | CLI only; permanent (row + file) |
| Web delete placement | **Info panel** trash button (deliberate, not a row hover) |
| Persist deleted state | new nullable **`deleted_at TEXT`** column (migration `0003`) |

## 3. Schema change

`migrations/0003_add_deleted_at.sql`:
```sql
ALTER TABLE papers ADD COLUMN deleted_at TEXT;
```
- `NULL` = active; an RFC-3339 timestamp = trashed. Existing rows get `NULL`.
- Add `deleted_at: Option<String>` to the `Paper` struct (after `added_at`);
  `SELECT *` / `FromRow` picks it up. Add it to `insert_paper`'s column list
  (bound as `NULL` on ingest) and to `update_paper`'s SET list so refresh/other
  updates don't clobber it. Not indexed (personal-library scale).

## 4. Delete / purge mechanics

- **Soft-delete** (`db::soft_delete(pool, id)`): `UPDATE papers SET deleted_at =
  <now> WHERE id = ? AND deleted_at IS NULL`. The PDF is **not** moved or touched.
  Returns whether a row was affected (so the caller can report "already deleted /
  not found").
- **Purge** (`db::purge(pool, id)`): read the row, delete the library file at
  `library_root/rel_path` (ignore a missing file), then `DELETE FROM papers WHERE
  id = ?`. Permanent.
- **Manual recovery** (documented, no code): `UPDATE papers SET deleted_at = NULL
  WHERE id = '…';` — the file never moved, so this fully restores the paper.

## 5. Active-view filtering

Trashed papers must disappear from every "live" surface. Add `WHERE deleted_at IS
NULL` (ANDed with existing predicates) to:
- `db::list_papers` (web sidebar list),
- `db::stats` (header counts),
- `db::all_papers` (used by `refresh` — trashed papers are not re-resolved/re-filed).

`db::get_by_id` is **unchanged** (returns a row regardless of `deleted_at`) so the
delete/purge commands can act on any paper. New query `db::trashed_papers(pool) ->
Vec<Paper>` (`WHERE deleted_at IS NOT NULL`) backs `purge --all` and a future
trash view.

**Dedup interaction (documented behavior):** a soft-deleted row still holds its
`content_hash`/`doi`/`arxiv_id`/`cite_key`, so `exists_by_hash` still matches it —
re-ingesting the same PDF while it's trashed is a **duplicate no-op**, and its
cite-key filename stays reserved (new ingests disambiguate around it). To truly
re-add it, un-delete it or `purge` then re-ingest.

## 6. CLI

Two new subcommands (reuse `find_one` from `refresh` for exact-or-unique-prefix id
resolution):
- `xuewen delete <ID> [--yes]` — soft-delete the paper. Prints the title and asks
  `Delete "<title>"? [y/N]` unless `--yes`. If already trashed or not found → a
  clear message, non-zero exit.
- `xuewen purge [<ID>] [--all] [--yes]` — permanently remove trashed papers:
  `<ID>` purges one (must already be trashed), `--all` purges every trashed paper;
  `<ID>` and `--all` are mutually exclusive (clap `conflicts_with`). Confirms
  (`Permanently delete N paper(s) and their files? [y/N]`) unless `--yes`. Purging
  a paper that isn't trashed → refuse with a message (purge only acts on trash).

Confirmation prompt reads a line from stdin; `--yes` skips it (for scripts).

## 7. Web

- **Endpoint:** `DELETE /api/papers/{id}` → `db::soft_delete`; `200` on success
  (JSON `{"deleted": true}`), `404` if the id doesn't exist. Open to any client
  (no auth — matches the current server posture). Purge is **not** exposed on the
  web (CLI only), so the web can only trash, never permanently destroy.
- **Frontend:** a **trash button** in the `InfoPanel` (bottom, subtle/destructive
  styling). Clicking asks for a lightweight in-panel confirm ("Delete this paper?
  Delete / Cancel"). On confirm: `DELETE /api/papers/{id}`, then locally
  `closeTab(id)`, drop it from `library.papers`, and refresh `stats` — so it
  disappears without a full reload. The API client gains `deletePaper(id)`.

## 8. Error handling

- CLI: unknown/ambiguous id → the same `find_one` errors as `refresh`; a failed
  file delete during purge → warn and still remove the row (don't leave an
  un-purgeable ghost); DB errors bubble up via `anyhow`.
- Web: `DELETE` on an unknown id → `404` JSON; DB error → `500` JSON, logged.
- A `delete` on an already-trashed paper is a no-op success (idempotent); `purge`
  on a non-trashed paper is refused.

## 9. Testing

- **db unit:** `soft_delete` sets `deleted_at` and hides the row from
  `list_papers`/`stats`/`all_papers`; `trashed_papers` returns only trashed;
  `purge` removes the row; `update_paper` round-trips `deleted_at`.
- **pipeline/refresh:** a soft-deleted paper is skipped by `refresh` (not in
  `all_papers`); re-ingesting a trashed paper's content is a `Duplicate`.
- **CLI:** covered by db + a manual smoke (`delete`/`purge --all` against a temp
  library) — the `main` arms themselves aren't unit-tested (consistent with the
  existing `serve`/`refresh` arms).
- **web:** `DELETE /api/papers/:id` soft-deletes (the row then absent from
  `GET /api/papers`), `404` on unknown id; purge is absent from the web surface.
- **frontend:** a component/smoke test that the info panel's delete button calls
  `deletePaper` and removes the tab (mocked fetch).

## 10. Decomposition (implementation plans)

- **Plan A — backend + CLI:** migration `0003` + `Paper.deleted_at` (+
  insert/update), `db::{soft_delete, purge, trashed_papers}` + active-view
  filtering on `list_papers`/`stats`/`all_papers`, the `delete`/`purge` CLI
  subcommands, tests. Fully Rust-tested; no web/frontend.
- **Plan B — web delete:** `DELETE /api/papers/{id}` handler + route, the
  `deletePaper` API client + `InfoPanel` trash button/confirm + local state
  update, tests.

## 11. Out of scope

- An automated `restore` command / web restore (manual SQL update is the path).
- A web "trash" view / listing trashed papers in the UI.
- Purge from the web UI (CLI only — the web can trash but not permanently destroy).
- Authentication / per-user permissions (unchanged; the soft-delete is the
  mitigation for the open web delete).
- Bulk/multi-select delete in the UI.
