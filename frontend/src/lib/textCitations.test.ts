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
});
