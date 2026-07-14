export interface TextRun { text: string; x: number; y: number; width: number; height: number; }
export interface UrlLink { x: number; y: number; width: number; height: number; url: string; }
export interface PageText {
  pageIndex: number;
  width: number;
  height: number;
  runs: TextRun[];
  urlLinks: UrlLink[];
}
/** An internal GoTo link annotation: a citation marker (source) with a destination. */
export interface GotoLink {
  pageIndex: number; // page the marker sits on
  x: number; y: number; width: number; height: number; // marker rect (top-left, PDF points)
  destPageIndex: number;
  destY: number; // destination y in the same top-left space (0 = page top when unknown)
  destX: number; // destination x in PDF points (0 when unknown) — column assignment
}
export interface RefAnchor { pageIndex: number; y: number; x: number; }

// A line is a references heading if, with every non-letter removed, it is one
// of these tokens — optionally preceded by a small roman-numeral section
// number ("VII. References"; arabic numbers are digits and vanish with the
// non-letters). The prefix must be a WELL-FORMED numeral in 1..39
// (x{0,3}(ix|iv|v?i{0,3})) so ordinary words spelled from roman letters
// ("Mild", "Civil", appendix label "D") cannot smuggle a match. Whole-line
// anchoring avoids "see the references section" false positives; letters-only
// comparison keeps tolerance for headings split across runs ("R"+"EFERENCES").
const HEADING_TOKENS = ['references', 'bibliography', 'workscited', 'referencesandnotes', 'referencescited'];
const HEADING_RE = new RegExp(`^(?:x{0,3}(?:ix|iv|v?i{0,3}))?(?:${HEADING_TOKENS.join('|')})$`);

// Runs whose y is within this many PDF points share a visual line (same baseline).
export const LINE_TOLERANCE = 3;

export function isReferencesHeading(lineText: string): boolean {
  return HEADING_RE.test(lineText.replace(/[^a-zA-Z]/g, '').toLowerCase());
}

/** Group a page's runs into visual lines: text concatenated in reading (x)
 *  order, tagged with the line's top y. */
function pageLines(page: PageText): { y: number; x: number; text: string }[] {
  const rows = new Map<number, TextRun[]>();
  for (const r of page.runs) {
    const key = Math.round(r.y / LINE_TOLERANCE);
    const arr = rows.get(key) ?? [];
    arr.push(r);
    rows.set(key, arr);
  }
  return [...rows.values()].map((rs) => {
    rs.sort((a, b) => a.x - b.x);
    return {
      y: Math.min(...rs.map((r) => r.y)),
      x: Math.min(...rs.map((r) => r.x)),
      text: rs.map((r) => r.text).join(''),
    };
  });
}

/**
 * The first line (in reading order: page ascending, then y ascending) that is
 * exactly a references heading. Returns its page + y, or null. Reconstructs
 * lines from runs so a styled heading split across runs is still detected.
 */
export function findReferencesStart(pages: PageText[]): RefAnchor | null {
  const ordered = [...pages].sort((a, b) => a.pageIndex - b.pageIndex);
  for (const p of ordered) {
    const lines = pageLines(p).sort((a, b) => a.y - b.y);
    for (const line of lines) {
      if (isReferencesHeading(line.text)) {
        return { pageIndex: p.pageIndex, y: line.y, x: line.x };
      }
    }
  }
  return null;
}

/** Assign each run to a column (0 = left/only, 1 = right). A page is
 *  two-column when both halves are populated and almost nothing straddles the
 *  midline (full-width headers/captions are tolerated up to 15%). */
export function assignColumns(runs: TextRun[], pageWidth: number): Map<TextRun, number> {
  const mid = pageWidth / 2;
  const straddling = runs.filter((r) => r.x < mid - 6 && r.x + r.width > mid + 6).length;
  const hasLeft = runs.some((r) => r.x + r.width <= mid);
  const hasRight = runs.some((r) => r.x >= mid);
  const twoCol = hasLeft && hasRight && straddling <= runs.length * 0.15;
  const out = new Map<TextRun, number>();
  for (const r of runs) out.set(r, twoCol && r.x + r.width / 2 >= mid ? 1 : 0);
  return out;
}

export interface Reference {
  index: number;
  destPageIndex: number;
  destY: number;
  rawText: string;
  externalUrl?: string;
}
export interface Marker {
  pageIndex: number; x: number; y: number; width: number; height: number;
  refIndex: number;
}
export interface CitationData { references: Reference[]; markers: Marker[]; }

// Two destinations are the same reference if in the same page+column within
// this many PDF points vertically (one line of slack).
const DEST_EPSILON = 6;

interface CmPos { col: number; y: number; x: number; }
const cmCompare = (a: CmPos, b: CmPos) => a.col - b.col || a.y - b.y || a.x - b.x;

export function buildCitationData(links: GotoLink[], pages: PageText[], refStart: RefAnchor): CitationData {
  // Per-page column assignment + column-major (reading-order) run list.
  const layout = new Map<number, { twoCol: boolean; mid: number; ordered: { run: TextRun; pos: CmPos }[] }>();
  for (const p of pages) {
    const cols = assignColumns(p.runs, p.width);
    const twoCol = [...cols.values()].some((c) => c === 1);
    const ordered = p.runs
      .map((run) => ({ run, pos: { col: cols.get(run) ?? 0, y: run.y, x: run.x } }))
      .sort((a, b) => cmCompare(a.pos, b.pos));
    layout.set(p.pageIndex, { twoCol, mid: p.width / 2, ordered });
  }
  const colOf = (pageIndex: number, x: number): number => {
    const l = layout.get(pageIndex);
    return l?.twoCol && x >= l.mid ? 1 : 0;
  };

  // A destination is a citation target if it reads AFTER the heading:
  // later page, later column, or same column at/after the heading y.
  const startPos: CmPos = { col: colOf(refStart.pageIndex, refStart.x), y: refStart.y - DEST_EPSILON, x: 0 };
  const isAfterStart = (pageIndex: number, pos: CmPos) =>
    pageIndex > refStart.pageIndex || (pageIndex === refStart.pageIndex && cmCompare(pos, startPos) >= 0);

  const posOfLink = (l: GotoLink): CmPos => ({ col: colOf(l.destPageIndex, l.destX), y: l.destY, x: 0 });
  const citeLinks = links.filter((l) => isAfterStart(l.destPageIndex, posOfLink(l)));

  // Distinct destinations = reference anchors, deduped by (page, column, y±ε),
  // in reading order.
  const anchors: { destPageIndex: number; pos: CmPos }[] = [];
  for (const l of citeLinks) {
    const pos = posOfLink(l);
    const hit = anchors.find(
      (a) => a.destPageIndex === l.destPageIndex && a.pos.col === pos.col && Math.abs(a.pos.y - pos.y) <= DEST_EPSILON,
    );
    if (!hit) anchors.push({ destPageIndex: l.destPageIndex, pos });
  }
  anchors.sort((a, b) => a.destPageIndex - b.destPageIndex || cmCompare(a.pos, b.pos));

  // Raw text: the column-major run band from this anchor to the next anchor
  // on the same page (flows across the column break), else to page end.
  const references: Reference[] = anchors.map((a, i) => {
    const next = anchors[i + 1];
    const l = layout.get(a.destPageIndex);
    const from: CmPos = { col: a.pos.col, y: a.pos.y - DEST_EPSILON, x: 0 };
    const to: CmPos | null =
      next && next.destPageIndex === a.destPageIndex
        ? { col: next.pos.col, y: next.pos.y - DEST_EPSILON, x: 0 }
        : null;
    const band = (l?.ordered ?? []).filter(
      ({ pos }) => cmCompare(pos, from) >= 0 && (to === null || cmCompare(pos, to) < 0),
    );
    const rawText = band.map(({ run }) => run.text).join(' ').replace(/\s+/g, ' ').trim();
    const p = pages.find((pp) => pp.pageIndex === a.destPageIndex);
    const externalUrl = (p?.urlLinks ?? []).find((u) => {
      const pos: CmPos = { col: colOf(a.destPageIndex, u.x), y: u.y, x: u.x };
      return cmCompare(pos, from) >= 0 && (to === null || cmCompare(pos, to) < 0);
    })?.url;
    return { index: i, destPageIndex: a.destPageIndex, destY: a.pos.y, rawText, externalUrl };
  });

  // Map each cite link (marker) to its reference.
  const markers: Marker[] = [];
  for (const l of citeLinks) {
    const pos = posOfLink(l);
    const ref = references.find(
      (r, i) =>
        anchors[i].destPageIndex === l.destPageIndex &&
        anchors[i].pos.col === pos.col &&
        Math.abs(anchors[i].pos.y - pos.y) <= DEST_EPSILON,
    );
    if (!ref) continue;
    markers.push({ pageIndex: l.pageIndex, x: l.x, y: l.y, width: l.width, height: l.height, refIndex: ref.index });
  }

  return { references, markers };
}
