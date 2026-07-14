import { describe, expect, it } from 'vitest';
import { authorLine, refLinks } from './refFormat';
import type { StructuredReference } from './types';

const base: StructuredReference = {
  authors: [], title: null, venue: null, year: null, doi: null, arxiv_id: null, url: null,
};

describe('authorLine', () => {
  it('shows one or two names verbatim', () => {
    expect(authorLine(['A. One'])).toBe('A. One');
    expect(authorLine(['A. One', 'B. Two'])).toBe('A. One, B. Two');
  });
  it('collapses three or more to first and last', () => {
    expect(authorLine(['A. One', 'B. Two', 'C. Three'])).toBe('A. One, …, C. Three');
    expect(authorLine(['A', 'B', 'C', 'D'])).toBe('A, …, D');
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
  it('drops non-http(s) and unparseable hrefs (javascript:, data:, garbage)', () => {
    expect(refLinks({ ...base, url: 'javascript:alert(1)' })).toEqual([]);
    expect(refLinks(null, 'data:text/html,<script>alert(1)</script>')).toEqual([]);
    expect(refLinks({ ...base, url: 'not a url' })).toEqual([]);
    expect(refLinks({ ...base, url: 'https://ok.example/x' })).toEqual([{ label: 'ok.example', href: 'https://ok.example/x' }]);
  });
});
