# Paper Deletion Plan B — Web `DELETE` + Info-Panel Trash Button

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose soft-delete on the web UI: `DELETE /api/papers/:id` (soft-delete via the existing `db::soft_delete`), and a **trash button in the info panel** that confirms, calls the endpoint, and drops the paper from the UI (closes its tab, removes it from the list, refreshes the counts). Purge stays CLI-only.

**Architecture:** A new axum `delete_paper` handler on the existing `/api/papers/{id}` route (added as `.delete(...)` alongside the current `get`). A `deletePaper` API client + a `removePaper` state action (delete → `closeTab` → drop from `library.papers` → reload `stats`). The `InfoPanel` gets a destructive trash button with an in-panel confirm; `App` keys the panel by active id so the confirm state resets per paper.

**Tech Stack:** Rust (axum, sqlx), Svelte 5, TypeScript. Tests: axum-test (Rust), vitest (frontend).

**Environment:** `$IN_NIX_SHELL` is not set — run tooling through the flake dev shell with SEPARATE args: `nix develop -c cargo test`, `nix develop -c npm --prefix frontend run check` (NOT a single quoted string). Commit with `git -c commit.gpgsign=false commit -m "..."` (SSH signing unavailable). Conventional Commits, scope required. Spec: `docs/superpowers/specs/2026-07-07-paper-deletion-design.md` §7.

**Prereqs (merged):** `db::soft_delete(pool, id) -> Result<bool>` exists; the web module (`src/web/{mod,api}.rs`) with `AppState`, `build_router`, `get_paper`, `not_found`/`internal_error` helpers; the Svelte frontend with `lib/{api.ts,state.svelte.ts}`, `components/InfoPanel.svelte`, `App.svelte`.

---

## File Structure

- **Modify** `src/web/api.rs` — add `delete_paper` handler.
- **Modify** `src/web/mod.rs` — add `.delete(api::delete_paper)` to the `/api/papers/{id}` route.
- **Modify** `tests/web_test.rs` — DELETE endpoint test.
- **Modify** `frontend/src/lib/api.ts` — `deletePaper(id)`.
- **Modify** `frontend/src/lib/state.svelte.ts` — `removePaper(id)` action.
- **Modify** `frontend/src/components/InfoPanel.svelte` — trash button + confirm.
- **Modify** `frontend/src/App.svelte` — key `InfoPanel` by active id.
- **Create** `frontend/src/components/InfoPanel.test.ts` — a `removePaper` state test (mocked fetch).

---

## Task 1: Backend `DELETE /api/papers/:id`

**Files:** `src/web/api.rs`, `src/web/mod.rs`, `tests/web_test.rs`.

- [ ] **Step 1: Write the failing endpoint test**

Append to `tests/web_test.rs` (its `temp_pool`/`paper`/`build_router` helpers already exist):
```rust
#[tokio::test]
async fn deletes_a_paper_softly() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "First", "resolved"))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Second", "needs_review"))
        .await
        .unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Before: both listed.
    assert_eq!(server.get("/api/papers").await.json::<Vec<serde_json::Value>>().len(), 2);

    // DELETE one → 200, and it drops out of the active list + stats.
    server.delete("/api/papers/aaaa1111").await.assert_status_ok();
    let list = server.get("/api/papers").await.json::<Vec<serde_json::Value>>();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "bbbb2222");
    assert_eq!(server.get("/api/stats").await.json::<serde_json::Value>()["total"], 1);

    // DELETE an unknown id → 404.
    server
        .delete("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `nix develop -c cargo test --test web_test deletes_a_paper_softly`
Expected: FAIL — no DELETE route yet (the router has no method handler for DELETE on that path → `405`/assertion fails).

- [ ] **Step 3: Add the `delete_paper` handler (`src/web/api.rs`)**

After the `get_paper` handler, add:
```rust
/// Soft-delete a paper (web mutation): flag it deleted; the file is untouched.
/// Purge (permanent removal) is CLI-only. Idempotent on an already-trashed paper.
pub async fn delete_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(_)) => match db::soft_delete(&app.pool, &id).await {
            Ok(_) => Json(serde_json::json!({ "deleted": true })).into_response(),
            Err(e) => {
                tracing::error!("delete_paper: {e}");
                internal_error()
            }
        },
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("delete_paper lookup: {e}");
            internal_error()
        }
    }
}
```

- [ ] **Step 4: Add DELETE to the route (`src/web/mod.rs`)**

Change the `/api/papers/{id}` route to also accept DELETE:
```rust
        .route(
            "/api/papers/{id}",
            get(api::get_paper).delete(api::delete_paper),
        )
```

- [ ] **Step 5: Run the test + full suite + clippy**

Run: `nix develop -c cargo test --test web_test` then `nix develop -c cargo test` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: `deletes_a_paper_softly` passes; whole suite green; clippy clean.

- [ ] **Step 6: Format + commit**

```bash
nix develop -c cargo fmt
git add src/web/api.rs src/web/mod.rs tests/web_test.rs
git -c commit.gpgsign=false commit -m "feat(web): DELETE /api/papers/:id (soft-delete)"
```

---

## Task 2: Frontend trash button + delete flow

**Files:** `frontend/src/lib/api.ts`, `frontend/src/lib/state.svelte.ts`, `frontend/src/components/InfoPanel.svelte`, `frontend/src/App.svelte`, `frontend/src/components/InfoPanel.test.ts`.

- [ ] **Step 1: Add `deletePaper` to the API client (`frontend/src/lib/api.ts`)**

Append:
```ts
export async function deletePaper(id: string): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`delete failed: ${res.status}`);
}
```

- [ ] **Step 2: Add the `removePaper` action (`frontend/src/lib/state.svelte.ts`)**

Add `deletePaper` to the existing `./api` import (it currently imports `getPaper, getStats, listPapers`):
```ts
import { deletePaper, getPaper, getStats, listPapers } from './api';
```
Then add this function (after `loadDetail`, or anywhere at module top-level after the state declarations):
```ts
/// Soft-delete a paper on the server, then drop it from the UI: close its tab,
/// remove it from the list, and refresh the counts.
export async function removePaper(id: string): Promise<void> {
  await deletePaper(id);
  closeTab(id);
  library.papers = library.papers.filter((p) => p.id !== id);
  await loadStats();
}
```

- [ ] **Step 3: Add the trash button + confirm to `InfoPanel.svelte`**

Change the lucide import to add `Trash2`:
```ts
  import { ExternalLink, Trash2 } from 'lucide-svelte';
```
Change the state import to add `removePaper`:
```ts
  import { loadDetail, removePaper } from '../lib/state.svelte';
```
Add local component state + a handler in the `<script>` (after `let { id }: { id: string } = $props();`):
```ts
  let confirming = $state(false);
  let deleting = $state(false);
  async function doDelete() {
    deleting = true;
    try {
      await removePaper(id);
      // On success the tab closes and this panel unmounts — no reset needed.
    } catch (e) {
      console.error(e);
      deleting = false;
      confirming = false;
    }
  }
```
In the markup, inside the `{:then d}` block, add a delete section as the LAST child (after the `{#if d.abstract}…{/if}` block, before `{:catch}`):
```svelte
    <div class="mt-6 border-t border-slate-200 pt-4 dark:border-slate-800">
      {#if confirming}
        <div class="flex items-center gap-2">
          <span class="text-sm text-slate-600 dark:text-slate-300">Delete this paper?</span>
          <button
            type="button"
            onclick={doDelete}
            disabled={deleting}
            class="rounded-lg bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700 disabled:opacity-50"
          >
            Delete
          </button>
          <button
            type="button"
            onclick={() => (confirming = false)}
            class="rounded-lg px-3 py-1 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Cancel
          </button>
        </div>
      {:else}
        <button
          type="button"
          onclick={() => (confirming = true)}
          class="inline-flex items-center gap-1.5 rounded-lg border border-red-200 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-500/10"
        >
          <Trash2 size={14} /> Delete paper
        </button>
      {/if}
    </div>
```

- [ ] **Step 4: Key `InfoPanel` by active id (`frontend/src/App.svelte`)**

So the confirm state resets when the active paper changes, wrap the panel in a `{#key}`:
```svelte
          {#if viewer.infoOpen && viewer.activeId}
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
```

- [ ] **Step 5: Add a `removePaper` state test (`frontend/src/components/InfoPanel.test.ts`)**

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { library, openTab, removePaper, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

describe('removePaper', () => {
  beforeEach(() => {
    library.papers = [];
    viewer.tabs = [];
    viewer.activeId = null;
    // All fetches (DELETE + the follow-up /api/stats) succeed.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ total: 0, resolved: 0, needs_review: 0 }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('deletes on the server, closes the tab, and drops it from the list', async () => {
    library.papers = [paper('x'), paper('y')];
    openTab(paper('x'));
    expect(viewer.tabs.length).toBe(1);

    await removePaper('x');

    expect(library.papers.map((p) => p.id)).toEqual(['y']);
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
    // fetch was called for the DELETE (and the stats refresh).
    expect((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0][1]).toMatchObject({
      method: 'DELETE',
    });
  });
});
```

- [ ] **Step 6: Type-check, test, build**

Run: `nix develop -c npm --prefix frontend run check` then `nix develop -c npm --prefix frontend test` then `nix develop -c npm --prefix frontend run build`
Expected: 0 type errors; all frontend tests pass (the 3 existing + `removePaper`); build succeeds.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/api.ts frontend/src/lib/state.svelte.ts frontend/src/components/InfoPanel.svelte frontend/src/App.svelte frontend/src/components/InfoPanel.test.ts
git -c commit.gpgsign=false commit -m "feat(web): info-panel delete button + delete flow"
```

---

## Verification (Definition of Done)

- `nix develop -c cargo test` — whole suite green incl. `deletes_a_paper_softly` (DELETE soft-deletes; the paper leaves `GET /api/papers` and `stats`; unknown id → 404).
- `nix develop -c npm --prefix frontend test` + `... run check` + `... run build` — all pass (incl. `removePaper`).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` — clean.
- End-to-end (after `npm run build` + `cargo build` + `serve` against a seeded library): opening a paper → info toggle → **Delete paper** → confirm → the paper vanishes from the sidebar, its tab closes, and the counts drop; the CLI `xuewen purge --all` then permanently removes it.

## Notes for the executor

- The web endpoint is **soft-delete only** — it calls `db::soft_delete`, never `db::delete_row`. Purge is deliberately absent from the web (CLI only), so the web can trash but never permanently destroy.
- `delete_paper` is idempotent: a DELETE on an already-trashed paper still returns `200` (the id exists); only a genuinely unknown id is `404`.
- The frontend `removePaper` updates local state after the server confirms, so the UI stays consistent without a full reload.
- Keying `InfoPanel` by `viewer.activeId` (Task 2 Step 4) is what resets the confirm state when you switch papers — don't skip it.
- Every commit uses `git -c commit.gpgsign=false`.
