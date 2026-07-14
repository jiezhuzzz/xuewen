import {
  assignColumns,
  LINE_TOLERANCE,
  type PageText,
  type RefAnchor,
  type Reference,
  type TextRun,
} from './citations';

export interface CmLine {
  pageIndex: number;
  col: number;
  y: number;
  x: number;
  text: string;
  runs: { run: TextRun; start: number; end: number }[];
}

/** Visual lines in column-major reading order, with char-offset→run mapping. */
export function columnMajorLines(page: PageText): CmLine[] {
  const cols = assignColumns(page.runs, page.width);
  const rows = new Map<string, TextRun[]>();
  for (const r of page.runs) {
    const key = `${cols.get(r)}:${Math.round(r.y / LINE_TOLERANCE)}`;
    const arr = rows.get(key) ?? [];
    arr.push(r);
    rows.set(key, arr);
  }
  const lines: CmLine[] = [];
  for (const rs of rows.values()) {
    rs.sort((a, b) => a.x - b.x);
    let text = '';
    const runs: CmLine['runs'] = [];
    for (const run of rs) {
      runs.push({ run, start: text.length, end: text.length + run.text.length });
      text += run.text;
    }
    lines.push({
      pageIndex: page.pageIndex,
      col: cols.get(rs[0]) ?? 0,
      y: Math.min(...rs.map((r) => r.y)),
      x: Math.min(...rs.map((r) => r.x)),
      text,
      runs,
    });
  }
  return lines.sort((a, b) => a.col - b.col || a.y - b.y || a.x - b.x);
}

export interface SegmentedRefs {
  references: Reference[];
  /** bib number n → reference index (numbered style only). */
  numberOf: Map<number, number>;
  style: 'numbered' | 'authoryear';
}

const MIN_ENTRIES = 2;
const NUMBERED_START = /^\s*\[(\d{1,3})\]\s*/;

/** Segment the bibliography that starts at `refStart` into entries.
 *  Numbered path: entries begin at lines starting with "[n]". */
export function segmentReferences(pages: PageText[], refStart: RefAnchor): SegmentedRefs | null {
  // All lines at/after the heading, in reading order across pages.
  const ordered = [...pages].sort((a, b) => a.pageIndex - b.pageIndex);
  const startCol = colOfPoint(pages, refStart.pageIndex, refStart.x);
  const lines: CmLine[] = [];
  for (const p of ordered) {
    for (const l of columnMajorLines(p)) {
      const after =
        p.pageIndex > refStart.pageIndex ||
        (p.pageIndex === refStart.pageIndex &&
          (l.col > startCol || (l.col === startCol && l.y >= refStart.y - LINE_TOLERANCE)));
      if (after) lines.push(l);
    }
  }

  const starts = lines
    .map((l, i) => ({ i, m: l.text.match(NUMBERED_START) }))
    .filter((s): s is { i: number; m: RegExpMatchArray } => s.m !== null);
  if (starts.length < MIN_ENTRIES) return null; // author-year path lands in Task 17

  const numberOf = new Map<number, number>();
  const references: Reference[] = starts.map((s, refIndex) => {
    const from = s.i;
    const to = refIndex + 1 < starts.length ? starts[refIndex + 1].i : lines.length;
    const span = lines.slice(from, to);
    const rawText = span.map((l) => l.text).join(' ').replace(/\s+/g, ' ').trim();
    numberOf.set(parseInt(s.m[1], 10), refIndex);
    const first = lines[from];
    const page = pages.find((p) => p.pageIndex === first.pageIndex);
    const externalUrl = page?.urlLinks.find((u) =>
      span.some((l) => l.pageIndex === page.pageIndex && Math.abs(l.y - u.y) <= LINE_TOLERANCE * 2),
    )?.url;
    return { index: refIndex, destPageIndex: first.pageIndex, destY: first.y, rawText, externalUrl };
  });

  return { references, numberOf, style: 'numbered' };
}

function colOfPoint(pages: PageText[], pageIndex: number, x: number): number {
  const p = pages.find((pp) => pp.pageIndex === pageIndex);
  if (!p) return 0;
  const cols = assignColumns(p.runs, p.width);
  const twoCol = [...cols.values()].some((c) => c === 1);
  return twoCol && x >= p.width / 2 ? 1 : 0;
}
