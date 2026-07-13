import type { Reference } from './citations';

/** Lowercase, replace every non-alphanumeric char with a space, collapse ws.
 *  Mirrors `normalize_title` in src/matching.rs so both ends agree. */
export function normalizeTitle(s: string): string {
  return s
    .replace(/[^a-zA-Z0-9]+/g, ' ')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, ' ');
}

// Minimum normalized-title length (chars) to attempt a substring match. Short
// titles ("On It") appear by chance in prose and cause false positives.
const MIN_TITLE_LEN = 12;

/**
 * For each reference, the id of the first library paper whose normalized title
 * occurs verbatim inside the normalized reference text. References almost always
 * contain the cited paper's exact title, so substring containment is reliable
 * and needs no reference parsing.
 */
export function matchReferences<P extends { id: string; title: string | null }>(
  refs: Reference[],
  papers: P[],
): Map<number, P> {
  const normed = papers
    .filter((p) => !!p.title)
    .map((p) => ({ paper: p, title: normalizeTitle(p.title as string) }))
    .filter((p) => p.title.length >= MIN_TITLE_LEN);

  const out = new Map<number, P>();
  for (const r of refs) {
    const hay = normalizeTitle(r.rawText);
    const hit = normed.find((p) => hay.includes(p.title));
    if (hit) out.set(r.index, hit.paper);
  }
  return out;
}
