import { describe, expect, it } from 'vitest';
import { searchParams } from './api';
import type { Filters, SearchOpts } from './types';

const allOpts: SearchOpts = {
  title: true,
  authors: true,
  abstract: true,
  body: true,
  keyword: true,
  semantic: true,
};
const filters: Filters = { q: '', status: 'all', sort: 'year_desc', project: 'all' };

describe('searchParams', () => {
  it('omits fields/engines when everything is selected', () => {
    const p = searchParams('fuzzing', allOpts, filters);
    expect(p.get('q')).toBe('fuzzing');
    expect(p.get('fields')).toBeNull();
    expect(p.get('engines')).toBeNull();
    expect(p.get('project')).toBeNull();
    expect(p.get('status')).toBeNull();
  });

  it('lists only the selected fields and engines', () => {
    const p = searchParams(
      'x',
      { ...allOpts, title: false, abstract: false, semantic: false },
      filters,
    );
    expect(p.get('fields')).toBe('authors,body');
    expect(p.get('engines')).toBe('keyword');
  });

  it('keywordOnly overrides the engine selection', () => {
    const p = searchParams('x', allOpts, filters, true);
    expect(p.get('engines')).toBe('keyword');
  });

  it('carries status and project filters', () => {
    const p = searchParams('x', allOpts, { ...filters, status: 'resolved', project: 'proj1' });
    expect(p.get('status')).toBe('resolved');
    expect(p.get('project')).toBe('proj1');
  });

  it('treats an empty selection the same as all (UI enforces at least one)', () => {
    const none: SearchOpts = {
      title: false,
      authors: false,
      abstract: false,
      body: false,
      keyword: false,
      semantic: false,
    };
    const p = searchParams('x', none, filters);
    expect(p.get('fields')).toBeNull();
    expect(p.get('engines')).toBeNull();
  });
});
