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
  destY: number; // destination y in the same top-left space
}
export interface RefAnchor { pageIndex: number; y: number; }

// A run is the References heading if, once stripped of punctuation/casing, it
// is exactly one of these words. Matching the whole run (not a substring)
// avoids "see references" false positives.
const HEADING_RE = /^(references|bibliography|works cited)$/;

function normalizeHeading(s: string): string {
  return s.replace(/[^a-zA-Z ]/g, '').trim().toLowerCase();
}

/**
 * The first run (in reading order: page ascending, then y ascending) whose text
 * is exactly a references heading. Returns its page + y, or null.
 */
export function findReferencesStart(pages: PageText[]): RefAnchor | null {
  const ordered = [...pages].sort((a, b) => a.pageIndex - b.pageIndex);
  for (const p of ordered) {
    const runs = [...p.runs].sort((a, b) => a.y - b.y);
    for (const r of runs) {
      if (HEADING_RE.test(normalizeHeading(r.text))) {
        return { pageIndex: p.pageIndex, y: r.y };
      }
    }
  }
  return null;
}
