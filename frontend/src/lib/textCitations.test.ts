import { describe, expect, it } from 'vitest';
import { columnMajorLines, segmentReferences } from './textCitations';
import type { PageText } from './citations';

function page(pageIndex: number, width: number, height: number, runs: PageText['runs'], urlLinks: PageText['urlLinks'] = []): PageText {
  return { pageIndex, width, height, runs, urlLinks };
}

describe('columnMajorLines', () => {
  it('joins same-baseline runs and orders lines column-major', () => {
    const p = page(0, 600, 800, [
      { text: 'right col line', x: 320, y: 50, width: 100, height: 12 },
      { text: 'left ', x: 50, y: 50, width: 40, height: 12 },
      { text: 'col line', x: 90, y: 51, width: 60, height: 12 },
    ]);
    const lines = columnMajorLines(p);
    expect(lines.map((l) => l.text)).toEqual(['left col line', 'right col line']);
    expect(lines[0].runs).toHaveLength(2);
    expect(lines[0].runs[1].start).toBe(5); // char offset of 'col line'
  });
});

describe('segmentReferences — numbered', () => {
  const refStart = { pageIndex: 2, y: 40, x: 50 };
  const bib = page(2, 600, 800, [
    { text: 'References', x: 50, y: 40, width: 90, height: 16 },
    { text: '[1] D. Kingma, J. Ba. Adam: A Method for', x: 50, y: 80, width: 300, height: 12 },
    { text: 'Stochastic Optimization. ICLR 2015.', x: 62, y: 100, width: 280, height: 12 },
    { text: '[2] J. Devlin et al. BERT. NAACL 2019.', x: 50, y: 130, width: 300, height: 12 },
  ], [{ x: 200, y: 100, width: 60, height: 12, url: 'https://arxiv.org/abs/1412.6980' }]);

  it('splits entries at [n] line starts and joins continuation lines', () => {
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('numbered');
    expect(seg.references).toHaveLength(2);
    expect(seg.references[0].rawText).toBe('[1] D. Kingma, J. Ba. Adam: A Method for Stochastic Optimization. ICLR 2015.');
    expect(seg.references[0].externalUrl).toBe('https://arxiv.org/abs/1412.6980');
    expect(seg.numberOf.get(1)).toBe(0);
    expect(seg.numberOf.get(2)).toBe(1);
  });

  it('returns null for a single [n] line (below the MIN_ENTRIES=2 guard)', () => {
    const tiny = page(0, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: '[1] Only one.', x: 50, y: 80, width: 120, height: 12 },
    ]);
    expect(segmentReferences([tiny], { pageIndex: 0, y: 40, x: 50 })).toBeNull();
  });

  it('includes right-column entries above the heading y (column-major, two-column page)', () => {
    // 600pt page, mid=300: heading + [1] in the LEFT column, [2] in the RIGHT
    // column at y=30 — above the heading's y=60, but AFTER it in reading order.
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 60, width: 90, height: 16 },
      { text: '[1] Left entry. 2020.', x: 50, y: 100, width: 180, height: 12 },
      { text: '[2] Right entry above heading y. 2021.', x: 320, y: 30, width: 240, height: 12 },
    ]);
    const seg = segmentReferences([bib], { pageIndex: 2, y: 60, x: 50 })!;
    expect(seg).not.toBeNull();
    expect(seg.style).toBe('numbered');
    expect(seg.references).toHaveLength(2);
    expect(seg.numberOf.get(1)).toBe(0);
    expect(seg.numberOf.get(2)).toBe(1);
    expect(seg.references[1].rawText).toBe('[2] Right entry above heading y. 2021.');
  });
});

describe('segmentReferences — author-year (hanging indent)', () => {
  const refStart = { pageIndex: 2, y: 40, x: 50 };

  it('splits entries at flush-left lines (continuations indented)', () => {
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. and Ba, J. (2015). Adam: a method', x: 50, y: 80, width: 300, height: 12 },
      { text: 'for stochastic optimization. In ICLR.', x: 68, y: 100, width: 260, height: 12 },
      { text: 'Devlin, J. et al. (2019). BERT. In NAACL.', x: 50, y: 130, width: 300, height: 12 },
      { text: 'Vaswani, A. (2017). Attention is all you need.', x: 50, y: 160, width: 300, height: 12 },
    ]);
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(3);
    expect(seg.references[0].rawText).toBe(
      'Kingma, D. and Ba, J. (2015). Adam: a method for stochastic optimization. In ICLR.',
    );
  });

  it('handles the inverted pattern (first line indented, continuations flush)', () => {
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. (2015). Adam: a method for', x: 68, y: 80, width: 280, height: 12 },
      { text: 'stochastic optimization. ICLR.', x: 50, y: 100, width: 240, height: 12 },
      { text: 'Devlin, J. (2019). BERT. NAACL.', x: 68, y: 130, width: 260, height: 12 },
    ]);
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(2);
  });

  it('rejects blocks where most entries lack a year (not a bibliography)', () => {
    const notBib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Some sentence without anything.', x: 50, y: 80, width: 280, height: 12 },
      { text: 'Another plain sentence here too.', x: 50, y: 110, width: 280, height: 12 },
    ]);
    expect(segmentReferences([notBib], refStart)).toBeNull();
  });

  it('segments two-column author-year bibliographies per column', () => {
    // Left column: starts x=50, one continuation line at x=68. Right column:
    // starts x=320, one continuation line at x=338. Globally the x=50 bucket
    // (heading + 2 entry starts) outranks x=320 (2 entry starts), so the old
    // global top-2-buckets logic picks {50, 320} as candidates and then the
    // year-share tiebreak (right column is 100% years vs left's 67%, since
    // "References" has no year) selects x=320 as the SOLE start column —
    // dropping both left-column entries entirely and returning only 2
    // references instead of 4. Per-column detection picks each column's own
    // start-x independently and recovers all 4 entries.
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. (2015). Adam: a method', x: 50, y: 80, width: 220, height: 12 },
      { text: 'for stochastic optimization. ICLR.', x: 68, y: 100, width: 200, height: 12 },
      { text: 'Devlin, J. (2019). BERT. NAACL.', x: 50, y: 130, width: 220, height: 12 },
      { text: 'He, K. (2016). ResNet. CVPR.', x: 320, y: 40, width: 200, height: 12 },
      { text: 'Vaswani, A. (2017). Attention is all', x: 320, y: 70, width: 220, height: 12 },
      { text: 'you need. NeurIPS.', x: 338, y: 90, width: 140, height: 12 },
    ]);
    const seg = segmentReferences([bib], { pageIndex: 2, y: 40, x: 50 })!;
    expect(seg).not.toBeNull();
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(4);
    expect(seg.references[2].rawText).toBe('He, K. (2016). ResNet. CVPR.');
    expect(seg.references[3].rawText).toBe('Vaswani, A. (2017). Attention is all you need. NeurIPS.');
  });
});
