import type { StructuredReference } from './types';

/** Words kept lowercase in title case (articles, conjunctions, short
 *  prepositions — AP-style), unless first/last or after a colon/period. */
const SMALL_WORDS = new Set([
  'a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'so', 'yet',
  'as', 'at', 'by', 'in', 'of', 'on', 'per', 'to', 'up', 'via', 'vs',
]);

/** Display title case for reference titles that arrive in sentence case.
 *  Only fully-lowercase words are touched — acronyms (PGFUZZ), identifiers
 *  (GPT-4), and mixed-case words (eBPF, McMahan, iOS) pass through — so a
 *  title that already carries intentional casing is never mangled. */
export function titleCase(title: string): string {
  const words = title.split(' ');
  const last = words.length - 1;
  let afterBreak = false; // previous word ended a clause (colon/period/question)
  return words
    .map((word, i) => {
      const force = i === 0 || i === last || afterBreak;
      afterBreak = /[:.?!]$/.test(word);
      // Hyphen parts are words too: "policy-guided" → "Policy-Guided",
      // "state-of-the-art" → "State-of-the-Art".
      return word
        .split('-')
        .map((part, j) => {
          if (!/^[a-z]/.test(part) || /[A-Z0-9]/.test(part)) return part; // not fully lowercase
          if (!(force && j === 0) && SMALL_WORDS.has(part.replace(/[^a-z]/g, ''))) return part;
          return part.charAt(0).toUpperCase() + part.slice(1);
        })
        .join('-');
    })
    .join(' ');
}

/** One or two authors verbatim; three or more collapse to "First, …, Last". */
export function authorLine(authors: string[]): string {
  if (authors.length <= 2) return authors.join(', ');
  return `${authors[0]}, …, ${authors[authors.length - 1]}`;
}

/** External links for a reference: structured DOI/arXiv/URL first, then the
 *  raw in-PDF link, deduped by href, at most two. */
export function refLinks(
  s: StructuredReference | null | undefined,
  externalUrl?: string,
): { label: string; href: string }[] {
  const out: { label: string; href: string }[] = [];
  const push = (label: string, href: string) => {
    // Only web URLs may become links: `url` comes from LLM output (prompt
    // contains raw PDF text) and `externalUrl` from raw PDF /URI actions —
    // a javascript:/data: href here would run in the app origin.
    try {
      const proto = new URL(href).protocol;
      if (proto !== 'http:' && proto !== 'https:') return;
    } catch {
      return; // unparseable → no link
    }
    if (!out.some((l) => l.href === href)) out.push({ label, href });
  };
  if (s?.doi) push('doi.org', `https://doi.org/${s.doi}`);
  if (s?.arxiv_id) push('arXiv', `https://arxiv.org/abs/${s.arxiv_id}`);
  if (s?.url) push(hostOf(s.url), s.url);
  if (externalUrl) push(hostOf(externalUrl), externalUrl);
  return out.slice(0, 2);
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
