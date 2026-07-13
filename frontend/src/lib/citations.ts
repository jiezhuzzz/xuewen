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

// Two destinations are the same reference if on the same page within this many
// PDF points vertically (one line of slack).
const DEST_EPSILON = 6;

function isAfter(a: RefAnchor, pageIndex: number, y: number): boolean {
  return pageIndex > a.pageIndex || (pageIndex === a.pageIndex && y >= a.y - DEST_EPSILON);
}

export function buildCitationData(links: GotoLink[], pages: PageText[], refStart: RefAnchor): CitationData {
  const pageByIndex = new Map(pages.map((p) => [p.pageIndex, p]));

  // 1. Keep only links whose destination is at/after the references start.
  const citeLinks = links.filter((l) => isAfter(refStart, l.destPageIndex, l.destY));

  // 2. Collect distinct destinations = the reference anchors, in reading order.
  const anchors: { destPageIndex: number; destY: number }[] = [];
  for (const l of citeLinks) {
    const hit = anchors.find(
      (a) => a.destPageIndex === l.destPageIndex && Math.abs(a.destY - l.destY) <= DEST_EPSILON,
    );
    if (!hit) anchors.push({ destPageIndex: l.destPageIndex, destY: l.destY });
  }
  anchors.sort((a, b) => a.destPageIndex - b.destPageIndex || a.destY - b.destY);

  // 3. Build a Reference per anchor: raw text = runs from this anchor's y down to
  //    the next anchor on the same page (or page end); external URL = first
  //    urlLink whose rect falls in that band.
  const references: Reference[] = anchors.map((a, i) => {
    const next = anchors[i + 1];
    const yEnd = next && next.destPageIndex === a.destPageIndex ? next.destY : Infinity;
    const p = pageByIndex.get(a.destPageIndex);
    const inBand = (y: number) => y >= a.destY - DEST_EPSILON && y < yEnd - DEST_EPSILON;
    const rawText = (p?.runs ?? [])
      .filter((r) => inBand(r.y))
      .sort((r1, r2) => r1.y - r2.y || r1.x - r2.x)
      .map((r) => r.text)
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim();
    const externalUrl = (p?.urlLinks ?? []).find((u) => inBand(u.y))?.url;
    return { index: i, destPageIndex: a.destPageIndex, destY: a.destY, rawText, externalUrl };
  });

  // 4. Map each cite link (marker) to the reference index sharing its destination.
  const refIndexOf = (destPageIndex: number, destY: number) =>
    references.find(
      (r) => r.destPageIndex === destPageIndex && Math.abs(r.destY - destY) <= DEST_EPSILON,
    )?.index;
  const markers: Marker[] = [];
  for (const l of citeLinks) {
    const refIndex = refIndexOf(l.destPageIndex, l.destY);
    if (refIndex === undefined) continue;
    markers.push({ pageIndex: l.pageIndex, x: l.x, y: l.y, width: l.width, height: l.height, refIndex });
  }

  return { references, markers };
}
