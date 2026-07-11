# Design: Web UI Refactor — Paper & Ink

**Date:** 2026-07-10
**Status:** Implemented (merged to main); amendments below record where the
implementation deviates from the body text.

## Amendments (post-implementation)

- **Close-tab key is `x`, not `⌘W`** — browsers reserve ⌘W/Ctrl+W for
  closing the browser tab; it cannot be intercepted.
- **"Open a paper" animation** — a list row never leaves the DOM when its
  tab opens, so the planned row→tab crossfade pair cannot fire. Shipped
  instead: a crossfading active-tab underline that slides between tabs,
  plus an open-indicator dot on the row.
- **`/` exits zen and opens the list pane before focusing search** — the
  pane subtree is `inert` while hidden (collapsed or zen), so focusing
  requires revealing it first.
- **`Enter` is guarded from focused controls** — Enter on a button/link
  activates that control only; it does not also open the selected paper.
- **Palette results do not FLIP as the query narrows** — dropped during
  planning; the list re-renders without reorder animation.
- **Known limitation:** keyboard shortcuts do not fire while focus is
  inside the PDF iframe (inherent iframe event isolation). Clicking any
  app chrome restores them.

## Overview

A thorough refactor of the Svelte frontend: a clearer two-pane layout, a warm
editorial visual identity ("Paper & Ink"), and a coherent animation language
built entirely on Svelte 5 built-ins. Frontend-only — the Rust backend and its
JSON API do not change.

The current UI has three structural problems this design removes:

1. **Browsing lives in two inconsistent places** — a cramped `w-80` sidebar
   list, and a card grid that only appears while no PDF tab is open.
2. **The sidebar header is overloaded** — search input, six filter chips,
   three dropdowns, and an export button stacked above the list.
3. **Almost nothing animates** — modals pop in, panes snap, list changes
   jump — so the UI reads as unfinished rather than calm.

## Goals

- One obvious home for browsing and one for reading, with the paper list
  reachable at all times (no mode switching in the find → open → read loop).
- A distinctive, content-appropriate visual identity: warm surfaces, ink
  text, serif titles.
- Fluent, consistent motion with shared tokens, on Svelte built-ins only.
- Small interaction additions that serve clarity: command palette, keyboard
  shortcuts, toasts, zen mode.
- Existing functionality preserved: search (fields/engines), filters,
  projects, import (files + URLs + proxy cookie), identify, cite/export,
  delete, theme switching.

## Non-goals

- Backend or API changes of any kind.
- A Daily arXiv page in this frontend (`/api/daily` remains Glance-only).
- Future views — paper-relationship graph, insights dashboard, reading
  queue, author explorer. The layout must not carry chrome for them; when
  one lands it joins as a small top-bar switcher or command-palette
  destination.
- New runtime dependencies beyond one variable font package
  (`@fontsource-variable/source-serif-4`). No animation libraries.
- Mobile-specific layouts (desktop-first, as today).

## Decisions settled during brainstorming

- **Layout:** two-pane workspace (list + view). A mode-based icon-rail
  design was rejected because the find→open→read loop would require a mode
  switch per paper; a persistent three-pane mail layout was rejected
  because the reader never gets the full window. Zen mode covers the
  full-window reading case explicitly.
- **Style:** "Paper & Ink" over refined-indigo and neutral-graphite
  alternatives.
- **Motion:** Svelte built-ins (`svelte/transition`, `svelte/animate`,
  `svelte/motion`) plus the View Transitions API. No motion library.
- **Scope:** refactor plus command palette, keyboard shortcuts, toasts, zen
  mode. Daily arXiv view dropped from scope.

## Layout

```
┌──────────────────────────────────────────────────────────────┐
│ TopBar: 𝑿𝒖𝒆𝒘𝒆𝒏 · stats        Import · theme · ⌘K hint      │
├───────────────┬──────────────────────────────────────────────┤
│ LibraryPane   │ Content pane                                 │
│  search       │                                              │
│  (chips in    │  Browsing (no tabs): DetailView — full-width │
│   popover)    │  serif title, authors, venue, abstract,      │
│  status·sort· │  links, projects, cite actions. Nothing      │
│  project row  │  selected → quiet welcome panel.             │
│  ┌─────────┐  │                                              │
│  │ papers  │  │  Reading (tabs open): TabBar + PDF iframe    │
│  │ list    │  │  + toggleable InfoPanel (same metadata       │
│  └─────────┘  │  components as DetailView).                  │
│  export .bib  │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

- **TopBar** (slim): wordmark, `total / resolved / needs review` stats,
  Import button, theme toggle, `⌘K` hint. Nothing else.
- **LibraryPane** (~300px, collapsible): search input; the current six
  chips (title/authors/abstract/body + keyword/semantic) consolidate into
  a compact search-options popover attached to the input; one filter row
  (status · sort · project + manage-projects); the paper list; export
  pinned at the bottom. Collapse via button and `[`; when collapsed,
  hovering the left edge peeks the pane as an overlay.
- **Selection vs. opening:** single-click a paper row selects it
  (DetailView shows it); opening the PDF is an explicit action
  (double-click, Enter, or an "Open PDF" button in the DetailView) that
  creates/activates a tab.
- **Zen mode:** toggle in the tab strip, or the `z` key while a tab is
  active. Hides LibraryPane and TopBar; a floating pill shows the active
  paper title and an exit control; `Esc` exits; the edge-peek overlay
  still works so the next paper is reachable without leaving zen.

## Components

| Component | Fate | Notes |
|---|---|---|
| `App.svelte` | rework | Two-pane shell; owns zen/palette wiring |
| `TopBar.svelte` | rework | Slimmer; hides in zen |
| `Sidebar.svelte` | replace | Becomes `LibraryPane.svelte` composed of `SearchBox`, `FilterRow`, `PaperList`, `PaperRow` |
| `PaperRow.svelte` | restyle | Serif title, selection vs. active states, snippet rendering kept |
| `EmptyState.svelte` | delete | Replaced by DetailView/welcome panel |
| `DetailView.svelte` | new | Full-width paper detail for browsing state |
| `InfoPanel.svelte` | slim down | Reuses shared `PaperMeta`, `CiteActions`, `ProjectTags` subcomponents with DetailView |
| `TabBar.svelte` | rework | Sliding active indicator, animated close, zen toggle |
| `PdfViewer.svelte` | keep | iframe-per-tab approach unchanged |
| `Modal.svelte` | new | Shared animated dialog wrapper (backdrop, Esc, focus trap) |
| `ImportModal` / `IdentifyModal` / `ProjectsModal` | restyle | Rebuilt on `Modal`, logic unchanged |
| `CommandPalette.svelte` | new | Fuzzy jump-to-paper over the loaded list + actions (import, export, theme, zen) |
| `Toaster.svelte` / toast store | new | Transient action feedback (copied, imported, deleted) |
| `ZenPill.svelte` | new | Floating title + exit control in zen |

## State & interactions

`lib/state.svelte.ts` additions (existing stores keep their shape):

- `ui.zen: boolean`, `ui.paletteOpen: boolean`, `ui.sidebarOpen` retained
  for the pane collapse.
- `selection.selectedId: string | null` — browsing selection, distinct
  from `viewer.activeId`.
- `toasts` store: `{ id, kind: 'success' | 'error' | 'info', message }[]`
  with auto-dismiss.

New `lib/shortcuts.ts` — one keydown listener, inert while focus is in an
input/textarea/select (except `Esc`):

| Key | Action |
|---|---|
| `/` | focus search |
| `⌘K` / `Ctrl+K` | command palette |
| `[` | collapse/expand list pane |
| `z` (when a tab is active) | toggle zen mode |
| `Esc` | close palette/modal, exit zen |
| `⌘W` / `Ctrl+W` (when a tab is active) | close tab |
| `j` / `k` | move list selection |
| `Enter` | open selected paper |

Toasts replace transient inline feedback; persistent failures (load
errors, delete errors) stay inline where they occur today.

## Animation language

One `lib/motion.ts` module exports the shared tokens so every animation
uses the same vocabulary:

- Durations: `fast = 150ms`, `base = 250ms`, `slow = 400ms`.
- Easing: one standard ease-out curve (`cubic-bezier(0.22, 1, 0.36, 1)`).
- Spring presets (from `svelte/motion`) for pane width and pill motion.
- A `reducedMotion` media-query check; every transition parameterizes
  through it (duration 0 when reduced motion is requested).

Catalog:

| Moment | Treatment |
|---|---|
| List pane collapse/expand | Spring-driven width; content fades; edge-peek slides an overlay |
| Zen enter/exit | Pane + TopBar choreographed together; ZenPill flies in |
| Open a paper | `crossfade` pair from list row to tab strip; PDF fades in |
| Search/filter list changes | `animate:flip` reorder + staggered fade-in of new rows |
| Detail view paper switch | Sections stagger-fade |
| Modals | Scale + fade with backdrop blur (shared in `Modal.svelte`) |
| Command palette | Drop from top, results FLIP as the query narrows |
| Tab close | Width collapses out; sliding active indicator |
| Toasts | Fly in from bottom edge, auto-dismiss fade |
| Theme switch | View Transitions API crossfade where supported, instant otherwise |

## Visual tokens

Defined as Tailwind 4 `@theme` variables in `app.css`:

- **Light:** warm off-white surfaces (`#faf9f7` family / stone scale), ink
  text (`stone-900`), hairline `stone-200` borders, amber-700 accent.
- **Dark:** warm near-black (`stone-950`), `stone-200` text, amber-500
  accent; same class-based `.dark` mechanism and `color-scheme` handling
  as today.
- **Type:** Inter Variable for UI; Source Serif 4 Variable
  (`@fontsource-variable/source-serif-4`) for paper titles, the DetailView
  display title, and the wordmark.
- **Status:** resolved = green family, needs review = yellow family, both
  tuned to the warm palette; the needs-review yellow must read as a status
  tint (pill background), never as the accent amber used for actions.
- Radii and shadows unified (one card radius, one overlay shadow).

## Error handling

Unchanged patterns: API errors surface inline where they do today;
clipboard fallback behavior kept. Toasts are additive feedback only — no
error information moves exclusively into a toast.

## Testing

- Existing vitest suites updated for renames/restructuring; all keep
  passing (`npm test`, `npm run check` clean).
- New tests: shortcut dispatch (focus-guard behavior included), command
  palette filtering and action dispatch, toast store lifecycle, zen-mode
  state transitions, selection-vs-open behavior in `PaperRow`/`PaperList`.
- Animation code paths are exercised with reduced-motion defaults in
  jsdom; visual motion itself is not unit-tested.

## Implementation order (for the plan)

1. Tokens & shell: `app.css` theme, `motion.ts`, `App` two-pane skeleton,
   TopBar.
2. LibraryPane decomposition + DetailView + selection model.
3. Reader: TabBar rework, InfoPanel slim-down, shared meta components,
   zen mode.
4. Overlays: `Modal` wrapper, restyled modals, command palette, toasts,
   shortcuts.
5. Polish pass: staggering, crossfades, View Transitions, reduced-motion
   audit.
