# Web UI Refactor — Paper & Ink Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the Svelte frontend into a two-pane Paper & Ink workspace — persistent library list + content pane (detail view or PDF reader), zen mode, command palette, keyboard shortcuts, toasts — with a coherent spring/FLIP/crossfade motion language on Svelte built-ins only.

**Architecture:** Frontend-only (`frontend/`); zero backend changes. The content pane is a tab strip whose first, permanent tab is the **Library home** (`viewer.activeId === null`) showing a full-width `DetailView` of the selected paper; PDF tabs come after it. The old `Sidebar` decomposes into `LibraryPane` (SearchBox / FilterRow / PaperList / PaperRow); `InfoPanel` and `DetailView` share `PaperMeta` / `ProjectTags` / `CiteActions` / `PaperActions` subcomponents. A shared `Modal` wrapper animates all three dialogs. New lib modules: `motion.ts` (tokens), `toasts.svelte.ts`, `shortcuts.ts`, `fuzzy.ts`.

**Tech Stack:** Svelte 5.56 (runes, `Spring` from `svelte/motion`, `crossfade`/`fly`/`fade`/`scale` from `svelte/transition`, `flip` from `svelte/animate`), Tailwind CSS 4 `@theme` tokens, lucide-svelte, `@fontsource-variable/inter` + `@fontsource-variable/source-serif-4`; vitest + @testing-library/svelte + jsdom.

**Spec:** `docs/superpowers/specs/2026-07-10-webui-refactor-design.md`

**Environment:** direnv loads the flake dev shell in this repo (`$IN_NIX_SHELL` is set), so `node`/`npm` work directly. If a command fails with a missing tool, re-run it as `nix develop -c <command>`. Commit with `git -c commit.gpgsign=false commit -m "..."` (signing is unavailable in this environment). Conventional Commits with scope; allowed types feat/fix/docs/chore/ci — use `feat(frontend)` for this plan's commits. Run all npm commands from the repo root with `--prefix frontend`.

## Global Constraints

- **No backend or API changes of any kind** (spec Non-goals). Nothing under `src/` (Rust) changes.
- **No new runtime dependencies except `@fontsource-variable/source-serif-4`** (spec Non-goals). No animation libraries.
- **No Daily arXiv UI** in this frontend.
- **Every animation duration flows through `dur()` from `lib/motion.ts`** so `prefers-reduced-motion` (and jsdom tests) get 0ms. No hardcoded durations in components.
- **Cinnabar (`--color-seal`) appears in exactly two places**: the TopBar wordmark seal and the Welcome panel seal. Never on actions, never as an accent.
- **Amber (`amber-700` light / `amber-500` dark) is the only action accent.** Status pills use lime (resolved) and yellow (needs review) tints — never amber.
- **Copy rules:** sentence case, active voice, verbs on buttons say what happens ("Open PDF", not "View"). Errors state what went wrong and what to do next.
- Existing functionality preserved: search fields/engines, filters, projects, import (files/URLs/proxy cookie), identify, cite/export, delete, theme cycle.
- After every task: `npm --prefix frontend run test` and `npm --prefix frontend run check` pass.

---

## Design Foundation (frontend-design pass)

**Subject:** a personal academic-paper library ("Xuewen", 學問 — *scholarship*). Audience: one researcher. The UI's job: find a paper, decide whether to read it, read it without distraction.

**Palette (named tokens):**

| Token | Light | Dark | Role |
|---|---|---|---|
| `paper` | `#faf9f7` | — | main surface (light) |
| `parchment` | `#f1efea` | — | recessed surface: pane bg, inputs, hover |
| `ink` | `#1c1917` | — | primary text (light) |
| `night` | — | `#161311` | main surface (dark) |
| `soot` | — | `#211d1a` | raised surface (dark) |
| `seal` | `#9e2b25` | same | signature mark ONLY |
| accent | `amber-700 #b45309` | `amber-500 #f59e0b` | interactive elements |
| grays | Tailwind `stone` scale | `stone` | borders `stone-200/800`, secondary text `stone-500/400` |
| status | `lime-100/800` resolved, `yellow-100/800` review | `lime-500/15 + lime-300`, `yellow-500/15 + yellow-300` | pills only |

**Type roles:** display = Source Serif 4 Variable (paper titles, DetailView hero, wordmark, abstract body in DetailView); UI = Inter Variable; data = `font-mono` system stack (cite keys, DOI/arXiv chips — bibliographic data is a BibTeX artifact, set it like one).

**Signature:** the cinnabar seal — a rounded-square stamp bearing 學, the way a scholar stamps a work into their collection. Two placements (wordmark, welcome panel), nowhere else. The second signature axis is motion: one spring, one ease, everything moves with the same accent.

**Self-critique vs. the generic default:** warm-cream + serif + terracotta is a known AI-default look. The brief (user-approved spec) pins the warm-paper/serif/amber direction, so distinctiveness is spent on execution: (1) the seal mark — a culturally-specific identity no template produces; (2) bibliographic data in mono as first-class artifacts; (3) discipline rules above (seal twice, amber only interactive) instead of accent-everywhere; (4) restrained motion with one shared vocabulary instead of scattered effects.

---

## File Structure

```
frontend/src/
  app.css                     rewrite: @theme tokens, fonts, view-transition guard
  App.svelte                  rewrite: two-pane shell, spring pane, peek, zen, overlays
  lib/
    motion.ts                 NEW: DUR/EASE/SPRINGS tokens, dur(), prefersReducedMotion()
    motion.test.ts            NEW
    toasts.svelte.ts          NEW: toast store
    toasts.test.ts            NEW
    shortcuts.ts              NEW: global keydown handler
    shortcuts.test.ts         NEW
    fuzzy.ts                  NEW: fuzzyScore
    fuzzy.test.ts             NEW
    state.svelte.ts           modify: selection, home-tab semantics, zen, palette flag,
                              view-transition theme, toast wiring in removePaper
    ui.test.ts                NEW: selection/zen/home-tab state tests
  components/
    SealMark.svelte           NEW: the signature stamp
    TopBar.svelte             rewrite
    LibraryPane.svelte        NEW (replaces Sidebar.svelte — deleted)
    SearchBox.svelte          NEW
    FilterRow.svelte          NEW
    PaperList.svelte          NEW
    PaperRow.svelte           rewrite: select vs open
    PaperRow.test.ts          NEW
    DetailView.svelte         NEW: Library-home content
    Welcome.svelte            NEW: nothing-selected state (replaces EmptyState — deleted)
    DetailView.test.ts        NEW
    PaperMeta.svelte          NEW (shared)
    ProjectTags.svelte        NEW (shared, logic from InfoPanel)
    CiteActions.svelte        NEW (shared, logic from InfoPanel)
    CiteActions.test.ts       NEW
    PaperActions.svelte       NEW (shared, identify/delete from InfoPanel)
    InfoPanel.svelte          rewrite: thin composition of shared components
    TabBar.svelte             rewrite: Library home tab, crossfade underline, zen/info buttons
    TabBar.test.ts            modify: extend for home tab
    PdfViewer.svelte          keep as-is
    Modal.svelte              NEW: shared dialog wrapper
    Modal.test.ts             NEW
    ImportModal.svelte        rework on Modal (logic unchanged)
    IdentifyModal.svelte      rework on Modal (logic unchanged)
    ProjectsModal.svelte      rework on Modal (logic unchanged; test labels preserved)
    CommandPalette.svelte     NEW
    CommandPalette.test.ts    NEW
    Toaster.svelte            NEW
    ZenPill.svelte            NEW
```

Store-level tests (`ImportModal.test.ts`, `IdentifyModal.test.ts`, `InfoPanel.test.ts`, `search*.test.ts`, `projects.test.ts`, `theme.test.ts`, `export.test.ts`) exercise `state.svelte.ts`/`api.ts` functions, not markup — they must keep passing untouched except where a task says otherwise.

---

## Task 1: Design tokens, fonts, and the motion module

**Files:**
- Modify: `frontend/package.json` (via npm install)
- Modify: `frontend/src/app.css`
- Create: `frontend/src/lib/motion.ts`
- Test: `frontend/src/lib/motion.test.ts`

**Interfaces:**
- Produces: `DUR = { fast: 150, base: 250, slow: 400 }`, `EASE: string`, `SPRINGS.pane: { stiffness: number; damping: number }`, `dur(ms: number): number`, `prefersReducedMotion(): boolean`. Tailwind tokens: `bg-paper`, `bg-parchment`, `text-ink`, `bg-night`, `bg-soot`, `bg-seal`, `font-serif`, `ease-fluent`.

- [ ] **Step 1: Add the serif font package**

```bash
npm --prefix frontend install @fontsource-variable/source-serif-4
```

Expected: `package.json` gains `"@fontsource-variable/source-serif-4"` under dependencies; lockfile updated.

- [ ] **Step 2: Write the failing motion test**

`frontend/src/lib/motion.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DUR, EASE, SPRINGS, dur, prefersReducedMotion } from './motion';

afterEach(() => vi.unstubAllGlobals());

function stubReducedMotion(matches: boolean): void {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('prefers-reduced-motion') ? matches : false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe('motion tokens', () => {
  it('exposes the shared vocabulary', () => {
    expect(DUR).toEqual({ fast: 150, base: 250, slow: 400 });
    expect(EASE).toBe('cubic-bezier(0.22, 1, 0.36, 1)');
    expect(SPRINGS.pane.stiffness).toBeGreaterThan(0);
  });

  it('prefersReducedMotion reads the media query', () => {
    stubReducedMotion(true);
    expect(prefersReducedMotion()).toBe(true);
    stubReducedMotion(false);
    expect(prefersReducedMotion()).toBe(false);
  });

  it('prefersReducedMotion is false when matchMedia is unavailable (jsdom default)', () => {
    expect(prefersReducedMotion()).toBe(false);
  });

  it('dur is 0 under vitest so transitions never linger in DOM tests', () => {
    expect(dur(250)).toBe(0);
  });
});
```

- [ ] **Step 3: Run it to make sure it fails**

```bash
npm --prefix frontend run test -- src/lib/motion.test.ts
```

Expected: FAIL — cannot resolve `./motion`.

- [ ] **Step 4: Implement `frontend/src/lib/motion.ts`**

```ts
/// Shared motion vocabulary. Every animated surface derives its timing from
/// these tokens so the whole UI moves with one accent — never hardcode a
/// duration in a component.
export const DUR = { fast: 150, base: 250, slow: 400 } as const;

/// The one standard ease-out (quart). Mirrors --ease-fluent in app.css.
export const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

/// Presets for `new Spring(value, SPRINGS.x)` from svelte/motion.
export const SPRINGS = {
  pane: { stiffness: 0.18, damping: 0.85 },
} as const;

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

/// Resolve a duration. 0 under reduced motion (accessibility) and under
/// vitest (jsdom runs transitions on rAF; non-zero durations leave outro
/// elements lingering and make DOM assertions flaky).
export function dur(ms: number): number {
  if (import.meta.env.MODE === 'test' || prefersReducedMotion()) return 0;
  return ms;
}
```

- [ ] **Step 5: Run the test — expect PASS**

```bash
npm --prefix frontend run test -- src/lib/motion.test.ts
```

- [ ] **Step 6: Rewrite `frontend/src/app.css`**

Full new content (keeps the load-bearing `color-scheme` comment):

```css
@import 'tailwindcss';
@import '@fontsource-variable/inter';
@import '@fontsource-variable/source-serif-4';

/* Class-based dark mode (toggled on <html> by the theme state). */
@custom-variant dark (&:where(.dark, .dark *));

@theme {
  --font-sans: 'Inter Variable', system-ui, -apple-system, sans-serif;
  --font-serif: 'Source Serif 4 Variable', 'Iowan Old Style', Georgia, serif;

  /* Paper & Ink surfaces (light) */
  --color-paper: #faf9f7;
  --color-parchment: #f1efea;
  --color-ink: #1c1917;

  /* Warm dark counterparts */
  --color-night: #161311;
  --color-soot: #211d1a;

  /* The seal — signature mark only (TopBar wordmark, Welcome). Never an
     action color; actions use amber-700/amber-500. */
  --color-seal: #9e2b25;

  --ease-fluent: cubic-bezier(0.22, 1, 0.36, 1);
}

/* Keep the browser's UA color scheme in sync with the class-based theme
   toggle (.dark on <html>). Using `light dark` here would make UA defaults
   — including the `canvastext` fallback color inherited by elements without
   an explicit text color, such as modal titles rendered outside the themed
   app wrapper — follow the OS `prefers-color-scheme` instead of the selected
   theme. On a dark-OS with the app in light mode that renders near-white text
   on the white modal card (invisible). Tracking the class keeps them legible. */
:root {
  color-scheme: light;
}

:root.dark {
  color-scheme: dark;
}

html,
body,
#app {
  height: 100%;
}

body {
  font-family: var(--font-sans);
}

/* Theme toggles use the View Transitions API for a soft crossfade; kill it
   for users who asked for less motion. */
@media (prefers-reduced-motion: reduce) {
  ::view-transition-group(*),
  ::view-transition-old(*),
  ::view-transition-new(*) {
    animation: none !important;
  }
}
```

- [ ] **Step 7: Full verification**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
npm --prefix frontend run build
```

Expected: all pass (old components still use stone/slate/indigo classes — slate/indigo remain valid built-ins until each is ported).

- [ ] **Step 8: Commit**

```bash
git add frontend/package.json frontend/package-lock.json frontend/src/app.css frontend/src/lib/motion.ts frontend/src/lib/motion.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): Paper & Ink design tokens and shared motion vocabulary"
```

---

## Task 2: State foundations — selection, Library-home tab semantics, zen

**Files:**
- Modify: `frontend/src/lib/state.svelte.ts`
- Test: `frontend/src/lib/ui.test.ts` (new)

**Interfaces:**
- Produces: `selection: { id: string | null }`, `selectPaper(id: string | null): void`, `goHome(): void`, `toggleZen(): void`; `ui` gains `zen: boolean` and `paletteOpen: boolean`. New semantics: `viewer.activeId === null` means the **Library home tab** is active (tabs may still exist); `openTab` also selects; `closeTab` of the last tab lands on home and exits zen; `goHome()` exits zen; `removePaper` clears a matching selection.

- [ ] **Step 1: Write the failing tests**

`frontend/src/lib/ui.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  closeTab,
  goHome,
  library,
  openTab,
  removePaper,
  selection,
  selectPaper,
  toggleZen,
  ui,
  viewer,
} from './state.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

beforeEach(() => {
  library.papers = [];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  ui.zen = false;
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

describe('selection and home tab', () => {
  it('selectPaper sets and clears the browsing selection', () => {
    selectPaper('a');
    expect(selection.id).toBe('a');
    selectPaper(null);
    expect(selection.id).toBe(null);
  });

  it('openTab activates the tab and selects the paper', () => {
    openTab(paper('a'));
    expect(viewer.activeId).toBe('a');
    expect(selection.id).toBe('a');
  });

  it('goHome keeps tabs open but activates the Library home', () => {
    openTab(paper('a'));
    goHome();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1);
  });

  it('closing the last tab lands on the Library home', () => {
    openTab(paper('a'));
    closeTab('a');
    expect(viewer.tabs.length).toBe(0);
    expect(viewer.activeId).toBe(null);
  });
});

describe('zen mode', () => {
  it('toggleZen only engages while a PDF tab is active', () => {
    toggleZen();
    expect(ui.zen).toBe(false); // home active — nothing to zen into
    openTab(paper('a'));
    toggleZen();
    expect(ui.zen).toBe(true);
    toggleZen();
    expect(ui.zen).toBe(false);
  });

  it('closing the last tab exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    closeTab('a');
    expect(ui.zen).toBe(false);
  });

  it('goHome exits zen', () => {
    openTab(paper('a'));
    toggleZen();
    goHome();
    expect(ui.zen).toBe(false);
  });
});

describe('removePaper selection', () => {
  it('clears the selection when the selected paper is deleted', async () => {
    library.papers = [paper('x')];
    selectPaper('x');
    await removePaper('x');
    expect(selection.id).toBe(null);
  });
});
```

- [ ] **Step 2: Run to verify failure**

```bash
npm --prefix frontend run test -- src/lib/ui.test.ts
```

Expected: FAIL — `selection`, `selectPaper`, `goHome`, `toggleZen` not exported.

- [ ] **Step 3: Implement in `frontend/src/lib/state.svelte.ts`**

Add after the `viewer` declaration (and update the `viewer` doc comment):

```ts
export interface Tab {
  id: string;
  title: string;
}
/// The content pane's tab strip. `activeId === null` means the permanent
/// "Library" home tab is active (DetailView of `selection`); a string means
/// that PDF tab is active. Tabs persist while home is active.
export const viewer = $state<{ tabs: Tab[]; activeId: string | null; infoOpen: boolean }>({
  tabs: [],
  activeId: null,
  infoOpen: false,
});

/// The browsing selection shown by the Library home's DetailView. Distinct
/// from viewer.activeId: selecting inspects, opening reads.
export const selection = $state<{ id: string | null }>({ id: null });

export function selectPaper(id: string | null): void {
  selection.id = id;
}

/// Activate the Library home tab (keeps PDF tabs open). Leaving the reader
/// always leaves zen too — zen without a PDF is a blank screen.
export function goHome(): void {
  viewer.activeId = null;
  ui.zen = false;
}

/// Zen requires an active PDF tab; toggling from home is a no-op.
export function toggleZen(): void {
  ui.zen = viewer.activeId !== null && !ui.zen;
}
```

Extend the `ui` store:

```ts
export const ui = $state<{
  sidebarOpen: boolean;
  importOpen: boolean;
  projectsOpen: boolean;
  zen: boolean;
  paletteOpen: boolean;
}>({
  sidebarOpen: true,
  importOpen: false,
  projectsOpen: false,
  zen: false,
  paletteOpen: false,
});
```

Update `openTab` and `closeTab`:

```ts
export function openTab(p: PaperSummary): void {
  if (!viewer.tabs.some((t) => t.id === p.id)) {
    viewer.tabs.push({ id: p.id, title: p.title ?? p.cite_key ?? p.id });
  }
  viewer.activeId = p.id;
  selection.id = p.id;
}

export function closeTab(id: string): void {
  const idx = viewer.tabs.findIndex((t) => t.id === id);
  if (idx === -1) return;
  viewer.tabs.splice(idx, 1);
  if (viewer.activeId === id) {
    viewer.activeId = viewer.tabs[Math.max(0, idx - 1)]?.id ?? null;
  }
  if (viewer.tabs.length === 0) ui.zen = false;
}
```

In `removePaper`, after `detailCache.delete(id);` add:

```ts
  if (selection.id === id) selection.id = null;
```

- [ ] **Step 4: Run tests — expect PASS, including the untouched suites**

```bash
npm --prefix frontend run test
```

Expected: all pass (`TabBar.test.ts`'s `closeTab` fallback assertion still holds; `InfoPanel.test.ts` unaffected).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/state.svelte.ts frontend/src/lib/ui.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): selection state, Library-home tab semantics, zen mode state"
```

---

## Task 3: Toast store and Toaster component

**Files:**
- Create: `frontend/src/lib/toasts.svelte.ts`
- Create: `frontend/src/components/Toaster.svelte`
- Modify: `frontend/src/App.svelte` (mount only)
- Test: `frontend/src/lib/toasts.test.ts`

**Interfaces:**
- Produces: `toast(kind: 'success' | 'error' | 'info', message: string, timeoutMs?: number): number`, `dismissToast(id: number): void`, `toasts: { items: Toast[] }` with `Toast = { id: number; kind: ...; message: string }`. Default timeout 3500ms; `timeoutMs: 0` means sticky.

- [ ] **Step 1: Write the failing store test**

`frontend/src/lib/toasts.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { dismissToast, toast, toasts } from './toasts.svelte';

beforeEach(() => {
  vi.useFakeTimers();
  toasts.items.length = 0;
});
afterEach(() => vi.useRealTimers());

describe('toast store', () => {
  it('pushes and auto-dismisses after the timeout', () => {
    toast('success', 'Citation copied');
    expect(toasts.items).toHaveLength(1);
    expect(toasts.items[0]).toMatchObject({ kind: 'success', message: 'Citation copied' });
    vi.advanceTimersByTime(3500);
    expect(toasts.items).toHaveLength(0);
  });

  it('timeoutMs 0 sticks until dismissed by hand', () => {
    const id = toast('error', 'Import failed', 0);
    vi.advanceTimersByTime(60_000);
    expect(toasts.items).toHaveLength(1);
    dismissToast(id);
    expect(toasts.items).toHaveLength(0);
  });

  it('dismissing an unknown id is a no-op', () => {
    toast('info', 'hello');
    dismissToast(999);
    expect(toasts.items).toHaveLength(1);
  });
});
```

- [ ] **Step 2: Run — expect FAIL (module missing)**

```bash
npm --prefix frontend run test -- src/lib/toasts.test.ts
```

- [ ] **Step 3: Implement `frontend/src/lib/toasts.svelte.ts`**

```ts
export interface Toast {
  id: number;
  kind: 'success' | 'error' | 'info';
  message: string;
}

export const toasts = $state<{ items: Toast[] }>({ items: [] });

let nextId = 1;

/// Show a transient toast. Returns the id (for programmatic dismissal).
/// timeoutMs 0 = sticky. Toasts are additive feedback — persistent errors
/// must also stay inline where they occur.
export function toast(kind: Toast['kind'], message: string, timeoutMs = 3500): number {
  const id = nextId++;
  toasts.items.push({ id, kind, message });
  if (timeoutMs > 0) setTimeout(() => dismissToast(id), timeoutMs);
  return id;
}

export function dismissToast(id: number): void {
  const idx = toasts.items.findIndex((t) => t.id === id);
  if (idx !== -1) toasts.items.splice(idx, 1);
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/lib/toasts.test.ts
```

- [ ] **Step 5: Create `frontend/src/components/Toaster.svelte`**

```svelte
<script lang="ts">
  import { CircleAlert, CircleCheck, Info, X } from 'lucide-svelte';
  import { fade, fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { dismissToast, toasts } from '../lib/toasts.svelte';
</script>

<div
  class="pointer-events-none fixed bottom-4 right-4 z-[70] flex w-80 flex-col gap-2"
  role="status"
  aria-live="polite"
>
  {#each toasts.items as t (t.id)}
    <div
      in:fly={{ y: 16, duration: dur(DUR.base) }}
      out:fade={{ duration: dur(DUR.fast) }}
      class="pointer-events-auto flex items-center gap-2 rounded-lg border border-stone-200 bg-paper px-3 py-2 text-sm text-ink shadow-lg dark:border-stone-800 dark:bg-soot dark:text-stone-100"
    >
      {#if t.kind === 'success'}
        <CircleCheck size={16} class="shrink-0 text-lime-700 dark:text-lime-400" />
      {:else if t.kind === 'error'}
        <CircleAlert size={16} class="shrink-0 text-red-600 dark:text-red-400" />
      {:else}
        <Info size={16} class="shrink-0 text-stone-500 dark:text-stone-400" />
      {/if}
      <span class="min-w-0 flex-1">{t.message}</span>
      <button
        type="button"
        aria-label="Dismiss"
        onclick={() => dismissToast(t.id)}
        class="rounded p-0.5 text-stone-400 hover:bg-parchment dark:hover:bg-stone-800"
      >
        <X size={14} />
      </button>
    </div>
  {/each}
</div>
```

- [ ] **Step 6: Mount in `frontend/src/App.svelte`**

Add `import Toaster from './components/Toaster.svelte';` and append `<Toaster />` as the last line of the template (after the modals).

- [ ] **Step 7: Verify, commit**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
git add frontend/src/lib/toasts.svelte.ts frontend/src/lib/toasts.test.ts frontend/src/components/Toaster.svelte frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): toast store and Toaster"
```

---

## Task 4: Shared paper components (PaperMeta, ProjectTags, CiteActions, PaperActions) + SealMark

**Files:**
- Create: `frontend/src/components/SealMark.svelte`
- Create: `frontend/src/components/PaperMeta.svelte`
- Create: `frontend/src/components/ProjectTags.svelte`
- Create: `frontend/src/components/CiteActions.svelte`
- Create: `frontend/src/components/PaperActions.svelte`
- Test: `frontend/src/components/CiteActions.test.ts`

**Interfaces:**
- Consumes: `PaperDetail` from `lib/types`, state fns `addToProject`, `removeFromProject`, `openProjects`, `projects`, `bibFormat`, `copyCitation`, `openIdentify`, `removePaper`; `toast` from Task 3.
- Produces component props: `SealMark { size?: number }`; `PaperMeta { d: PaperDetail; hero?: boolean }`; `ProjectTags { d: PaperDetail }`; `CiteActions { id: string; citeKey: string | null }`; `PaperActions { d: PaperDetail }`. (These are used verbatim by Tasks 6's DetailView and InfoPanel.)

- [ ] **Step 1: Create `frontend/src/components/SealMark.svelte`**

```svelte
<script lang="ts">
  // The signature: a cinnabar seal stamping 學 (xué, "learning"). Used in
  // exactly two places — TopBar wordmark and Welcome. Decorative only.
  let { size = 24 }: { size?: number } = $props();
</script>

<span
  aria-hidden="true"
  style={`width:${size}px;height:${size}px;font-size:${Math.round(size * 0.62)}px`}
  class="inline-flex select-none items-center justify-center rounded-[22%] bg-seal font-serif font-semibold leading-none text-paper"
>學</span>
```

- [ ] **Step 2: Create `frontend/src/components/PaperMeta.svelte`**

```svelte
<script lang="ts">
  import { ExternalLink } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import StatusPill from './StatusPill.svelte';

  let { d, hero = false }: { d: PaperDetail; hero?: boolean } = $props();

  type Link = { label: string; href: string };
  const links = $derived.by(() => {
    const out: Link[] = [];
    if (d.doi) out.push({ label: 'DOI', href: `https://doi.org/${d.doi}` });
    if (d.arxiv_id) out.push({ label: 'arXiv', href: `https://arxiv.org/abs/${d.arxiv_id}` });
    if (d.dblp_key) out.push({ label: 'DBLP', href: `https://dblp.org/rec/${d.dblp_key}.html` });
    if (d.url) out.push({ label: 'URL', href: d.url });
    return out;
  });
</script>

{#if d.venue || d.year}
  <p
    class={`font-medium uppercase tracking-widest text-stone-500 dark:text-stone-400 ${hero ? 'text-xs' : 'text-[10px]'}`}
  >
    {d.venue ?? ''}{d.venue && d.year ? ' · ' : ''}{d.year ?? ''}
  </p>
{/if}
<h2
  class={`font-serif font-semibold text-ink dark:text-stone-100 ${
    hero ? 'mt-2 text-3xl leading-tight text-balance' : 'mt-1 text-base leading-snug'
  }`}
>
  {d.title ?? '(untitled)'}
</h2>
{#if d.authors.length}
  <p class="mt-3 text-sm text-stone-600 dark:text-stone-300">{d.authors.join(', ')}</p>
{/if}
<div class="mt-3 flex flex-wrap items-center gap-1.5">
  <StatusPill status={d.status} />
  {#each links as l (l.label)}
    <a
      href={l.href}
      target="_blank"
      rel="noreferrer"
      class="inline-flex items-center gap-1 rounded-full border border-stone-200 px-2 py-0.5 font-mono text-[11px] text-stone-600 hover:border-amber-700 hover:text-amber-700 dark:border-stone-700 dark:text-stone-300 dark:hover:border-amber-500 dark:hover:text-amber-400"
    >
      {l.label}<ExternalLink size={10} />
    </a>
  {/each}
</div>
{#if d.cite_key || d.source}
  <dl class="mt-3 space-y-0.5 text-xs text-stone-500 dark:text-stone-400">
    {#if d.cite_key}
      <div><dt class="inline font-medium">Cite key</dt> <dd class="inline font-mono">{d.cite_key}</dd></div>
    {/if}
    {#if d.source}
      <div><dt class="inline font-medium">Source</dt> <dd class="inline">{d.source}</dd></div>
    {/if}
  </dl>
{/if}
```

- [ ] **Step 3: Create `frontend/src/components/ProjectTags.svelte`** (logic lifted from today's InfoPanel lines 49–80 and 128–161)

```svelte
<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import { addToProject, openProjects, projects, removeFromProject } from '../lib/state.svelte';

  let { d }: { d: PaperDetail } = $props();

  let membershipError = $state<string | null>(null);

  async function onAddProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    // The sentinel option opens the Projects modal instead of adding.
    if (projectId === '__new__') {
      openProjects();
      return;
    }
    membershipError = null;
    try {
      await addToProject(d.id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  async function onRemoveProject(projectId: string) {
    membershipError = null;
    try {
      await removeFromProject(d.id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  function projectName(pid: string): string {
    return projects.items.find((p) => p.id === pid)?.name ?? pid;
  }
</script>

<h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Projects</h3>
{#if d.project_ids.length}
  <div class="flex flex-wrap gap-1.5">
    {#each d.project_ids as pid (pid)}
      <span
        class="inline-flex items-center gap-1 rounded-full bg-parchment px-2 py-0.5 text-xs text-stone-700 dark:bg-stone-800 dark:text-stone-300"
      >
        {projectName(pid)}
        <button
          type="button"
          aria-label={`Remove from ${projectName(pid)}`}
          onclick={() => void onRemoveProject(pid)}
          class="rounded-full hover:bg-stone-200 dark:hover:bg-stone-700"
        >
          <X size={12} />
        </button>
      </span>
    {/each}
  </div>
{/if}
<select
  aria-label="Add to project"
  onchange={onAddProject}
  class="mt-2 w-full rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
>
  <option value="">Add to project…</option>
  {#each projects.items.filter((p) => !d.project_ids.includes(p.id)) as p (p.id)}
    <option value={p.id}>{p.name}</option>
  {/each}
  <option value="__new__">New project…</option>
</select>
{#if membershipError}
  <p class="mt-1 text-xs text-red-600 dark:text-red-400">{membershipError}</p>
{/if}
```

- [ ] **Step 4: Write the failing CiteActions test**

`frontend/src/components/CiteActions.test.ts`:

```ts
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import CiteActions from './CiteActions.svelte';
import { toasts } from '../lib/toasts.svelte';

beforeEach(() => {
  toasts.items.length = 0;
  vi.unstubAllGlobals();
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response('@article{key2024}', { status: 200 })),
  );
});

describe('CiteActions', () => {
  it('copies the citation and confirms with a toast', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    render(CiteActions, { props: { id: 'p1', citeKey: 'key2024' } });
    await userEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(writeText).toHaveBeenCalledWith('@article{key2024}');
    expect(toasts.items.some((t) => t.kind === 'success')).toBe(true);
  });

  it('keeps the failure hint inline when copy is impossible', async () => {
    vi.stubGlobal('navigator', {}); // no clipboard API
    vi.stubGlobal('document', Object.assign(document, {})); // keep jsdom document
    // jsdom's execCommand is undefined -> the legacy path throws.
    render(CiteActions, { props: { id: 'p1', citeKey: 'key2024' } });
    await userEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(screen.getByText(/use Download instead/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Run — expect FAIL (component missing)**

```bash
npm --prefix frontend run test -- src/components/CiteActions.test.ts
```

- [ ] **Step 6: Create `frontend/src/components/CiteActions.svelte`** (logic from InfoPanel lines 19–32 and 168–197; success feedback becomes a toast, failure stays inline)

```svelte
<script lang="ts">
  import { Copy, Download } from 'lucide-svelte';
  import { bibFormat, copyCitation } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  let { id, citeKey }: { id: string; citeKey: string | null } = $props();

  let copyError = $state(false);
  async function doCopy() {
    copyError = false;
    try {
      await copyCitation(id);
      toast('success', 'Citation copied');
    } catch {
      // Both clipboard paths failed — surface it inline so the user knows
      // to use Download (a toast alone would vanish).
      copyError = true;
    }
  }
</script>

<div class="flex items-center gap-2">
  <select
    bind:value={bibFormat.value}
    aria-label="Citation format"
    class="rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
  >
    <option value="bibtex">BibTeX</option>
    <option value="biblatex">BibLaTeX</option>
  </select>
  <button
    type="button"
    onclick={doCopy}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Copy size={12} /> Copy
  </button>
  <a
    href={`/api/papers/${encodeURIComponent(id)}/export?format=${bibFormat.value}`}
    download={`${citeKey ?? id}.bib`}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Download size={12} /> Download
  </a>
</div>
{#if copyError}
  <p class="mt-1 text-xs text-yellow-700 dark:text-yellow-400">Couldn't copy — use Download instead.</p>
{/if}
```

- [ ] **Step 7: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/components/CiteActions.test.ts
```

- [ ] **Step 8: Create `frontend/src/components/PaperActions.svelte`** (identify/delete from InfoPanel lines 34–47 and 198–240; delete success gains a toast)

```svelte
<script lang="ts">
  import { Trash2, Wand2 } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import { openIdentify, removePaper } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  let { d }: { d: PaperDetail } = $props();

  let confirming = $state(false);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  async function doDelete() {
    deleting = true;
    deleteError = null;
    try {
      await removePaper(d.id);
      toast('success', 'Paper deleted');
      // On success the surrounding view unmounts (tab closes / selection clears).
    } catch (e) {
      deleteError = (e as Error).message;
      deleting = false;
    }
  }
</script>

<div class="flex flex-wrap items-center gap-3">
  <button
    type="button"
    onclick={() => openIdentify(d.id, { doi: d.doi, arxiv_id: d.arxiv_id })}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Wand2 size={14} /> Identify…
  </button>
  {#if confirming}
    {#if deleting}
      <span class="text-sm text-stone-500 dark:text-stone-400">Deleting…</span>
    {:else}
      <span class="text-sm text-stone-600 dark:text-stone-300">Delete this paper?</span>
      <button
        type="button"
        onclick={doDelete}
        class="rounded-lg bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
      >
        Delete
      </button>
      <button
        type="button"
        onclick={() => (confirming = false)}
        class="rounded-lg px-3 py-1 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        Cancel
      </button>
    {/if}
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
{#if deleteError}
  <p class="mt-2 text-xs text-red-600 dark:text-red-400">Delete failed: {deleteError}</p>
{/if}
```

- [ ] **Step 9: Verify all suites and svelte-check, commit**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
git add frontend/src/components/{SealMark,PaperMeta,ProjectTags,CiteActions,PaperActions}.svelte frontend/src/components/CiteActions.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): shared paper components and the seal mark"
```

---

## Task 5: LibraryPane — decompose the sidebar (SearchBox, FilterRow, PaperList, PaperRow)

**Files:**
- Create: `frontend/src/components/SearchBox.svelte`
- Create: `frontend/src/components/FilterRow.svelte`
- Create: `frontend/src/components/PaperList.svelte`
- Create: `frontend/src/components/LibraryPane.svelte`
- Rewrite: `frontend/src/components/PaperRow.svelte`
- Delete: `frontend/src/components/Sidebar.svelte`
- Modify: `frontend/src/App.svelte` (swap Sidebar → LibraryPane)
- Test: `frontend/src/components/PaperRow.test.ts` (new)

**Interfaces:**
- Consumes: Task 2's `selection`/`selectPaper`/`goHome`, existing search/filter state fns.
- Produces: `LibraryPane` (no props) — the whole left pane; `PaperRow { paper: PaperSummary }` with click=select, dblclick=open semantics. The search input carries `data-search-input` (Task 11's `/` shortcut focuses it).

- [ ] **Step 1: Write the failing PaperRow test**

`frontend/src/components/PaperRow.test.ts`:

```ts
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PaperRow from './PaperRow.svelte';
import { selection, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const paper: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
  source: null, status: 'resolved', added_at: '',
};

beforeEach(() => {
  selection.id = null;
  viewer.tabs = [];
  viewer.activeId = null;
});

describe('PaperRow', () => {
  it('single click selects without opening a tab', async () => {
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(selection.id).toBe('p1');
    expect(viewer.tabs).toHaveLength(0);
  });

  it('double click opens the PDF tab', async () => {
    render(PaperRow, { props: { paper } });
    await userEvent.dblClick(screen.getByRole('button', { name: /Attention/ }));
    expect(viewer.tabs.map((t) => t.id)).toEqual(['p1']);
    expect(viewer.activeId).toBe('p1');
  });

  it('clicking while a PDF is active returns to the Library home to inspect', async () => {
    viewer.tabs = [{ id: 'other', title: 'Other' }];
    viewer.activeId = 'other';
    render(PaperRow, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: /Attention/ }));
    expect(selection.id).toBe('p1');
    expect(viewer.activeId).toBe(null); // home shows the detail
    expect(viewer.tabs).toHaveLength(1); // the open tab is untouched
  });
});
```

- [ ] **Step 2: Run — expect FAIL** (current PaperRow opens a tab on single click)

```bash
npm --prefix frontend run test -- src/components/PaperRow.test.ts
```

- [ ] **Step 3: Rewrite `frontend/src/components/PaperRow.svelte`**

```svelte
<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { goHome, openTab, searchMeta, selectPaper, selection, viewer } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { paper }: { paper: PaperSummary } = $props();
  const selected = $derived(selection.id === paper.id);
  const isOpen = $derived(viewer.tabs.some((t) => t.id === paper.id));
  const authors = $derived(
    paper.authors.length > 3
      ? `${paper.authors.slice(0, 3).join(', ')} et al.`
      : paper.authors.join(', '),
  );

  // Click inspects (Library home shows the detail); double-click reads.
  function select() {
    selectPaper(paper.id);
    if (viewer.activeId !== null) goHome();
  }
  function open() {
    openTab(paper);
  }
</script>

<button
  type="button"
  onclick={select}
  ondblclick={open}
  class={`w-full border-l-2 px-4 py-3 text-left transition-colors hover:bg-parchment dark:hover:bg-stone-800/50 ${
    selected ? 'border-amber-700 bg-parchment dark:border-amber-500 dark:bg-stone-800/50' : 'border-transparent'
  }`}
>
  <div class="line-clamp-2 font-serif text-sm font-medium text-ink dark:text-stone-100">
    {paper.title ?? '(untitled)'}
    {#if isOpen}
      <span
        title="Open in a tab"
        class="ml-1 inline-block h-1.5 w-1.5 rounded-full bg-amber-700 align-middle dark:bg-amber-500"
      ></span>
    {/if}
  </div>
  {#if authors}
    <div class="mt-0.5 line-clamp-1 text-xs text-stone-500 dark:text-stone-400">{authors}</div>
  {/if}
  {#if searchMeta.byId[paper.id]}
    {@const m = searchMeta.byId[paper.id]}
    <div class="mt-1 text-xs text-stone-600 dark:text-stone-300">
      <span class="mr-1 rounded bg-parchment px-1 py-px font-mono text-[10px] uppercase tracking-wide text-stone-500 dark:bg-stone-800 dark:text-stone-400">
        {m.field}{#if m.page != null}&nbsp;p.{m.page}{/if}
      </span>
      <!-- Server contract: snippet text is HTML-escaped; only <mark> tags. -->
      <span class="[&_mark]:rounded [&_mark]:bg-yellow-200 [&_mark]:px-0.5 dark:[&_mark]:bg-yellow-500/40">
        {@html m.snippet}
      </span>
    </div>
  {/if}
  <div class="mt-1.5 flex items-center gap-2 text-xs text-stone-500 dark:text-stone-400">
    {#if paper.year}<span>{paper.year}</span>{/if}
    {#if paper.venue}<span class="truncate">{#if paper.year}· {/if}{paper.venue}</span>{/if}
    <StatusPill status={paper.status} />
  </div>
</button>
```

- [ ] **Step 4: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/components/PaperRow.test.ts
```

- [ ] **Step 5: Create `frontend/src/components/SearchBox.svelte`** (search input + options popover; chips move here from the old Sidebar with warm-palette classes)

```svelte
<script lang="ts">
  import { Search, SlidersHorizontal } from 'lucide-svelte';
  import { scale } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import {
    filters,
    searchMeta,
    searchOpts,
    semanticBlocked,
    setSearch,
    toggleSearchEngine,
    toggleSearchField,
  } from '../lib/state.svelte';

  let optionsOpen = $state(false);
  const FIELDS = [
    ['title', 'Title'],
    ['authors', 'Authors'],
    ['abstract', 'Abstract'],
    ['body', 'Body'],
  ] as const;
  const activeCount = $derived(
    FIELDS.filter(([k]) => searchOpts[k]).length +
      Number(searchOpts.keyword) +
      Number(searchOpts.semantic && !semanticBlocked()),
  );
</script>

<div class="relative">
  <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-stone-400" />
  <input
    data-search-input
    type="search"
    aria-label="Search papers"
    placeholder="Search library…"
    value={filters.q}
    oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
    class="w-full rounded-lg border border-stone-200 bg-paper py-2 pl-9 pr-9 text-sm outline-none focus:border-amber-700 focus:ring-2 focus:ring-amber-700/15 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
  />
  <button
    type="button"
    aria-label="Search options"
    aria-expanded={optionsOpen}
    onclick={() => (optionsOpen = !optionsOpen)}
    class="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-stone-400 hover:bg-parchment hover:text-stone-600 dark:hover:bg-stone-700 dark:hover:text-stone-300"
  >
    <SlidersHorizontal size={14} />
  </button>

  {#if optionsOpen}
    <div
      transition:scale={{ start: 0.96, duration: dur(DUR.fast) }}
      class="absolute left-0 right-0 top-full z-20 mt-1 space-y-2 rounded-lg border border-stone-200 bg-paper p-2 shadow-lg dark:border-stone-700 dark:bg-soot"
    >
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Search in</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        {#each FIELDS as [key, label] (key)}
          <button
            type="button"
            aria-pressed={searchOpts[key]}
            onclick={() => toggleSearchField(key)}
            class={`rounded-full border px-2 py-0.5 ${
              searchOpts[key]
                ? 'border-amber-700/40 bg-amber-700/10 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
                : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
            }`}
          >
            {label}
          </button>
        {/each}
      </div>
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Engines</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        <button
          type="button"
          aria-pressed={searchOpts.keyword}
          onclick={() => toggleSearchEngine('keyword')}
          class={`rounded-full border px-2 py-0.5 ${
            searchOpts.keyword
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
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
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
          }`}
        >
          Semantic
        </button>
      </div>
      {#if searchMeta.pending > 0}
        <p class="text-[11px] text-stone-400 dark:text-stone-500">
          indexing {searchMeta.pending} paper{searchMeta.pending === 1 ? '' : 's'}…
        </p>
      {/if}
    </div>
  {/if}
</div>
{#if activeCount < 6 && !optionsOpen}
  <p class="mt-1 text-[10px] text-stone-400">Search options narrowed — open ⚙ to review.</p>
{/if}
```

- [ ] **Step 6: Create `frontend/src/components/FilterRow.svelte`** (the three selects + manage button from the old Sidebar, warm classes)

```svelte
<script lang="ts">
  import { FolderOpen, Settings2 } from 'lucide-svelte';
  import {
    filters,
    loadPapers,
    openProjects,
    projects,
    setProjectFilter,
  } from '../lib/state.svelte';
  import type { Sort, StatusFilter } from '../lib/types';

  function onStatus(e: Event) {
    filters.status = (e.currentTarget as HTMLSelectElement).value as StatusFilter;
    loadPapers();
  }
  function onSort(e: Event) {
    filters.sort = (e.currentTarget as HTMLSelectElement).value as Sort;
    loadPapers();
  }
  function onProject(e: Event) {
    void setProjectFilter((e.currentTarget as HTMLSelectElement).value);
  }

  const selectClasses =
    'min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-xs dark:border-stone-700 dark:bg-stone-800';
</script>

<div class="flex gap-2">
  <select value={filters.status} aria-label="Filter by status" onchange={onStatus} class={selectClasses}>
    <option value="all">All status</option>
    <option value="resolved">Resolved</option>
    <option value="needs_review">Needs review</option>
  </select>
  <select value={filters.sort} aria-label="Sort papers" onchange={onSort} class={selectClasses}>
    <option value="year_desc">Newest</option>
    <option value="year_asc">Oldest</option>
    <option value="added_desc">Recently added</option>
    <option value="title">Title A–Z</option>
  </select>
</div>
<div class="mt-2 flex items-center gap-2">
  <FolderOpen size={16} class="shrink-0 text-stone-500 dark:text-stone-400" />
  <select value={filters.project} aria-label="Filter by project" onchange={onProject} class={selectClasses}>
    <option value="all">All projects</option>
    {#each projects.items as p (p.id)}
      <option value={p.id}>{p.name} ({p.paper_count})</option>
    {/each}
  </select>
  <button
    type="button"
    aria-label="Manage projects"
    onclick={openProjects}
    class="rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:bg-parchment dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800"
  >
    <Settings2 size={16} />
  </button>
</div>
```

- [ ] **Step 7: Create `frontend/src/components/PaperList.svelte`** (FLIP + staggered entrance)

```svelte
<script lang="ts">
  import { flip } from 'svelte/animate';
  import { fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { library } from '../lib/state.svelte';
  import PaperRow from './PaperRow.svelte';
</script>

<div class="min-h-0 flex-1 divide-y divide-stone-200/60 overflow-y-auto dark:divide-stone-800/60">
  {#if library.loading}
    <p class="p-4 text-sm text-stone-500 dark:text-stone-400">Loading…</p>
  {:else if library.error}
    <p class="p-4 text-sm text-red-600 dark:text-red-400">{library.error}</p>
  {:else if library.papers.length === 0}
    <p class="p-4 text-sm text-stone-500 dark:text-stone-400">
      No papers match. Clear the search or import one.
    </p>
  {:else}
    {#each library.papers as paper, i (paper.id)}
      <div
        animate:flip={{ duration: dur(DUR.base) }}
        in:fade={{ duration: dur(DUR.base), delay: dur(Math.min(i * 20, 160)) }}
      >
        <PaperRow {paper} />
      </div>
    {/each}
  {/if}
</div>
```

- [ ] **Step 8: Create `frontend/src/components/LibraryPane.svelte`** (assembly + export footer from old Sidebar)

```svelte
<script lang="ts">
  import { Download } from 'lucide-svelte';
  import { exportUrl } from '../lib/api';
  import { bibFormat, filters } from '../lib/state.svelte';
  import FilterRow from './FilterRow.svelte';
  import PaperList from './PaperList.svelte';
  import SearchBox from './SearchBox.svelte';
</script>

<aside class="flex h-full w-[304px] shrink-0 flex-col border-r border-stone-200 bg-parchment/60 dark:border-stone-800 dark:bg-soot/60">
  <div class="space-y-3 border-b border-stone-200 p-3 dark:border-stone-800">
    <SearchBox />
    <FilterRow />
  </div>

  <PaperList />

  <div class="border-t border-stone-200 p-2 dark:border-stone-800">
    {#if filters.q.trim()}
      <!-- Batch export filters by the legacy title/author match, not hybrid
           search results — hidden while a query is active to avoid exporting
           a different set than the list shows. -->
      <span
        title="Clear the search to export"
        class="inline-flex w-full cursor-not-allowed items-center justify-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1.5 text-xs font-medium text-stone-400 dark:border-stone-700 dark:text-stone-600"
      >
        <Download size={14} /> Export .bib
      </span>
    {:else}
      <a
        href={exportUrl(filters, bibFormat.value)}
        download="xuewen.bib"
        class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1.5 text-xs font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      >
        <Download size={14} /> Export .bib
      </a>
    {/if}
  </div>
</aside>
```

- [ ] **Step 9: Swap in App and delete Sidebar**

In `frontend/src/App.svelte`: replace `import Sidebar from './components/Sidebar.svelte';` with `import LibraryPane from './components/LibraryPane.svelte';` and `{#if ui.sidebarOpen}<Sidebar />{/if}` with `{#if ui.sidebarOpen}<LibraryPane />{/if}` (the spring-driven width arrives in Task 8).

```bash
git rm frontend/src/components/Sidebar.svelte
```

- [ ] **Step 10: Verify everything, commit**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
git add -A frontend/src
git -c commit.gpgsign=false commit -m "feat(frontend): decompose sidebar into LibraryPane with select-vs-open rows"
```

---

## Task 6: DetailView, Welcome, and the slimmed InfoPanel

**Files:**
- Create: `frontend/src/components/DetailView.svelte`
- Create: `frontend/src/components/Welcome.svelte`
- Rewrite: `frontend/src/components/InfoPanel.svelte`
- Test: `frontend/src/components/DetailView.test.ts` (new)

**Interfaces:**
- Consumes: Task 4 components (`PaperMeta`, `ProjectTags`, `CiteActions`, `PaperActions`, `SealMark`), Task 2 `selection`, existing `loadDetail`, `detailRefresh`, `library`, `openTab`, `openImport`.
- Produces: `DetailView` (no props — reads `selection`), `Welcome` (no props), `InfoPanel { id: string }` (same prop as today).

- [ ] **Step 1: Write the failing DetailView test**

`frontend/src/components/DetailView.test.ts`:

```ts
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DetailView from './DetailView.svelte';
import { library, selection, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const summary: PaperSummary = {
  id: 'p1', title: 'Attention Is All You Need', authors: ['Vaswani'], venue: 'NeurIPS',
  year: 2017, doi: '10.1/x', arxiv_id: null, dblp_key: null, cite_key: 'vaswani2017',
  url: null, source: 'crossref', status: 'resolved', added_at: '',
};

beforeEach(() => {
  selection.id = null;
  viewer.tabs = [];
  viewer.activeId = null;
  library.papers = [summary];
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(
        JSON.stringify({ ...summary, abstract: 'The dominant sequence transduction models…', project_ids: [] }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    ),
  );
});

describe('DetailView', () => {
  it('shows the welcome panel when nothing is selected', () => {
    render(DetailView);
    expect(screen.getByText(/Select a paper/i)).toBeInTheDocument();
  });

  it('renders the selected paper and opens its PDF', async () => {
    selection.id = 'p1';
    render(DetailView);
    expect(await screen.findByText('Attention Is All You Need')).toBeInTheDocument();
    expect(await screen.findByText(/dominant sequence transduction/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Open PDF' }));
    expect(viewer.activeId).toBe('p1');
  });
});
```

- [ ] **Step 2: Run — expect FAIL (component missing)**

```bash
npm --prefix frontend run test -- src/components/DetailView.test.ts
```

- [ ] **Step 3: Create `frontend/src/components/Welcome.svelte`**

```svelte
<script lang="ts">
  import { Upload } from 'lucide-svelte';
  import { library, openImport } from '../lib/state.svelte';
  import SealMark from './SealMark.svelte';
</script>

<div class="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
  <SealMark size={48} />
  <h2 class="font-serif text-2xl font-semibold text-ink dark:text-stone-100">Xuewen</h2>
  {#if library.papers.length === 0}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Your library is empty. Import a PDF, a DOI, or an arXiv link to begin.
    </p>
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import papers
    </button>
  {:else}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Select a paper to see its details. Double-click to read it.
    </p>
    <dl class="grid grid-cols-[auto_auto] gap-x-3 gap-y-1 text-xs text-stone-400 dark:text-stone-500">
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">/</kbd></dt>
      <dd class="text-left">search</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">⌘K</kbd></dt>
      <dd class="text-left">command palette</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">z</kbd></dt>
      <dd class="text-left">zen mode while reading</dd>
    </dl>
  {/if}
</div>
```

- [ ] **Step 4: Create `frontend/src/components/DetailView.svelte`**

```svelte
<script lang="ts">
  import { BookOpen } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { detailRefresh, library, loadDetail, openTab, selection } from '../lib/state.svelte';
  import CiteActions from './CiteActions.svelte';
  import PaperActions from './PaperActions.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import ProjectTags from './ProjectTags.svelte';
  import Welcome from './Welcome.svelte';

  function openPdf(id: string) {
    const p = library.papers.find((x) => x.id === id);
    if (p) openTab(p);
  }
</script>

{#if selection.id === null}
  <Welcome />
{:else}
  {#key `${selection.id}-${detailRefresh.n}`}
    <div class="h-full min-w-0 flex-1 overflow-y-auto">
      <article class="mx-auto max-w-3xl px-8 py-10">
        {#await loadDetail(selection.id)}
          <p class="text-sm text-stone-500 dark:text-stone-400">Loading…</p>
        {:then d}
          <header in:fly={{ y: 8, duration: dur(DUR.base) }}>
            <PaperMeta {d} hero />
          </header>
          <div
            in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(60) }}
            class="mt-6 flex flex-wrap items-center gap-3 border-y border-stone-200 py-3 dark:border-stone-800"
          >
            <button
              type="button"
              onclick={() => openPdf(d.id)}
              class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
            >
              <BookOpen size={15} /> Open PDF
            </button>
            <CiteActions id={d.id} citeKey={d.cite_key} />
          </div>
          {#if d.abstract}
            <section in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(120) }} class="mt-6">
              <h3 class="text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">
                Abstract
              </h3>
              <p class="mt-2 max-w-[65ch] font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
                {d.abstract}
              </p>
            </section>
          {/if}
          <section in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(180) }} class="mt-6 max-w-sm">
            <ProjectTags {d} />
          </section>
          <footer
            in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(240) }}
            class="mt-10 border-t border-stone-200 pt-4 dark:border-stone-800"
          >
            <PaperActions {d} />
          </footer>
        {:catch}
          <p class="text-sm text-red-600 dark:text-red-400">
            Failed to load details. Check that the server is running, then select the paper again.
          </p>
        {/await}
      </article>
    </div>
  {/key}
{/if}
```

- [ ] **Step 5: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/components/DetailView.test.ts
```

- [ ] **Step 6: Rewrite `frontend/src/components/InfoPanel.svelte`** as a thin composition (all lifted logic now lives in the shared components)

```svelte
<script lang="ts">
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { detailRefresh, loadDetail } from '../lib/state.svelte';
  import CiteActions from './CiteActions.svelte';
  import PaperActions from './PaperActions.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import ProjectTags from './ProjectTags.svelte';

  let { id }: { id: string } = $props();
</script>

<aside
  transition:fly={{ x: 24, duration: dur(DUR.base) }}
  class="flex h-full w-80 shrink-0 flex-col overflow-y-auto border-l border-stone-200 bg-paper p-4 dark:border-stone-800 dark:bg-night"
>
  {#key `${id}-${detailRefresh.n}`}
    {#await loadDetail(id)}
      <p class="text-sm text-stone-500 dark:text-stone-400">Loading…</p>
    {:then d}
      <PaperMeta {d} />
      <div class="mt-4"><ProjectTags {d} /></div>
      {#if d.abstract}
        <section class="mt-4">
          <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">
            Abstract
          </h3>
          <p class="text-sm leading-relaxed text-stone-600 dark:text-stone-300">{d.abstract}</p>
        </section>
      {/if}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Cite</h3>
        <CiteActions id={d.id} citeKey={d.cite_key} />
      </div>
      <div class="mt-6 border-t border-stone-200 pt-4 dark:border-stone-800">
        <PaperActions {d} />
      </div>
    {:catch}
      <p class="text-sm text-red-600 dark:text-red-400">Failed to load details.</p>
    {/await}
  {/key}
</aside>
```

- [ ] **Step 7: Verify all suites, commit**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
git add frontend/src/components/{DetailView,Welcome,InfoPanel}.svelte frontend/src/components/DetailView.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): DetailView library home, welcome panel, slimmed InfoPanel"
```

---

## Task 7: TabBar with the Library home tab + App content-pane wiring

**Files:**
- Rewrite: `frontend/src/components/TabBar.svelte`
- Modify: `frontend/src/App.svelte`
- Delete: `frontend/src/components/EmptyState.svelte`
- Test: `frontend/src/components/TabBar.test.ts` (extend)

**Interfaces:**
- Consumes: `goHome`, `toggleZen`, `closeTab`, `viewer` from state; `DetailView` from Task 6.
- Produces: the content pane contract used by Tasks 8–9: TabBar always rendered (except zen); `PdfViewer` stays mounted (hidden) while home is active so iframes never reload.

- [ ] **Step 1: Extend the TabBar test — add these cases to `frontend/src/components/TabBar.test.ts`**

```ts
import { goHome, ui } from '../lib/state.svelte'; // extend the existing import list

  it('always shows the Library home tab and returns home on click', async () => {
    openTab(paper('a', 'First Paper'));
    render(TabBar);
    const home = screen.getByRole('button', { name: 'Library' });
    expect(home).toBeInTheDocument();
    home.click();
    await Promise.resolve();
    expect(viewer.activeId).toBe(null);
    expect(viewer.tabs.length).toBe(1); // tabs survive going home
  });

  it('marks the home tab current when no PDF tab is active', () => {
    render(TabBar);
    expect(screen.getByRole('button', { name: 'Library' })).toHaveAttribute('aria-current', 'page');
  });

  it('shows the zen toggle only while a PDF tab is active', async () => {
    render(TabBar);
    expect(screen.queryByRole('button', { name: 'Zen mode' })).not.toBeInTheDocument();
    openTab(paper('a', 'First Paper'));
    await Promise.resolve();
    expect(screen.getByRole('button', { name: 'Zen mode' })).toBeInTheDocument();
  });
```

Also add `ui.zen = false;` to the existing `beforeEach`.

- [ ] **Step 2: Run — expect FAIL (no home tab yet)**

```bash
npm --prefix frontend run test -- src/components/TabBar.test.ts
```

- [ ] **Step 3: Rewrite `frontend/src/components/TabBar.svelte`**

```svelte
<script lang="ts">
  import { Info, LibraryBig, Maximize2, X } from 'lucide-svelte';
  import { flip } from 'svelte/animate';
  import { crossfade, fade } from 'svelte/transition';
  import { DUR, EASE, dur } from '../lib/motion';
  import { closeTab, goHome, toggleZen, viewer } from '../lib/state.svelte';

  // The active-tab underline crossfades between tabs — a real sliding
  // indicator with no measurement code.
  const [send, receive] = crossfade({ duration: dur(DUR.fast) });
</script>

<div class="flex h-11 shrink-0 items-center border-b border-stone-200 bg-paper dark:border-stone-800 dark:bg-night">
  <button
    type="button"
    aria-label="Library"
    aria-current={viewer.activeId === null ? 'page' : undefined}
    onclick={goHome}
    class={`relative flex h-11 shrink-0 items-center gap-1.5 px-3 text-sm ${
      viewer.activeId === null
        ? 'text-ink dark:text-stone-100'
        : 'text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800/40'
    }`}
  >
    <LibraryBig size={15} />
    Library
    {#if viewer.activeId === null}
      <span
        in:receive={{ key: 'tab-underline' }}
        out:send={{ key: 'tab-underline' }}
        class="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
      ></span>
    {/if}
  </button>
  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each viewer.tabs as tab (tab.id)}
      <div
        animate:flip={{ duration: dur(DUR.base), easing: undefined }}
        out:fade={{ duration: dur(DUR.fast) }}
        class={`group relative flex h-11 max-w-52 shrink-0 items-center gap-2 border-r border-stone-200 px-3 dark:border-stone-800 ${
          viewer.activeId === tab.id
            ? 'bg-parchment/70 dark:bg-stone-800/60'
            : 'hover:bg-parchment/50 dark:hover:bg-stone-800/30'
        }`}
      >
        <button
          type="button"
          onclick={() => (viewer.activeId = tab.id)}
          class="min-w-0 truncate font-serif text-sm text-stone-700 dark:text-stone-200"
        >
          {tab.title}
        </button>
        <button
          type="button"
          aria-label="Close tab"
          onclick={() => closeTab(tab.id)}
          class="rounded p-0.5 text-stone-500 opacity-0 hover:bg-stone-200 group-hover:opacity-100 dark:text-stone-400 dark:hover:bg-stone-700"
        >
          <X size={14} />
        </button>
        {#if viewer.activeId === tab.id}
          <span
            in:receive={{ key: 'tab-underline' }}
            out:send={{ key: 'tab-underline' }}
            class="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
          ></span>
        {/if}
      </div>
    {/each}
  </div>

  {#if viewer.activeId !== null}
    <button
      type="button"
      aria-label="Zen mode"
      title="Zen mode (z)"
      onclick={toggleZen}
      class="mr-1 shrink-0 rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <Maximize2 size={16} />
    </button>
    <button
      type="button"
      aria-label="Toggle info"
      onclick={() => (viewer.infoOpen = !viewer.infoOpen)}
      class={`mr-2 shrink-0 rounded-lg p-2 ${
        viewer.infoOpen
          ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
          : 'text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800'
      }`}
    >
      <Info size={18} />
    </button>
  {/if}
</div>
```

Note: `easing: undefined` on `flip` just uses the default; remove the property if svelte-check complains.

- [ ] **Step 4: Rewrite the `App.svelte` template's main area** (script imports: drop `EmptyState`, add `DetailView`)

```svelte
<div class="flex h-full flex-col bg-paper text-ink dark:bg-night dark:text-stone-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    {#if ui.sidebarOpen}<LibraryPane />{/if}
    <main class="flex min-h-0 min-w-0 flex-1 flex-col">
      <TabBar />
      <div class="flex min-h-0 flex-1">
        <!-- PdfViewer stays mounted while home is active so iframe scroll
             positions survive a trip to the Library. -->
        <div class={`min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
        </div>
        {#if viewer.activeId === null}
          <DetailView />
        {/if}
      </div>
    </main>
  </div>
</div>
{#if ui.importOpen}<ImportModal />{/if}
{#if identifyState.open}<IdentifyModal />{/if}
{#if ui.projectsOpen}<ProjectsModal />{/if}
<Toaster />
```

Then delete the empty state:

```bash
git rm frontend/src/components/EmptyState.svelte
```

- [ ] **Step 5: Run everything — expect PASS**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
```

- [ ] **Step 6: Manual smoke test**

```bash
npm --prefix frontend run dev
```

Against a running `xuewen serve`: Library tab shows Welcome → select paper → detail; double-click → PDF tab with underline sliding; back to Library; reopen tab — the PDF does **not** reload (scroll preserved).

- [ ] **Step 7: Commit**

```bash
git add -A frontend/src
git -c commit.gpgsign=false commit -m "feat(frontend): Library home tab, crossfade tab underline, persistent PDF iframes"
```

---

## Task 8: TopBar rework, spring-driven pane collapse, edge peek

**Files:**
- Rewrite: `frontend/src/components/TopBar.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Consumes: `SealMark`, `Spring` from `svelte/motion`, `SPRINGS`/`prefersReducedMotion` from motion.
- Produces: pane width spring pattern reused by zen (Task 9); TopBar's ⌘K chip calls `ui.paletteOpen = true`.

- [ ] **Step 1: Rewrite `frontend/src/components/TopBar.svelte`**

```svelte
<script lang="ts">
  import { Monitor, Moon, PanelLeft, Sun, Upload } from 'lucide-svelte';
  import { openImport, stats, theme, toggleSidebar, toggleTheme, ui } from '../lib/state.svelte';
  import SealMark from './SealMark.svelte';

  const themeLabel = $derived(
    theme.mode === 'light' ? 'Light' : theme.mode === 'dark' ? 'Dark' : 'System',
  );
</script>

<header class="flex h-14 shrink-0 items-center justify-between border-b border-stone-200 bg-paper px-4 dark:border-stone-800 dark:bg-night">
  <div class="flex items-center gap-2">
    <button
      type="button"
      onclick={toggleSidebar}
      aria-label="Toggle list pane"
      title="Toggle list pane ([)"
      class="rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <PanelLeft size={18} />
    </button>
    <SealMark size={22} />
    <span class="font-serif text-lg font-semibold tracking-tight">Xuewen</span>
  </div>
  <div class="flex items-center gap-3">
    {#if stats.value}
      <div class="hidden items-center gap-3 text-xs text-stone-500 sm:flex dark:text-stone-400">
        <span>{stats.value.total} papers</span>
        <span class="text-lime-700 dark:text-lime-400">{stats.value.resolved} resolved</span>
        <span class="text-yellow-700 dark:text-yellow-400">{stats.value.needs_review} to review</span>
      </div>
    {/if}
    <button
      type="button"
      onclick={() => (ui.paletteOpen = true)}
      class="hidden items-center gap-1 rounded-lg border border-stone-200 px-2 py-1 text-xs text-stone-400 hover:bg-parchment sm:inline-flex dark:border-stone-700 dark:hover:bg-stone-800"
    >
      <kbd>⌘K</kbd>
    </button>
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import
    </button>
    <button
      type="button"
      onclick={toggleTheme}
      aria-label={`Theme: ${themeLabel} (click to change)`}
      title={`Theme: ${themeLabel}`}
      class="rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      {#if theme.mode === 'light'}<Sun size={18} />{:else if theme.mode === 'dark'}<Moon size={18} />{:else}<Monitor size={18} />{/if}
    </button>
  </div>
</header>
```

- [ ] **Step 2: Add the spring pane + edge peek to `frontend/src/App.svelte`**

Script additions:

```ts
import { Spring } from 'svelte/motion';
import { SPRINGS, prefersReducedMotion } from './lib/motion';

const PANE_W = 304;
const paneW = new Spring(PANE_W, SPRINGS.pane);
let peek = $state(false);
const paneHidden = $derived(!ui.sidebarOpen || ui.zen);
$effect(() => {
  const target = paneHidden ? 0 : PANE_W;
  if (import.meta.env.MODE === 'test' || prefersReducedMotion()) {
    paneW.set(target, { instant: true });
  } else {
    paneW.target = target;
  }
});
$effect(() => {
  if (!paneHidden) peek = false;
});
```

Template: replace `{#if ui.sidebarOpen}<LibraryPane />{/if}` with:

```svelte
    <div class="relative min-h-0 shrink-0 overflow-hidden" style={`width:${paneW.current}px`}>
      <div class="absolute inset-y-0 left-0 w-[304px]"><LibraryPane /></div>
    </div>
    {#if paneHidden}
      <!-- Edge peek: hover the left edge to overlay the list without expanding it. -->
      <div class="absolute inset-y-0 left-0 z-30 w-2" onmouseenter={() => (peek = true)} role="presentation"></div>
      {#if peek}
        <div
          transition:fly={{ x: -24, duration: dur(DUR.base) }}
          onmouseleave={() => (peek = false)}
          role="presentation"
          class="absolute inset-y-0 left-0 z-40 shadow-2xl"
        >
          <LibraryPane />
        </div>
      {/if}
    {/if}
```

The enclosing `<div class="flex min-h-0 flex-1">` gains `relative`: `<div class="relative flex min-h-0 flex-1">`. Add `import { fly } from 'svelte/transition';` and `import { DUR, dur } from './lib/motion';` to the script.

- [ ] **Step 3: Verify + manual smoke**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
npm --prefix frontend run dev
```

Manual: `[` isn't wired yet — click the PanelLeft button; the pane springs closed with a slight settle; hovering the left edge slides the overlay in; moving away dismisses it.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/TopBar.svelte frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): seal wordmark TopBar, spring pane collapse, edge peek"
```

---

## Task 9: Zen mode chrome (hide TopBar/TabBar, ZenPill)

**Files:**
- Create: `frontend/src/components/ZenPill.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Consumes: `ui.zen`, `viewer`, `toggleZen` (state), `slide`/`fly` transitions.
- Produces: zen chrome contract for Task 11 (`z`/`Esc` only flip `ui.zen`; the DOM follows).

- [ ] **Step 1: Create `frontend/src/components/ZenPill.svelte`**

```svelte
<script lang="ts">
  import { Minimize2 } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { ui, viewer } from '../lib/state.svelte';

  const title = $derived(viewer.tabs.find((t) => t.id === viewer.activeId)?.title ?? '');
</script>

<div
  transition:fly={{ y: -16, duration: dur(DUR.base) }}
  class="fixed left-1/2 top-3 z-50 flex max-w-md -translate-x-1/2 items-center gap-2 rounded-full border border-stone-200 bg-paper/90 px-4 py-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/90"
>
  <span class="truncate font-serif text-sm text-ink dark:text-stone-100">{title}</span>
  <button
    type="button"
    aria-label="Exit zen mode"
    title="Exit zen (Esc)"
    onclick={() => (ui.zen = false)}
    class="rounded-full p-1 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
  >
    <Minimize2 size={14} />
  </button>
</div>
```

- [ ] **Step 2: Wire zen into `frontend/src/App.svelte`**

Add `import { slide } from 'svelte/transition';` and `import ZenPill from './components/ZenPill.svelte';`. Wrap the chrome:

```svelte
  {#if !ui.zen}
    <div transition:slide={{ duration: dur(DUR.base) }}>
      <TopBar />
    </div>
  {/if}
```

and inside `<main>`:

```svelte
      {#if !ui.zen}
        <div transition:slide={{ duration: dur(DUR.base) }}>
          <TabBar />
        </div>
      {/if}
```

After the modals, before `<Toaster />`:

```svelte
{#if ui.zen}<ZenPill />{/if}
```

(The pane already collapses in zen via Task 8's `paneHidden`, and the edge peek still works, so the next paper is one hover away.)

- [ ] **Step 3: Verify + manual smoke**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
npm --prefix frontend run dev
```

Manual: open a PDF → click the Maximize button → TopBar/TabBar slide away together, pane springs shut, pill drops in; peek the left edge; click a row → returns home and exits zen (state invariant from Task 2).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/ZenPill.svelte frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): zen mode chrome with floating pill"
```

---

## Task 10: Shared Modal wrapper; port Import/Identify/Projects modals

**Files:**
- Create: `frontend/src/components/Modal.svelte`
- Test: `frontend/src/components/Modal.test.ts` (new)
- Modify: `frontend/src/components/ImportModal.svelte`
- Modify: `frontend/src/components/IdentifyModal.svelte`
- Modify: `frontend/src/components/ProjectsModal.svelte`

**Interfaces:**
- Produces: `Modal { title: string; onclose: () => void; children: Snippet; footer?: Snippet }` — animated backdrop+panel, Esc closes (stopping propagation so global shortcuts don't also fire), backdrop click closes, focus moves into the panel and returns on close.
- Constraint: `ProjectsModal.test.ts` labels must keep passing verbatim: placeholder `New project name…`, buttons `Add`, `Delete Survey`, `Delete`, label `Rename Survey`.

- [ ] **Step 1: Write the failing Modal test**

`frontend/src/components/Modal.test.ts`:

```ts
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { createRawSnippet } from 'svelte';
import Modal from './Modal.svelte';

function body() {
  return createRawSnippet(() => ({ render: () => '<p>modal body</p>' }));
}

describe('Modal', () => {
  it('renders a dialog with the title and body', () => {
    render(Modal, { props: { title: 'Test dialog', onclose: () => {}, children: body() } });
    expect(screen.getByRole('dialog', { name: 'Test dialog' })).toBeInTheDocument();
    expect(screen.getByText('modal body')).toBeInTheDocument();
  });

  it('closes on Escape', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.keyboard('{Escape}');
    expect(onclose).toHaveBeenCalled();
  });

  it('closes on backdrop click but not on panel click', async () => {
    const onclose = vi.fn();
    render(Modal, { props: { title: 'Test dialog', onclose, children: body() } });
    await userEvent.click(screen.getByText('modal body'));
    expect(onclose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('presentation'));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run — expect FAIL (component missing)**

```bash
npm --prefix frontend run test -- src/components/Modal.test.ts
```

- [ ] **Step 3: Create `frontend/src/components/Modal.svelte`**

```svelte
<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { Snippet } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';

  let {
    title,
    onclose,
    children,
    footer,
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();

  let panel = $state<HTMLElement | null>(null);

  // Move focus into the dialog; hand it back when the dialog unmounts.
  $effect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel?.focus();
    return () => previous?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // The dialog owns Esc — global shortcuts (zen exit) must not also fire.
      e.stopPropagation();
      onclose();
    } else if (e.key === 'Tab' && panel) {
      const focusables = panel.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={onkeydown} />

<div
  transition:fade={{ duration: dur(DUR.fast) }}
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
  class="fixed inset-0 z-50 flex items-center justify-center bg-stone-950/40 p-4 backdrop-blur-[2px]"
>
  <div
    bind:this={panel}
    tabindex="-1"
    transition:scale={{ start: 0.96, duration: dur(DUR.base) }}
    role="dialog"
    aria-modal="true"
    aria-label={title}
    class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-stone-200 bg-paper text-ink shadow-2xl outline-none dark:border-stone-800 dark:bg-soot dark:text-stone-100"
  >
    <div class="flex items-center justify-between border-b border-stone-200 p-4 dark:border-stone-800">
      <h2 class="font-serif text-base font-semibold">{title}</h2>
      <button
        type="button"
        onclick={onclose}
        aria-label="Close dialog"
        class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        <X size={18} />
      </button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      {@render children()}
    </div>
    {#if footer}
      <div class="border-t border-stone-200 p-3 dark:border-stone-800">
        {@render footer()}
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 4: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/components/Modal.test.ts
```

- [ ] **Step 5: Port the three modals onto `Modal`**

For each, the `<script>` block is unchanged except noted; the outer two `<div>`s + header + footer chrome are replaced by `Modal`.

`ImportModal.svelte` — template becomes:

```svelte
<Modal title="Import papers" onclose={closeImport}>
  <!-- everything that was inside the scroll body, verbatim:
       URL form, drop zone, hidden file input, items list, EZproxy details -->
  {#snippet footer()}
    {#if importState.items.length}
      <p class="text-xs text-stone-500 dark:text-stone-400">
        {summary.ingested} ingested, {summary.skipped} skipped, {summary.failed} failed
      </p>
    {/if}
  {/snippet}
</Modal>
```

(Add `import Modal from './Modal.svelte';`. Since the footer snippet is conditional inside, always pass it. Restyle inner slate→stone / indigo→amber classes: inputs `border-stone-300 dark:border-stone-700`, primary buttons `bg-amber-700 hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500`, drop-zone active state `border-amber-600 bg-amber-700/5 dark:bg-amber-500/10`, badges emerald→lime.)

`IdentifyModal.svelte` — template becomes:

```svelte
<Modal title="Identify paper" onclose={closeIdentify}>
  <!-- search form, error line, direct-identifier note, candidate list — verbatim,
       with slate→stone, indigo→amber (selected candidate:
       'border-amber-600 bg-amber-700/5 dark:bg-amber-500/10'), amber warnings → yellow-700 -->
  {#snippet footer()}
    {#if identifyState.selected || identifyState.direct}
      {#if dropsIdentifier(identifyState)}
        <p class="mb-2 text-xs text-yellow-700 dark:text-yellow-400">
          Applying this match will drop an identifier the paper currently has
          (DOI/arXiv id not present in the selected record).
        </p>
      {/if}
      <button
        type="button"
        onclick={() => void applyIdentify()}
        disabled={identifyState.busy}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <Check size={14} /> Apply match
      </button>
    {/if}
  {/snippet}
</Modal>
```

`ProjectsModal.svelte` — template becomes:

```svelte
<Modal title="Projects" onclose={closeProjects}>
  <!-- create form + project list — verbatim markup with slate→stone,
       indigo→amber on the Add button. KEEP these strings exactly (tests):
       placeholder "New project name…", button text "Add", aria-label
       "Rename {p.name}", aria-label "Delete {p.name}", confirm button "Delete". -->
  {#snippet footer()}
    <div class="text-right">
      <button
        type="button"
        onclick={closeProjects}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <Check size={14} /> Done
      </button>
    </div>
  {/snippet}
</Modal>
```

- [ ] **Step 6: Run all suites — the ProjectsModal tests are the canary**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
```

Expected: PASS. If `ProjectsModal.test.ts` fails, a required label string was changed — restore it.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/{Modal,ImportModal,IdentifyModal,ProjectsModal}.svelte frontend/src/components/Modal.test.ts
git -c commit.gpgsign=false commit -m "feat(frontend): shared animated Modal wrapper for all dialogs"
```

---

## Task 11: Keyboard shortcuts

**Files:**
- Create: `frontend/src/lib/shortcuts.ts`
- Test: `frontend/src/lib/shortcuts.test.ts`
- Modify: `frontend/src/App.svelte` (wire `<svelte:window>`)

**Interfaces:**
- Consumes: state stores/fns from Task 2, `identifyState`.
- Produces: `handleKeydown(e: KeyboardEvent): void`, `isEditable(t: EventTarget | null): boolean`.
- **Spec deviation (documented):** the spec lists `⌘W` for close-tab, but browsers reserve ⌘W/Ctrl+W to close the browser tab — it cannot be intercepted. The in-app binding is **`x`** (close active tab). Flag this in the PR/summary for the spec to be amended.

Keymap: `/` focus search · `⌘K`/`Ctrl+K` palette · `[` pane · `z` zen · `x` close tab · `j`/`k` move selection · `Enter` open selected · `Esc` close palette / exit zen (modals own their Esc via Task 10's stopPropagation).

- [ ] **Step 1: Write the failing tests**

`frontend/src/lib/shortcuts.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { handleKeydown, isEditable } from './shortcuts';
import { identifyState, library, selection, ui, viewer } from './state.svelte';
import type { PaperSummary } from './types';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

function key(k: string, opts: Partial<KeyboardEvent> & { target?: EventTarget } = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: k, ...opts });
  if (opts.target) Object.defineProperty(e, 'target', { value: opts.target });
  return e;
}

beforeEach(() => {
  library.papers = [paper('a'), paper('b'), paper('c')];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  ui.zen = false;
  ui.paletteOpen = false;
  ui.sidebarOpen = true;
  ui.importOpen = false;
  ui.projectsOpen = false;
  identifyState.open = false;
});

describe('isEditable', () => {
  it('flags inputs and textareas', () => {
    expect(isEditable(document.createElement('input'))).toBe(true);
    expect(isEditable(document.createElement('textarea'))).toBe(true);
    expect(isEditable(document.createElement('div'))).toBe(false);
    expect(isEditable(null)).toBe(false);
  });
});

describe('handleKeydown', () => {
  it('cmd+k toggles the palette even from an input', () => {
    handleKeydown(key('k', { metaKey: true, target: document.createElement('input') }));
    expect(ui.paletteOpen).toBe(true);
  });

  it('[ toggles the pane; ignored while typing', () => {
    handleKeydown(key('['));
    expect(ui.sidebarOpen).toBe(false);
    handleKeydown(key('[', { target: document.createElement('input') }));
    expect(ui.sidebarOpen).toBe(false); // unchanged
  });

  it('j/k move the selection through the list, Enter opens it', () => {
    handleKeydown(key('j'));
    expect(selection.id).toBe('a');
    handleKeydown(key('j'));
    expect(selection.id).toBe('b');
    handleKeydown(key('k'));
    expect(selection.id).toBe('a');
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
  });

  it('z toggles zen only with an active tab; x closes the active tab', () => {
    handleKeydown(key('z'));
    expect(ui.zen).toBe(false);
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    expect(ui.zen).toBe(true);
    handleKeydown(key('x'));
    expect(viewer.tabs).toHaveLength(0);
    expect(ui.zen).toBe(false);
  });

  it('Escape closes the palette first, then exits zen', () => {
    ui.paletteOpen = true;
    ui.zen = true;
    handleKeydown(key('Escape'));
    expect(ui.paletteOpen).toBe(false);
    expect(ui.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(ui.zen).toBe(false);
  });

  it('single-key shortcuts are inert while a modal is open', () => {
    ui.importOpen = true;
    handleKeydown(key('['));
    expect(ui.sidebarOpen).toBe(true);
    handleKeydown(key('Escape')); // the modal owns Esc
    expect(ui.importOpen).toBe(true); // handler must not touch it
  });
});
```

- [ ] **Step 2: Run — expect FAIL (module missing)**

```bash
npm --prefix frontend run test -- src/lib/shortcuts.test.ts
```

- [ ] **Step 3: Implement `frontend/src/lib/shortcuts.ts`**

```ts
import {
  closeTab,
  identifyState,
  library,
  openTab,
  selection,
  selectPaper,
  toggleSidebar,
  toggleZen,
  ui,
  viewer,
} from './state.svelte';

export function isEditable(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return (
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLSelectElement ||
    t.isContentEditable
  );
}

function anyModalOpen(): boolean {
  return ui.importOpen || ui.projectsOpen || identifyState.open;
}

function moveSelection(delta: number): void {
  const papers = library.papers;
  if (papers.length === 0) return;
  const idx = papers.findIndex((p) => p.id === selection.id);
  const next = idx === -1 ? (delta > 0 ? 0 : papers.length - 1) : Math.min(papers.length - 1, Math.max(0, idx + delta));
  selectPaper(papers[next].id);
}

function openSelected(): void {
  const p = library.papers.find((x) => x.id === selection.id);
  if (p) openTab(p);
}

function focusSearch(): void {
  document.querySelector<HTMLInputElement>('[data-search-input]')?.focus();
}

/// Global keymap. Modals own their Esc (Modal.svelte stops propagation);
/// everything except ⌘K is inert while a modal is open or focus is in a
/// text control. Spec deviation: close-tab is `x`, not ⌘W — browsers
/// reserve ⌘W/Ctrl+W for closing the browser tab.
export function handleKeydown(e: KeyboardEvent): void {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    ui.paletteOpen = !ui.paletteOpen;
    return;
  }
  if (anyModalOpen()) return;
  if (e.key === 'Escape') {
    if (ui.paletteOpen) ui.paletteOpen = false;
    else if (ui.zen) ui.zen = false;
    return;
  }
  if (isEditable(e.target) || ui.paletteOpen) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  switch (e.key) {
    case '/':
      e.preventDefault();
      focusSearch();
      break;
    case '[':
      toggleSidebar();
      break;
    case 'z':
      toggleZen();
      break;
    case 'x':
      if (viewer.activeId) closeTab(viewer.activeId);
      break;
    case 'j':
      moveSelection(1);
      break;
    case 'k':
      moveSelection(-1);
      break;
    case 'Enter':
      openSelected();
      break;
  }
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/lib/shortcuts.test.ts
```

- [ ] **Step 5: Wire in `frontend/src/App.svelte`**

Add `import { handleKeydown } from './lib/shortcuts';` and, at the top of the template:

```svelte
<svelte:window onkeydown={handleKeydown} />
```

- [ ] **Step 6: Verify all, commit**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
git add frontend/src/lib/shortcuts.ts frontend/src/lib/shortcuts.test.ts frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): global keyboard shortcuts"
```

---

## Task 12: Command palette

**Files:**
- Create: `frontend/src/lib/fuzzy.ts`
- Test: `frontend/src/lib/fuzzy.test.ts`
- Create: `frontend/src/components/CommandPalette.svelte`
- Test: `frontend/src/components/CommandPalette.test.ts`
- Modify: `frontend/src/App.svelte` (render when `ui.paletteOpen`)

**Interfaces:**
- Consumes: `ui.paletteOpen`, `library`, `openTab`, `openImport`, `toggleTheme`, `toggleSidebar`, `toggleZen`, `goHome`.
- Produces: `fuzzyScore(query: string, text: string): number | null` (null = no match; higher = better).

- [ ] **Step 1: Write the failing fuzzy test**

`frontend/src/lib/fuzzy.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { fuzzyScore } from './fuzzy';

describe('fuzzyScore', () => {
  it('matches subsequences case-insensitively', () => {
    expect(fuzzyScore('aiayn', 'Attention Is All You Need')).not.toBeNull();
    expect(fuzzyScore('xyz', 'Attention Is All You Need')).toBeNull();
  });

  it('empty query matches everything with score 0', () => {
    expect(fuzzyScore('', 'anything')).toBe(0);
  });

  it('prefers consecutive and prefix matches', () => {
    const consecutive = fuzzyScore('atten', 'Attention Is All You Need')!;
    const scattered = fuzzyScore('atn', 'Attention Is All You Need')!;
    expect(consecutive).toBeGreaterThan(scattered);
    const prefix = fuzzyScore('lora', 'LoRA: Low-Rank Adaptation')!;
    const inner = fuzzyScore('lora', 'Exploring LoRA Variants')!;
    expect(prefix).toBeGreaterThan(inner);
  });
});
```

- [ ] **Step 2: Run — expect FAIL; implement `frontend/src/lib/fuzzy.ts`**

```bash
npm --prefix frontend run test -- src/lib/fuzzy.test.ts
```

```ts
/// Subsequence fuzzy match. Returns null when `query` is not a subsequence
/// of `text` (case-insensitive); otherwise a score favoring consecutive
/// runs (+3 per adjacent hit vs +1) and a match starting at index 0 (+2).
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (q.length === 0) return 0;
  let qi = 0;
  let score = 0;
  let last = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      score += last === ti - 1 ? 3 : 1;
      if (ti === 0) score += 2;
      last = ti;
      qi++;
    }
  }
  return qi === q.length ? score : null;
}
```

- [ ] **Step 3: Run — expect PASS**

```bash
npm --prefix frontend run test -- src/lib/fuzzy.test.ts
```

- [ ] **Step 4: Write the failing palette test**

`frontend/src/components/CommandPalette.test.ts`:

```ts
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import CommandPalette from './CommandPalette.svelte';
import { library, ui, viewer } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '',
  };
}

beforeEach(() => {
  library.papers = [paper('p1', 'Attention Is All You Need'), paper('p2', 'Denoising Diffusion')];
  viewer.tabs = [];
  viewer.activeId = null;
  ui.paletteOpen = true;
});

describe('CommandPalette', () => {
  it('filters papers by fuzzy query and opens on Enter', async () => {
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'atten');
    expect(screen.getByText('Attention Is All You Need')).toBeInTheDocument();
    expect(screen.queryByText('Denoising Diffusion')).not.toBeInTheDocument();
    await userEvent.keyboard('{Enter}');
    expect(viewer.activeId).toBe('p1');
    expect(ui.paletteOpen).toBe(false);
  });

  it('lists actions and runs them', async () => {
    render(CommandPalette);
    await userEvent.type(screen.getByRole('combobox'), 'import');
    await userEvent.click(screen.getByText('Import papers…'));
    expect(ui.importOpen).toBe(true);
    expect(ui.paletteOpen).toBe(false);
  });

  it('closes on Escape', async () => {
    render(CommandPalette);
    await userEvent.keyboard('{Escape}');
    expect(ui.paletteOpen).toBe(false);
  });
});
```

- [ ] **Step 5: Run — expect FAIL; create `frontend/src/components/CommandPalette.svelte`**

```bash
npm --prefix frontend run test -- src/components/CommandPalette.test.ts
```

```svelte
<script lang="ts">
  import { ArrowRight, FileText, Search } from 'lucide-svelte';
  import { fade, fly } from 'svelte/transition';
  import { fuzzyScore } from '../lib/fuzzy';
  import { DUR, dur } from '../lib/motion';
  import {
    goHome,
    library,
    openImport,
    openTab,
    toggleSidebar,
    toggleTheme,
    toggleZen,
    ui,
    viewer,
  } from '../lib/state.svelte';
  import type { PaperSummary } from '../lib/types';

  let query = $state('');
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
  });

  function close() {
    ui.paletteOpen = false;
    query = '';
  }

  type Item =
    | { kind: 'paper'; id: string; label: string; paper: PaperSummary; score: number }
    | { kind: 'action'; id: string; label: string; run: () => void; score: number };

  const ACTIONS: Array<{ id: string; label: string; run: () => void }> = [
    { id: 'import', label: 'Import papers…', run: () => openImport() },
    { id: 'home', label: 'Go to library', run: () => goHome() },
    { id: 'theme', label: 'Cycle theme', run: () => toggleTheme() },
    { id: 'pane', label: 'Toggle list pane', run: () => toggleSidebar() },
    { id: 'zen', label: 'Toggle zen mode', run: () => toggleZen() },
  ];

  const items = $derived.by((): Item[] => {
    const q = query.trim();
    const papers: Item[] = library.papers
      .map((p) => ({
        p,
        score: fuzzyScore(q, `${p.title ?? ''} ${p.authors.join(' ')} ${p.cite_key ?? ''}`),
      }))
      .filter((x): x is { p: PaperSummary; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, 8)
      .map(({ p, score }) => ({
        kind: 'paper' as const,
        id: `paper-${p.id}`,
        label: p.title ?? '(untitled)',
        paper: p,
        score,
      }));
    const actions: Item[] = ACTIONS.map((a) => ({
      kind: 'action' as const,
      ...a,
      score: fuzzyScore(q, a.label) ?? -1,
    })).filter((a) => a.score >= 0);
    // With no query: actions first (verbs), then recent papers. With a
    // query: best matches first regardless of kind.
    return q ? [...papers, ...actions].sort((a, b) => b.score - a.score) : [...actions, ...papers];
  });

  $effect(() => {
    void items;
    active = 0;
  });

  function run(item: Item) {
    close();
    if (item.kind === 'paper') openTab(item.paper);
    else item.run();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(items.length - 1, active + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(0, active - 1);
    } else if (e.key === 'Enter' && items[active]) {
      e.preventDefault();
      run(items[active]);
    }
  }
</script>

<div
  transition:fade={{ duration: dur(DUR.fast) }}
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
  class="fixed inset-0 z-[60] flex items-start justify-center bg-stone-950/40 p-4 pt-[12vh] backdrop-blur-[2px]"
>
  <div
    transition:fly={{ y: -12, duration: dur(DUR.base) }}
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    class="w-full max-w-lg overflow-hidden rounded-xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    <div class="flex items-center gap-2 border-b border-stone-200 px-3 dark:border-stone-800">
      <Search size={16} class="shrink-0 text-stone-400" />
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onkeydown}
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-label="Search papers and actions"
        placeholder="Type a paper title or a command…"
        class="w-full bg-transparent py-3 text-sm text-ink outline-none dark:text-stone-100"
      />
    </div>
    <ul id="palette-list" role="listbox" class="max-h-80 overflow-y-auto p-1">
      {#if items.length === 0}
        <li class="px-3 py-4 text-sm text-stone-500 dark:text-stone-400">
          Nothing matches. Try fewer letters.
        </li>
      {/if}
      {#each items as item, i (item.id)}
        <li role="option" aria-selected={i === active}>
          <button
            type="button"
            onclick={() => run(item)}
            onmouseenter={() => (active = i)}
            class={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm ${
              i === active
                ? 'bg-amber-700/10 text-ink dark:bg-amber-500/10 dark:text-stone-100'
                : 'text-stone-600 dark:text-stone-300'
            }`}
          >
            {#if item.kind === 'paper'}
              <FileText size={14} class="shrink-0 text-stone-400" />
              <span class="min-w-0 flex-1 truncate font-serif">{item.label}</span>
            {:else}
              <ArrowRight size={14} class="shrink-0 text-stone-400" />
              <span class="min-w-0 flex-1 truncate">{item.label}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </div>
</div>
```

- [ ] **Step 6: Render in `frontend/src/App.svelte`**

Add `import CommandPalette from './components/CommandPalette.svelte';` and, next to the modals:

```svelte
{#if ui.paletteOpen}<CommandPalette />{/if}
```

- [ ] **Step 7: Run everything — expect PASS**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
```

- [ ] **Step 8: Commit**

```bash
git add frontend/src/lib/fuzzy.ts frontend/src/lib/fuzzy.test.ts frontend/src/components/CommandPalette.svelte frontend/src/components/CommandPalette.test.ts frontend/src/App.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): command palette with fuzzy paper jump and actions"
```

---

## Task 13: Theme view-transition, StatusPill retint, polish & audit

**Files:**
- Modify: `frontend/src/lib/state.svelte.ts` (`toggleTheme`)
- Modify: `frontend/src/components/StatusPill.svelte`
- Verify: whole app

**Interfaces:**
- Consumes: `dur` from motion (view-transition gate).

- [ ] **Step 1: Retint `frontend/src/components/StatusPill.svelte`** (last emerald/amber-status holdout)

```svelte
<script lang="ts">
  let { status }: { status: string } = $props();
  const resolved = $derived(status === 'resolved');
</script>

<span
  class={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
    resolved
      ? 'bg-lime-100 text-lime-800 dark:bg-lime-500/15 dark:text-lime-300'
      : 'bg-yellow-100 text-yellow-800 dark:bg-yellow-500/15 dark:text-yellow-300'
  }`}
>
  {resolved ? 'resolved' : 'needs review'}
</span>
```

`StatusPill.test.ts` asserts text only — still green.

- [ ] **Step 2: Soft-crossfade theme switches via the View Transitions API**

In `frontend/src/lib/state.svelte.ts`, add `import { dur } from './motion';` and replace `toggleTheme`:

```ts
const THEME_CYCLE: ThemeMode[] = ['light', 'dark', 'system'];
export function toggleTheme(): void {
  theme.mode = THEME_CYCLE[(THEME_CYCLE.indexOf(theme.mode) + 1) % THEME_CYCLE.length];
  localStorage.setItem('xuewen-theme', theme.mode);
  // Crossfade the whole page where the View Transitions API exists; fall
  // back to an instant swap (also under reduced motion / tests via dur).
  const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
  if (doc.startViewTransition && dur(1) > 0) {
    doc.startViewTransition(() => applyTheme());
  } else {
    applyTheme();
  }
}
```

(`theme.test.ts` stays green: jsdom has no `startViewTransition`, so the fallback runs synchronously as before.)

- [ ] **Step 3: Sweep for stragglers**

```bash
grep -rn "slate-\|indigo-\|emerald-" frontend/src --include='*.svelte' --include='*.ts'
grep -rn "duration: [0-9]" frontend/src --include='*.svelte' | grep -v "dur("
```

Expected: no hits. Any hit is an unported class or a hardcoded duration — fix it with the palette table / `dur()`.

- [ ] **Step 4: Full verification**

```bash
npm --prefix frontend run test
npm --prefix frontend run check
npm --prefix frontend run build
cargo build
```

Expected: all green; `cargo build` re-embeds `frontend/dist` without complaint.

- [ ] **Step 5: Manual QA checklist** (against `xuewen serve`, `npm --prefix frontend run dev`)

- Browse: select papers with mouse and `j`/`k`; DetailView sections stagger in; abstract set in serif.
- Read: double-click and Enter open tabs; underline slides between tabs and Library; `x` closes; PDFs don't reload after visiting Library.
- Zen: `z` in a tab hides all chrome, pill drops in, edge-hover peeks the list, `Esc` exits, closing the last tab exits.
- Pane: `[` springs it closed/open; edge peek works while closed.
- Search: type → keyword pass then semantic; snippets highlighted yellow; options popover toggles fields/engines; export disabled during a query.
- Import/Identify/Projects: dialogs scale+fade in, Esc and backdrop close, focus returns to the trigger; all flows work as before; ProjectsModal labels unchanged.
- Palette: ⌘K everywhere (including inside inputs); fuzzy jump opens the PDF; actions run.
- Toasts: copy cite → "Citation copied"; delete → "Paper deleted"; failure hints stay inline.
- Themes: light/dark/system all readable — warm paper light, warm near-black dark; toggle crossfades (Chromium) or swaps cleanly (Firefox).
- Reduced motion: with OS reduce-motion on, everything appears instantly, nothing animates.
- Seal discipline: cinnabar appears exactly twice (TopBar wordmark, Welcome).

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/state.svelte.ts frontend/src/components/StatusPill.svelte
git -c commit.gpgsign=false commit -m "feat(frontend): warm status tints and view-transition theme crossfade"
```

---

## Plan Self-Review (done at authoring time)

1. **Spec coverage:** layout (Tasks 5–9), components table (4–10, 12), state & shortcuts (2, 11 — with the documented `x`-for-⌘W deviation), animation catalog (1, 5, 7–10, 12, 13 — with crossfade-on-open narrowed to the tab underline: a list row never leaves the DOM when a tab opens, so a literal row→tab crossfade pair cannot fire; the underline crossfade + row open-dot carry the handoff), visual tokens (1, 13), errors (unchanged patterns + toast rules in 3, 4), testing (every task). Non-goals respected: no backend edits, no Daily UI, one font dependency.
2. **Placeholder scan:** the three modal-port snippets in Task 10 intentionally say "verbatim" for bodies that already exist in the repo — the exact class substitutions are enumerated; everything new is fully written out.
3. **Type consistency:** `dur`/`DUR`/`SPRINGS` (Task 1) used identically in 3, 5–13; `selection`/`selectPaper`/`goHome`/`toggleZen` (Task 2) match usage in 5–7, 9, 11, 12; `toast` signature (3) matches 4; `Modal` snippet props (10) match all three ports; `fuzzyScore` (12) single definition.
