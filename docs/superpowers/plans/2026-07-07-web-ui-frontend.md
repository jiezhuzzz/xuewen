# Web UI Plan B — Svelte Reader Frontend

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A modern, beautiful, read-only Svelte SPA — searchable sidebar + tabbed inline PDF viewer (multiple PDFs open at once) + slide-over metadata panel, dark/light — that consumes the Plan A JSON API and is embedded into the `xuewen` binary.

**Architecture:** A `frontend/` Vite + Svelte 5 (runes) + TypeScript + Tailwind CSS 4 app. A typed API client (`lib/api.ts`) + shared reactive state (`lib/state.svelte.ts`, rune-based). Components: `TopBar` (stats + theme), `Sidebar` (search/filter/sort/list), `Viewer` (tab bar + keep-alive iframe PDF tabs + info panel + empty-state grid). `npm run build` → `frontend/dist/`, embedded by the existing `rust-embed` `Assets` (Plan A). Dev uses Vite's server proxying `/api` + `/papers` to `xuewen serve` on :8080.

**Tech Stack:** Svelte 5, Vite 6, TypeScript, Tailwind CSS 4 (`@tailwindcss/vite`), lucide-svelte, `@fontsource-variable/inter`; tests: vitest + `@testing-library/svelte` + jsdom.

**Environment:** `$IN_NIX_SHELL` is not set — run tooling through the flake dev shell with SEPARATE args: `nix develop -c npm --prefix frontend run build`, `nix develop -c cargo build` (NOT a single quoted string). After Task 1 adds `nodejs` to `flake.nix`, `node`/`npm` are available inside `nix develop -c ...`. Commit with `git -c commit.gpgsign=false commit -m "..."` (SSH signing unavailable). Conventional Commits, scope required, types feat/fix/docs/chore/ci. Keep Rust rustfmt-clean (this plan barely touches Rust). Spec: `docs/superpowers/specs/2026-07-07-web-ui-design.md` §5.

**Prereqs:** Plan A is merged — `xuewen serve` + JSON API + `/papers/:id/pdf` + the `rust-embed` `Assets { #[folder="frontend/dist"] }` + the `build.rs` placeholder all exist. `frontend/dist/` is git-ignored.

---

## File Structure

- **Modify** `flake.nix` — add `nodejs` to the devShell packages.
- **Modify** `.gitignore` — add `/frontend/node_modules/`.
- **Create** `frontend/` — `package.json`, `package-lock.json` (committed), `vite.config.ts`, `svelte.config.js`, `tsconfig.json`, `tsconfig.node.json`, `index.html`, and `src/` (`main.ts`, `app.css`, `vite-env.d.ts`, `App.svelte`, `lib/{types.ts,api.ts,state.svelte.ts}`, `components/*.svelte`, `components/*.test.ts`).

`frontend/dist/` (build output) and `frontend/node_modules/` stay git-ignored. No Rust source changes except possibly none.

---

## Task 1: Toolchain, scaffold, and build wiring

**Files:** `flake.nix`, `.gitignore`, `frontend/{package.json,vite.config.ts,svelte.config.js,tsconfig.json,tsconfig.node.json,index.html}`, `frontend/src/{main.ts,app.css,vite-env.d.ts,App.svelte}`.

- [ ] **Step 1: Add `nodejs` to the dev shell**

In `flake.nix`, add `nodejs` to the `packages` list (after `sqlite`):
```nix
          packages = with pkgs; [
            cargo rustc rustfmt clippy rust-analyzer
            poppler-utils   # provides `pdftotext`
            sqlite
            nodejs          # frontend build (npm)
            pkg-config
          ];
```

- [ ] **Step 2: Ignore node_modules**

In `.gitignore`, add:
```
/frontend/node_modules/
```

- [ ] **Step 3: Create the frontend package manifest**

`frontend/package.json`:
```json
{
  "name": "xuewen-frontend",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "check": "svelte-check --tsconfig ./tsconfig.json",
    "test": "vitest run"
  },
  "dependencies": {
    "@fontsource-variable/inter": "^5.1.0",
    "lucide-svelte": "^0.468.0"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5.0.0",
    "@tailwindcss/vite": "^4.0.0",
    "@testing-library/jest-dom": "^6.6.0",
    "@testing-library/svelte": "^5.2.0",
    "@tsconfig/svelte": "^5.0.0",
    "jsdom": "^25.0.0",
    "svelte": "^5.15.0",
    "svelte-check": "^4.1.0",
    "tailwindcss": "^4.0.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0",
    "vitest": "^2.1.0"
  }
}
```
(If any `^` range fails to resolve at `npm install` time, take the nearest published version and note it in your report — keep the major versions: Svelte 5, Vite 6, Tailwind 4.)

- [ ] **Step 4: Vite + Svelte + Tailwind config**

`frontend/vite.config.ts`:
```ts
/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  build: { outDir: 'dist', emptyOutDir: true },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/papers': 'http://127.0.0.1:8080',
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
  },
});
```

`frontend/svelte.config.js`:
```js
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default { preprocess: vitePreprocess() };
```

`frontend/tsconfig.json`:
```json
{
  "extends": "@tsconfig/svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "allowJs": true,
    "checkJs": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "strict": true,
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src/**/*.ts", "src/**/*.svelte", "src/**/*.d.ts"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

`frontend/tsconfig.node.json`:
```json
{
  "compilerOptions": {
    "composite": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "types": ["node"]
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 5: HTML entry, CSS, and env types**

`frontend/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Xuewen</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`frontend/src/app.css`:
```css
@import 'tailwindcss';
@import '@fontsource-variable/inter';

/* Class-based dark mode (toggled on <html> by the theme state). */
@custom-variant dark (&:where(.dark, .dark *));

:root {
  color-scheme: light dark;
}

html,
body,
#app {
  height: 100%;
}

body {
  font-family: 'Inter Variable', system-ui, -apple-system, sans-serif;
}
```

`frontend/src/vite-env.d.ts`:
```ts
/// <reference types="svelte" />
/// <reference types="vite/client" />
```

- [ ] **Step 6: Entry point + minimal App**

`frontend/src/main.ts`:
```ts
import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

const app = mount(App, { target: document.getElementById('app')! });

export default app;
```

`frontend/src/App.svelte`:
```svelte
<script lang="ts">
</script>

<main class="grid min-h-full place-items-center bg-white p-8 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
  <h1 class="text-2xl font-semibold">Xuewen</h1>
</main>
```

- [ ] **Step 7: Install, build, and verify the embed pipeline**

Run: `nix develop -c npm --prefix frontend install`
Then: `nix develop -c npm --prefix frontend run build`
Expected: `frontend/dist/index.html` + `frontend/dist/assets/*` produced (real build, replacing the placeholder).

Then verify the Rust binary embeds and serves it:
```bash
nix develop -c cargo build
SM=$(mktemp -d); mkdir -p "$SM/library"
printf 'inbox_dir="%s/inbox"\nlibrary_root="%s/library"\ndatabase_url="sqlite:%s/library.db"\n' "$SM" "$SM" "$SM" > "$SM/xuewen.toml"
nix develop -c bash -c "timeout 3 ./target/debug/xuewen --config '$SM/xuewen.toml' serve --port 8138 & sleep 1; curl -s http://127.0.0.1:8138/ | head -c 200; echo"
```
Expected: `/` returns the built app's HTML (contains `<div id=\"app\">` and a `/assets/…` script tag), NOT the plain placeholder paragraph.

- [ ] **Step 8: Commit**

```bash
git add flake.nix .gitignore frontend/package.json frontend/package-lock.json frontend/vite.config.ts frontend/svelte.config.js frontend/tsconfig.json frontend/tsconfig.node.json frontend/index.html frontend/src/main.ts frontend/src/app.css frontend/src/vite-env.d.ts frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(web): scaffold Svelte+Vite+Tailwind frontend and embed pipeline"
```

---

## Task 2: Types, API client, and reactive state

**Files:** create `frontend/src/lib/{types.ts,api.ts,state.svelte.ts}`.

- [ ] **Step 1: Types (`frontend/src/lib/types.ts`)** — mirror the Rust DTOs:
```ts
export interface PaperSummary {
  id: string;
  title: string | null;
  authors: string[];
  venue: string | null;
  year: number | null;
  doi: string | null;
  arxiv_id: string | null;
  dblp_key: string | null;
  cite_key: string | null;
  url: string | null;
  source: string | null;
  status: string;
  added_at: string;
}

export interface PaperDetail extends PaperSummary {
  abstract: string | null;
}

export interface Stats {
  total: number;
  resolved: number;
  needs_review: number;
}

export type StatusFilter = 'all' | 'resolved' | 'needs_review';
export type Sort = 'year_desc' | 'year_asc' | 'added_desc' | 'title';

export interface Filters {
  q: string;
  status: StatusFilter;
  sort: Sort;
}
```

- [ ] **Step 2: API client (`frontend/src/lib/api.ts`)**:
```ts
import type { Filters, PaperDetail, PaperSummary, Stats } from './types';

export async function listPapers(f: Filters): Promise<PaperSummary[]> {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  params.set('sort', f.sort);
  const res = await fetch(`/api/papers?${params.toString()}`);
  if (!res.ok) throw new Error(`list failed: ${res.status}`);
  return res.json();
}

export async function getPaper(id: string): Promise<PaperDetail> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}`);
  if (!res.ok) throw new Error(`detail failed: ${res.status}`);
  return res.json();
}

export async function getStats(): Promise<Stats> {
  const res = await fetch('/api/stats');
  if (!res.ok) throw new Error(`stats failed: ${res.status}`);
  return res.json();
}

export function pdfUrl(id: string): string {
  return `/papers/${encodeURIComponent(id)}/pdf`;
}
```

- [ ] **Step 3: Reactive state (`frontend/src/lib/state.svelte.ts`)** — Svelte 5 runes shared across the app. All exports are `$state` OBJECTS whose *properties* are mutated (never reassign an exported binding):
```ts
import { getPaper, getStats, listPapers } from './api';
import type { Filters, PaperDetail, PaperSummary, Stats } from './types';

export const filters = $state<Filters>({ q: '', status: 'all', sort: 'year_desc' });

export const library = $state<{
  papers: PaperSummary[];
  loading: boolean;
  error: string | null;
}>({ papers: [], loading: false, error: null });

export const stats = $state<{ value: Stats | null }>({ value: null });

export interface Tab {
  id: string;
  title: string;
}
export const viewer = $state<{ tabs: Tab[]; activeId: string | null; infoOpen: boolean }>({
  tabs: [],
  activeId: null,
  infoOpen: false,
});

export const theme = $state<{ mode: 'light' | 'dark' }>({ mode: 'light' });

const detailCache = new Map<string, PaperDetail>();

export async function loadStats(): Promise<void> {
  try {
    stats.value = await getStats();
  } catch (e) {
    console.error(e);
  }
}

export async function loadPapers(): Promise<void> {
  library.loading = true;
  library.error = null;
  try {
    library.papers = await listPapers({ ...filters });
  } catch (e) {
    library.error = (e as Error).message;
  } finally {
    library.loading = false;
  }
}

let debounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  clearTimeout(debounce);
  debounce = setTimeout(loadPapers, 200);
}

export function openTab(p: PaperSummary): void {
  if (!viewer.tabs.some((t) => t.id === p.id)) {
    viewer.tabs.push({ id: p.id, title: p.title ?? p.cite_key ?? p.id });
  }
  viewer.activeId = p.id;
}

export function closeTab(id: string): void {
  const idx = viewer.tabs.findIndex((t) => t.id === id);
  if (idx === -1) return;
  viewer.tabs.splice(idx, 1);
  if (viewer.activeId === id) {
    viewer.activeId = viewer.tabs[Math.max(0, idx - 1)]?.id ?? null;
  }
}

export async function loadDetail(id: string): Promise<PaperDetail> {
  const cached = detailCache.get(id);
  if (cached) return cached;
  const d = await getPaper(id);
  detailCache.set(id, d);
  return d;
}

function applyTheme(): void {
  document.documentElement.classList.toggle('dark', theme.mode === 'dark');
}
export function initTheme(): void {
  const saved = localStorage.getItem('xuewen-theme');
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  theme.mode = saved === 'dark' || (!saved && prefersDark) ? 'dark' : 'light';
  applyTheme();
}
export function toggleTheme(): void {
  theme.mode = theme.mode === 'dark' ? 'light' : 'dark';
  localStorage.setItem('xuewen-theme', theme.mode);
  applyTheme();
}
```

- [ ] **Step 4: Type-check**

Run: `nix develop -c npm --prefix frontend run check`
Expected: no type errors. (No runtime test yet — components come next.)

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/
git -c commit.gpgsign=false commit -m "feat(web): frontend types, API client, and reactive state"
```

---

## Task 3: Shell + Sidebar (search / filter / sort / list)

**Files:** create `frontend/src/components/{StatusPill.svelte,PaperRow.svelte,Sidebar.svelte,TopBar.svelte}`; rewrite `frontend/src/App.svelte`.

- [ ] **Step 1: `StatusPill.svelte`** — colored status chip:
```svelte
<script lang="ts">
  let { status }: { status: string } = $props();
  const resolved = $derived(status === 'resolved');
</script>

<span
  class={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
    resolved
      ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
      : 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-400'
  }`}
>
  {resolved ? 'resolved' : 'needs review'}
</span>
```

- [ ] **Step 2: `PaperRow.svelte`** — one paper in the sidebar list:
```svelte
<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { openTab, viewer } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { paper }: { paper: PaperSummary } = $props();
  const active = $derived(viewer.activeId === paper.id);
  const authors = $derived(
    paper.authors.length > 3
      ? `${paper.authors.slice(0, 3).join(', ')} et al.`
      : paper.authors.join(', '),
  );
</script>

<button
  type="button"
  onclick={() => openTab(paper)}
  class={`w-full border-l-2 px-4 py-3 text-left transition hover:bg-slate-50 dark:hover:bg-slate-800/50 ${
    active
      ? 'border-indigo-500 bg-slate-50 dark:bg-slate-800/50'
      : 'border-transparent'
  }`}
>
  <div class="line-clamp-2 text-sm font-medium text-slate-900 dark:text-slate-100">
    {paper.title ?? '(untitled)'}
  </div>
  {#if authors}
    <div class="mt-0.5 line-clamp-1 text-xs text-slate-500 dark:text-slate-400">{authors}</div>
  {/if}
  <div class="mt-1.5 flex items-center gap-2 text-xs text-slate-400">
    {#if paper.year}<span>{paper.year}</span>{/if}
    {#if paper.venue}<span class="truncate">· {paper.venue}</span>{/if}
    <StatusPill status={paper.status} />
  </div>
</button>
```

- [ ] **Step 3: `Sidebar.svelte`** — search + filters + list:
```svelte
<script lang="ts">
  import { Search } from 'lucide-svelte';
  import { filters, library, loadPapers, setSearch } from '../lib/state.svelte';
  import type { Sort, StatusFilter } from '../lib/types';
  import PaperRow from './PaperRow.svelte';

  function onStatus(e: Event) {
    filters.status = (e.currentTarget as HTMLSelectElement).value as StatusFilter;
    loadPapers();
  }
  function onSort(e: Event) {
    filters.sort = (e.currentTarget as HTMLSelectElement).value as Sort;
    loadPapers();
  }
</script>

<aside class="flex h-full w-80 shrink-0 flex-col border-r border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
  <div class="space-y-3 border-b border-slate-200 p-3 dark:border-slate-800">
    <div class="relative">
      <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-slate-400" />
      <input
        type="search"
        placeholder="Search title or author…"
        value={filters.q}
        oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
        class="w-full rounded-lg border border-slate-200 bg-slate-50 py-2 pl-9 pr-3 text-sm outline-none focus:border-indigo-400 focus:ring-2 focus:ring-indigo-500/20 dark:border-slate-700 dark:bg-slate-800"
      />
    </div>
    <div class="flex gap-2">
      <select
        onchange={onStatus}
        class="flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-700 dark:bg-slate-800"
      >
        <option value="all">All status</option>
        <option value="resolved">Resolved</option>
        <option value="needs_review">Needs review</option>
      </select>
      <select
        onchange={onSort}
        class="flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-700 dark:bg-slate-800"
      >
        <option value="year_desc">Newest</option>
        <option value="year_asc">Oldest</option>
        <option value="added_desc">Recently added</option>
        <option value="title">Title A–Z</option>
      </select>
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto divide-y divide-slate-100 dark:divide-slate-800/60">
    {#if library.loading}
      <p class="p-4 text-sm text-slate-400">Loading…</p>
    {:else if library.error}
      <p class="p-4 text-sm text-red-500">{library.error}</p>
    {:else if library.papers.length === 0}
      <p class="p-4 text-sm text-slate-400">No papers match.</p>
    {:else}
      {#each library.papers as paper (paper.id)}
        <PaperRow {paper} />
      {/each}
    {/if}
  </div>
</aside>
```

- [ ] **Step 4: `TopBar.svelte`** — title, stats, theme toggle:
```svelte
<script lang="ts">
  import { Library, Moon, Sun } from 'lucide-svelte';
  import { stats, theme, toggleTheme } from '../lib/state.svelte';
</script>

<header class="flex h-14 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 dark:border-slate-800 dark:bg-slate-900">
  <div class="flex items-center gap-2">
    <Library size={20} class="text-indigo-500" />
    <span class="text-lg font-semibold tracking-tight">Xuewen</span>
  </div>
  <div class="flex items-center gap-4">
    {#if stats.value}
      <div class="hidden items-center gap-3 text-xs text-slate-500 sm:flex dark:text-slate-400">
        <span>{stats.value.total} papers</span>
        <span class="text-emerald-600 dark:text-emerald-400">{stats.value.resolved} resolved</span>
        <span class="text-amber-600 dark:text-amber-400">{stats.value.needs_review} to review</span>
      </div>
    {/if}
    <button
      type="button"
      onclick={toggleTheme}
      aria-label="Toggle theme"
      class="rounded-lg p-2 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      {#if theme.mode === 'dark'}<Sun size={18} />{:else}<Moon size={18} />{/if}
    </button>
  </div>
</header>
```

- [ ] **Step 5: Rewrite `App.svelte`** — shell wiring (viewer is a placeholder until Task 4):
```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import Sidebar from './components/Sidebar.svelte';
  import TopBar from './components/TopBar.svelte';
  import { initTheme, loadPapers, loadStats } from './lib/state.svelte';

  onMount(() => {
    initTheme();
    loadStats();
    loadPapers();
  });
</script>

<div class="flex h-full flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    <Sidebar />
    <main class="grid min-h-0 flex-1 place-items-center text-slate-400">
      <p>Select a paper to open its PDF.</p>
    </main>
  </div>
</div>
```

- [ ] **Step 6: Type-check + build**

Run: `nix develop -c npm --prefix frontend run check` then `nix develop -c npm --prefix frontend run build`
Expected: no type errors; build succeeds.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/ frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(web): reader shell — top bar, sidebar, search/filter/sort"
```

---

## Task 4: Viewer — tabbed inline PDF + info panel + empty state

**Files:** create `frontend/src/components/{TabBar.svelte,PdfViewer.svelte,InfoPanel.svelte,EmptyState.svelte}`; update `frontend/src/App.svelte`.

- [ ] **Step 1: `EmptyState.svelte`** — welcome + a card grid to open PDFs:
```svelte
<script lang="ts">
  import { FileText } from 'lucide-svelte';
  import { library, openTab } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';
</script>

<div class="h-full overflow-y-auto p-8">
  <div class="mx-auto max-w-5xl">
    <div class="mb-6 flex items-center gap-2 text-slate-400">
      <FileText size={18} />
      <p class="text-sm">Open a paper to read it inline. You can keep several open as tabs.</p>
    </div>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {#each library.papers as paper (paper.id)}
        <button
          type="button"
          onclick={() => openTab(paper)}
          class="rounded-xl border border-slate-200 bg-white p-4 text-left shadow-sm transition hover:-translate-y-0.5 hover:shadow-md dark:border-slate-800 dark:bg-slate-900"
        >
          <div class="line-clamp-2 font-medium">{paper.title ?? '(untitled)'}</div>
          <div class="mt-1 line-clamp-1 text-xs text-slate-500 dark:text-slate-400">
            {paper.authors.slice(0, 3).join(', ')}{paper.authors.length > 3 ? ' et al.' : ''}
          </div>
          <div class="mt-2 flex items-center gap-2 text-xs text-slate-400">
            {#if paper.year}<span>{paper.year}</span>{/if}
            <StatusPill status={paper.status} />
          </div>
        </button>
      {/each}
    </div>
  </div>
</div>
```

- [ ] **Step 2: `TabBar.svelte`** — open-tab strip:
```svelte
<script lang="ts">
  import { Info, X } from 'lucide-svelte';
  import { closeTab, viewer } from '../lib/state.svelte';
</script>

<div class="flex h-11 shrink-0 items-center border-b border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each viewer.tabs as tab (tab.id)}
      <div
        class={`group flex h-11 max-w-52 shrink-0 items-center gap-2 border-r border-slate-200 px-3 dark:border-slate-800 ${
          viewer.activeId === tab.id
            ? 'bg-slate-50 dark:bg-slate-800/60'
            : 'hover:bg-slate-50 dark:hover:bg-slate-800/30'
        }`}
      >
        <button
          type="button"
          onclick={() => (viewer.activeId = tab.id)}
          class="min-w-0 truncate text-sm text-slate-700 dark:text-slate-200"
        >
          {tab.title}
        </button>
        <button
          type="button"
          aria-label="Close tab"
          onclick={() => closeTab(tab.id)}
          class="rounded p-0.5 text-slate-400 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
        >
          <X size={14} />
        </button>
      </div>
    {/each}
  </div>
  <button
    type="button"
    aria-label="Toggle info"
    onclick={() => (viewer.infoOpen = !viewer.infoOpen)}
    class={`mr-2 shrink-0 rounded-lg p-2 ${
      viewer.infoOpen
        ? 'bg-indigo-50 text-indigo-600 dark:bg-indigo-500/15 dark:text-indigo-400'
        : 'text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800'
    }`}
  >
    <Info size={18} />
  </button>
</div>
```

- [ ] **Step 3: `InfoPanel.svelte`** — slide-over metadata for the active paper:
```svelte
<script lang="ts">
  import { ExternalLink } from 'lucide-svelte';
  import { loadDetail } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { id }: { id: string } = $props();

  type Link = { label: string; href: string };
  function links(d: {
    doi: string | null;
    arxiv_id: string | null;
    dblp_key: string | null;
    url: string | null;
  }): Link[] {
    const out: Link[] = [];
    if (d.doi) out.push({ label: 'DOI', href: `https://doi.org/${d.doi}` });
    if (d.arxiv_id) out.push({ label: 'arXiv', href: `https://arxiv.org/abs/${d.arxiv_id}` });
    if (d.dblp_key) out.push({ label: 'DBLP', href: `https://dblp.org/rec/${d.dblp_key}.html` });
    if (d.url) out.push({ label: 'URL', href: d.url });
    return out;
  }
</script>

<aside class="flex h-full w-80 shrink-0 flex-col overflow-y-auto border-l border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
  {#await loadDetail(id)}
    <p class="text-sm text-slate-400">Loading…</p>
  {:then d}
    <h2 class="text-base font-semibold leading-snug">{d.title ?? '(untitled)'}</h2>
    <div class="mt-2"><StatusPill status={d.status} /></div>
    {#if d.authors.length}
      <p class="mt-3 text-sm text-slate-600 dark:text-slate-300">{d.authors.join(', ')}</p>
    {/if}
    <dl class="mt-3 space-y-1 text-xs text-slate-500 dark:text-slate-400">
      {#if d.venue}<div><dt class="inline font-medium">Venue:</dt> {d.venue}</div>{/if}
      {#if d.year}<div><dt class="inline font-medium">Year:</dt> {d.year}</div>{/if}
      {#if d.cite_key}<div><dt class="inline font-medium">Cite key:</dt> <code>{d.cite_key}</code></div>{/if}
      {#if d.source}<div><dt class="inline font-medium">Source:</dt> {d.source}</div>{/if}
    </dl>
    {#if links(d).length}
      <div class="mt-3 flex flex-wrap gap-2">
        {#each links(d) as l (l.label)}
          <a
            href={l.href}
            target="_blank"
            rel="noreferrer"
            class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-2 py-1 text-xs text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
          >
            {l.label}<ExternalLink size={12} />
          </a>
        {/each}
      </div>
    {/if}
    {#if d.abstract}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-400">Abstract</h3>
        <p class="text-sm leading-relaxed text-slate-600 dark:text-slate-300">{d.abstract}</p>
      </div>
    {/if}
  {:catch}
    <p class="text-sm text-red-500">Failed to load details.</p>
  {/await}
</aside>
```

- [ ] **Step 4: `PdfViewer.svelte`** — keep-alive iframe per open tab:
```svelte
<script lang="ts">
  import { pdfUrl } from '../lib/api';
  import { viewer } from '../lib/state.svelte';
</script>

<div class="relative min-h-0 flex-1 bg-slate-100 dark:bg-slate-950">
  {#each viewer.tabs as tab (tab.id)}
    <iframe
      title={tab.title}
      src={pdfUrl(tab.id)}
      class={`absolute inset-0 h-full w-full border-0 ${tab.id === viewer.activeId ? '' : 'hidden'}`}
    ></iframe>
  {/each}
</div>
```

- [ ] **Step 5: Final `App.svelte`** — assemble sidebar + viewer:
```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import EmptyState from './components/EmptyState.svelte';
  import InfoPanel from './components/InfoPanel.svelte';
  import PdfViewer from './components/PdfViewer.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import TabBar from './components/TabBar.svelte';
  import TopBar from './components/TopBar.svelte';
  import { initTheme, loadPapers, loadStats, viewer } from './lib/state.svelte';

  onMount(() => {
    initTheme();
    loadStats();
    loadPapers();
  });
</script>

<div class="flex h-full flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    <Sidebar />
    <main class="flex min-h-0 flex-1 flex-col">
      {#if viewer.tabs.length === 0}
        <EmptyState />
      {:else}
        <TabBar />
        <div class="flex min-h-0 flex-1">
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            <InfoPanel id={viewer.activeId} />
          {/if}
        </div>
      {/if}
    </main>
  </div>
</div>
```

- [ ] **Step 6: Type-check + build**

Run: `nix develop -c npm --prefix frontend run check` then `nix develop -c npm --prefix frontend run build`
Expected: no type errors; build succeeds.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/ frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(web): tabbed inline PDF viewer, info panel, empty state"
```

---

## Task 5: Component tests + final live verification

**Files:** create `frontend/src/test-setup.ts`, `frontend/src/components/StatusPill.test.ts`, `frontend/src/components/TabBar.test.ts`.

- [ ] **Step 1: Test setup (`frontend/src/test-setup.ts`)**:
```ts
import '@testing-library/jest-dom/vitest';
```

- [ ] **Step 2: `StatusPill.test.ts`** — renders the right label:
```ts
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusPill from './StatusPill.svelte';

describe('StatusPill', () => {
  it('shows "resolved" for resolved status', () => {
    render(StatusPill, { props: { status: 'resolved' } });
    expect(screen.getByText('resolved')).toBeInTheDocument();
  });

  it('shows "needs review" otherwise', () => {
    render(StatusPill, { props: { status: 'needs_review' } });
    expect(screen.getByText('needs review')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: `TabBar.test.ts`** — open tabs render and close works:
```ts
import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import TabBar from './TabBar.svelte';
import { closeTab, openTab, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

describe('TabBar', () => {
  beforeEach(() => {
    viewer.tabs = [];
    viewer.activeId = null;
  });

  it('renders one tab per open paper and closes them', async () => {
    openTab(paper('a', 'First Paper'));
    openTab(paper('b', 'Second Paper'));
    render(TabBar);
    expect(screen.getByText('First Paper')).toBeInTheDocument();
    expect(screen.getByText('Second Paper')).toBeInTheDocument();
    expect(viewer.tabs.length).toBe(2);
    expect(viewer.activeId).toBe('b'); // most-recently opened is active

    closeTab('b');
    expect(viewer.tabs.length).toBe(1);
    expect(viewer.activeId).toBe('a'); // falls back to a neighbor
  });
});
```

- [ ] **Step 4: Run the tests + type-check**

Run: `nix develop -c npm --prefix frontend test` then `nix develop -c npm --prefix frontend run check`
Expected: all vitest tests pass; no type errors.

- [ ] **Step 5: Final live verification (whole stack)**

Build the frontend, embed it, seed a paper + PDF, and drive the running server:
```bash
nix develop -c npm --prefix frontend run build
nix develop -c cargo build
```
The web UI is read-only, so this check confirms the embedded SPA is served and reaches the API against an empty library (no data-seeding needed):
```bash
SM=$(mktemp -d); mkdir -p "$SM/library"
printf 'inbox_dir="%s/inbox"\nlibrary_root="%s/library"\ndatabase_url="sqlite:%s/library.db"\n' "$SM" "$SM" "$SM" > "$SM/xuewen.toml"
nix develop -c bash -c "timeout 3 ./target/debug/xuewen --config '$SM/xuewen.toml' serve --port 8139 & sleep 1; echo '--- / (built app) ---'; curl -s http://127.0.0.1:8139/ | grep -o '<div id=\"app\">'; echo '--- assets referenced ---'; curl -s http://127.0.0.1:8139/ | grep -o '/assets/[^\"]*' | head -2; echo '--- api ---'; curl -s http://127.0.0.1:8139/api/papers; echo"
```
Expected: `/` returns the built SPA (`<div id="app">` present, an `/assets/…` bundle referenced), and `/api/papers` returns `[]` (empty library). This confirms the embedded Svelte app is served and can reach the API.

Optionally, for a full visual check with data, the user can point the config at the real `refresh_smoke`/live library from earlier and open `http://127.0.0.1:8080` in a browser after `xuewen serve` — note this in the report as a suggested manual check.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/test-setup.ts frontend/src/components/StatusPill.test.ts frontend/src/components/TabBar.test.ts
git -c commit.gpgsign=false commit -m "feat(web): frontend component tests (status pill, tab bar)"
```

---

## Verification (Definition of Done)

- `nix develop -c npm --prefix frontend run build` produces `frontend/dist/`; `nix develop -c cargo build` embeds it; `xuewen serve` returns the built SPA at `/` (not the placeholder) and the JSON API/PDF routes still work.
- `nix develop -c npm --prefix frontend test` and `... run check` pass (component smoke tests + type-check).
- `nix develop -c cargo test` still green (Rust unchanged); `cargo clippy`/`cargo fmt -- --check` clean.
- The reader works end-to-end: sidebar lists/searches/filters papers, clicking opens an inline PDF tab, multiple tabs coexist and switch instantly, the info toggle shows metadata/abstract/links, dark/light toggles, empty state shows the card grid.

## Notes for the executor

- **Svelte 5 idioms** (not Svelte 4): props via `$props()`, reactivity via `$state`/`$derived`/`$effect`, events as `onclick=` (not `on:click`), mount via `import { mount } from 'svelte'`. Shared state lives in `.svelte.ts` modules exporting `$state` objects whose *properties* are mutated (never reassign an exported binding — that's why state is modeled as objects like `library.papers`, `stats.value`).
- **Tailwind 4**: `@import 'tailwindcss'` + the `@tailwindcss/vite` plugin; dark mode via the `@custom-variant dark` line + a `.dark` class on `<html>` (toggled in `applyTheme`).
- **Dev loop** (not required for tasks, but for humans): `xuewen serve` on :8080 + `nix develop -c npm --prefix frontend run dev` (Vite :5173, proxying `/api`+`/papers`) for hot-reload.
- **Commit `frontend/package-lock.json`** (Task 1) for reproducible installs. Do NOT commit `frontend/dist/` or `frontend/node_modules/` (git-ignored).
- If a pinned npm version doesn't resolve, use the nearest published one and report it; keep Svelte 5 / Vite 6 / Tailwind 4 majors.
- No Rust source changes are expected in this plan; if you find you need one, stop and report (it likely means a Plan A gap).
- Every commit uses `git -c commit.gpgsign=false`.
