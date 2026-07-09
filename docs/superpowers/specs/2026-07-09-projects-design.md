# Design: Projects (aggregate related papers)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-09
**Status:** Approved (design phase)

## 1. Purpose

When writing a manuscript, the author wants to gather the papers relevant to it
into one named group and, while writing, narrow the library down to just that
group. Today the library is a single flat list filterable only by status, sort,
and a title/author search — there is no way to say "these N papers belong to the
survey I'm drafting."

This feature adds **projects**: named, many-to-many groupings of papers. A paper
can belong to several projects (a survey cited by two manuscripts lives in both);
a project can hold any number of papers. Projects carry a name and an optional
free-text note (e.g. the manuscript's working title or a scope reminder).

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Membership | **Many-to-many** (tag-like) — join table `paper_projects` |
| Surfaces | **Web UI + CLI** |
| Project fields | **Name + optional note** (no per-paper-in-project notes) |
| Name uniqueness | **Case-insensitive unique** (`COLLATE NOCASE`) |
| Delete semantics | Deleting a project removes its memberships only; **papers untouched** |
| Paper delete → membership | Cascade via FK (`ON DELETE CASCADE`) once `PRAGMA foreign_keys=ON` |
| UI integration | **Approach A**: Sidebar project filter + InfoPanel project chips + a Projects modal |

**Out of scope (YAGNI for v1):** nested/sub-projects, per-paper notes within a
project, drag-and-drop reordering, per-project export/BibTeX, sharing.

## 3. Data model

Two new migrations under `migrations/`.

```sql
-- 0005_add_projects.sql
CREATE TABLE projects (
  id         TEXT PRIMARY KEY,       -- uuid, same scheme as papers.id
  name       TEXT NOT NULL,
  note       TEXT,                   -- NULL = no note
  created_at TEXT NOT NULL           -- RFC-3339
);
CREATE UNIQUE INDEX idx_projects_name ON projects(name COLLATE NOCASE);

-- 0006_add_paper_projects.sql
CREATE TABLE paper_projects (
  paper_id   TEXT NOT NULL REFERENCES papers(id)   ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  added_at   TEXT NOT NULL,
  PRIMARY KEY (paper_id, project_id)
);
CREATE INDEX idx_paper_projects_project ON paper_projects(project_id);
```

**Foreign keys.** SQLite ignores FK clauses unless `PRAGMA foreign_keys=ON` is
set per-connection. `db::connect` currently does not set it; the design enables
it on the pool's connect options:

```rust
let opts = SqliteConnectOptions::from_str(database_url)?
    .create_if_missing(true)
    .foreign_keys(true);
```

There are no pre-existing FK constraints, so enabling it changes no current
behavior; it only makes the new cascades fire. `db::delete_row` (hard purge) and
`projects` delete both rely on the cascade, so no manual membership cleanup is
needed in code.

## 4. Backend

### 4.1 Model (`src/models.rs`)

```rust
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub created_at: String,
}
```

A list-with-count struct `ProjectSummary { #[serde(flatten)] project: Project,
paper_count: i64 }` lives in `models.rs` next to `Project` (so `db` can return it
without depending on the web layer, matching how `db::stats` returns plain data).
It is populated by a `LEFT JOIN … GROUP BY` count query and serialized directly by
the `GET /api/projects` handler.

### 4.2 Query layer (`src/db.rs`)

New functions (mirroring existing signatures/error handling):

- `create_project(pool, name, note) -> Result<Project>` — generates id + timestamp; a
  unique-name violation surfaces via the existing `is_unique_violation` helper.
- `list_projects(pool) -> Result<Vec<ProjectSummary>>` — projects + membership counts,
  ordered by `name COLLATE NOCASE`.
- `get_project(pool, id) -> Result<Option<Project>>`
- `update_project(pool, id, name, note) -> Result<bool>` — rename / edit note.
- `delete_project(pool, id) -> Result<bool>`
- `add_paper_to_project(pool, paper_id, project_id) -> Result<()>` — `INSERT … ON CONFLICT
  DO NOTHING` (idempotent).
- `remove_paper_from_project(pool, paper_id, project_id) -> Result<bool>`
- `project_ids_for_paper(pool, paper_id) -> Result<Vec<String>>`
- `list_papers` gains an optional `project: Option<&str>` argument; when present it adds
  `AND id IN (SELECT paper_id FROM paper_projects WHERE project_id = ?)` (keeps the
  existing `deleted_at IS NULL`, search, status, and whitelisted-sort logic intact).

### 4.3 HTTP API (`src/web/mod.rs` routes + `src/web/api.rs` handlers)

| Method & path | Body / query | Result |
|---|---|---|
| `GET /api/projects` | — | `[ProjectSummary]` |
| `POST /api/projects` | `{name, note?}` | `201` Project; **409** on duplicate name |
| `PATCH /api/projects/{id}` | `{name?, note?}` | `200` Project; 404 / 409 |
| `DELETE /api/projects/{id}` | — | `204`; 404 if missing |
| `PUT /api/papers/{paper_id}/projects/{project_id}` | — | `204` (idempotent); 404 if either id unknown |
| `DELETE /api/papers/{paper_id}/projects/{project_id}` | — | `204`; 404 if membership absent |
| `GET /api/papers?project={id}` | existing + `project` | filtered `[Paper]` |

The single-paper response (`GET /api/papers/{id}`) gains `project_ids: [String]` so the
InfoPanel can render current memberships without a second round-trip. (`list_papers`
rows do **not** carry `project_ids` — the list stays lean.)

### 4.4 CLI (`src/main.rs`)

New `Project` variant on `enum Command` with a nested `#[derive(Subcommand)] enum
ProjectCmd`:

```
xuewen project list                              # name, note, #papers
xuewen project new <name> [--note <text>]
xuewen project rm <name|id>
xuewen project add <project> <paper-id-prefix>…  # add one or more papers
xuewen project remove <project> <paper-id-prefix>
xuewen project show <project>                    # papers in the project
```

`<project>` resolves by exact name (case-insensitive) first, then by id prefix.
Paper lookup reuses `db::find_by_id_prefix`; an ambiguous or missing prefix errors the
same way existing paper commands do.

## 5. Frontend (`frontend/src/`)

### 5.1 State & client

- `lib/types.ts`: `Project = { id; name; note: string | null; paper_count: number }`; add
  `project_ids?: string[]` to the single-paper type; `filters.project: string` (`'all'` or id).
- `lib/api.ts`: `listProjects`, `createProject`, `updateProject`, `deleteProject`,
  `addPaperToProject`, `removePaperFromProject`; `listPapers` sends `project` when not `'all'`.
- `lib/state.svelte.ts`: `projects` state + `loadProjects()`; `loadPapers()` includes the
  project filter; membership mutations refresh the affected paper and project counts.

### 5.2 Components

- **Sidebar.svelte**: a "Project" `<select>` next to status/sort (`All projects` + one
  option per project), and a small ＋ button opening the Projects modal.
- **ProjectsModal.svelte** (new, patterned on ImportModal): list projects; create
  (name + note); rename / edit note; delete (with a confirm on delete).
- **InfoPanel.svelte**: a "Projects" section — a chip per project the paper belongs to
  (× removes), plus an "Add to project" dropdown whose last item is "New project…".

### 5.3 Flow

`onMount` → `loadProjects()` alongside the existing loads. Selecting a project in the
Sidebar sets `filters.project` and calls `loadPapers()`. Adding/removing membership in the
InfoPanel calls the membership endpoint, then refreshes the paper's `project_ids` and the
sidebar counts. Deleting a project that is the active filter resets the filter to `'all'`.

## 6. Testing

Following the repo's existing test style:

- **Rust `db` tests**: create + duplicate-name rejection; rename; delete cascades
  memberships; add/remove membership idempotency; `list_papers` project filter returns
  only members; hard-purging a paper drops its memberships (FK cascade).
- **Rust `web` test**: an end-to-end round-trip (create project → add paper → list papers
  filtered by project → delete project).
- **CLI**: a smoke test for `project new`/`add`/`show` if the existing CLI test harness
  supports it; otherwise covered by the db/web tests.
- **Frontend**: a `ProjectsModal` component test (create/rename/delete render + calls) and
  a `state` test for the project filter feeding `loadPapers()`.

## 7. Risks / notes

- **FK pragma**: enabling `foreign_keys=ON` is per-connection; because it's set on the
  pool connect options every pooled connection gets it. Verified there are no existing FK
  constraints that could newly fail.
- **Unique-name races**: two rapid creates with the same name — the DB unique index is the
  source of truth; the second returns 409. No app-level locking needed.
- **Migration ordering**: `paper_projects` references `projects`, so it must be the later
  migration (0006 after 0005).
