import { describe, expect, it } from 'vitest';
import { matchReferences, normalizeTitle } from './citationMatch';
import type { Reference } from './citations';

function ref(index: number, rawText: string): Reference {
  return { index, destPageIndex: 1, destY: 100 + index, rawText };
}

describe('normalizeTitle', () => {
  it('lowercases, strips punctuation, collapses whitespace (mirrors matching.rs)', () => {
    expect(normalizeTitle('KGAT: Knowledge-Graph  Attention Network!')).toBe(
      'kgat knowledge graph attention network',
    );
  });
});

describe('matchReferences', () => {
  const papers = [
    { id: 'p-adam', title: 'Adam: A Method for Stochastic Optimization' },
    { id: 'p-resnet', title: 'Deep Residual Learning for Image Recognition' },
    { id: 'p-empty', title: null },
  ];

  it('matches when a library title appears verbatim inside the reference text', () => {
    const refs = [ref(0, '[12] D. Kingma, J. Ba. Adam: A Method for Stochastic Optimization. ICLR 2015.')];
    const m = matchReferences(refs, papers);
    expect(m.get(0)?.id).toBe('p-adam');
  });

  it('does not match unrelated references', () => {
    const refs = [ref(0, '[3] Some Unrelated Paper About Frogs. Nature 2001.')];
    expect(matchReferences(refs, papers).has(0)).toBe(false);
  });

  it('guards against very short titles causing false positives', () => {
    const shortTitlePapers = [{ id: 'p-x', title: 'On It' }];
    const refs = [ref(0, '[1] A paper that mentions on it somewhere in prose. 2020.')];
    expect(matchReferences(refs, shortTitlePapers).has(0)).toBe(false);
  });

  it('ignores papers with null titles', () => {
    const refs = [ref(0, 'anything at all')];
    expect(matchReferences(refs, papers).has(0)).toBe(false);
  });
});
