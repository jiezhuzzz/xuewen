import { describe, expect, it } from 'vitest';
import { authorLine, refLinks } from './refFormat';
import type { StructuredReference } from './types';

const base: StructuredReference = {
  authors: [], title: null, venue: null, year: null, doi: null, arxiv_id: null, url: null,
};

describe('authorLine', () => {
  it('joins up to three names', () => {
    expect(authorLine(['A. One', 'B. Two', 'C. Three'])).toBe('A. One, B. Two, C. Three');
  });
  it('truncates with et al.', () => {
    expect(authorLine(['A', 'B', 'C', 'D'])).toBe('A, B, C et al.');
  });
  it('empty list gives empty string', () => {
    expect(authorLine([])).toBe('');
  });
});

describe('refLinks', () => {
  it('prefers DOI, then arXiv, dedupes by href, caps at 2', () => {
    const links = refLinks({ ...base, doi: '10.1/x', arxiv_id: '1412.6980', url: 'https://a.b/c' });
    expect(links).toEqual([
      { label: 'doi.org', href: 'https://doi.org/10.1/x' },
      { label: 'arXiv', href: 'https://arxiv.org/abs/1412.6980' },
    ]);
  });
  it('falls back to the raw externalUrl when unparsed', () => {
    expect(refLinks(null, 'https://doi.org/10.1/x')).toEqual([
      { label: 'doi.org', href: 'https://doi.org/10.1/x' },
    ]);
  });
  it('dedupes structured DOI against the same externalUrl', () => {
    const links = refLinks({ ...base, doi: '10.1/x' }, 'https://doi.org/10.1/x');
    expect(links).toHaveLength(1);
  });
});
