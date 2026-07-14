import type { StructuredReference } from './types';

/** "A, B, C" for up to three authors, then "A, B, C et al." */
export function authorLine(authors: string[]): string {
  const shown = authors.slice(0, 3).join(', ');
  return authors.length > 3 ? `${shown} et al.` : shown;
}

/** External links for a reference: structured DOI/arXiv/URL first, then the
 *  raw in-PDF link, deduped by href, at most two. */
export function refLinks(
  s: StructuredReference | null | undefined,
  externalUrl?: string,
): { label: string; href: string }[] {
  const out: { label: string; href: string }[] = [];
  const push = (label: string, href: string) => {
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
