import { describe, expect, it } from 'vitest';
import { PdfActionType, PdfZoomMode } from '@embedpdf/models';
import type { PdfBookmarkObject } from '@embedpdf/models';
import { toOutline } from './outline';

const dest = (pageIndex: number) =>
  ({ pageIndex, zoom: { mode: PdfZoomMode.Unknown }, view: [] }) as never;

describe('toOutline', () => {
  it('flattens titles, depths, and destination pages', () => {
    const bookmarks: PdfBookmarkObject[] = [
      {
        title: 'Introduction',
        target: { type: 'destination', destination: dest(0) },
        children: [{ title: 'Motivation', target: { type: 'destination', destination: dest(1) } }],
      },
      { title: 'Evaluation', target: { type: 'destination', destination: dest(7) } },
    ];
    const nodes = toOutline(bookmarks);
    expect(nodes).toHaveLength(2);
    expect(nodes[0]).toMatchObject({ title: 'Introduction', pageIndex: 0, depth: 0 });
    expect(nodes[0].children[0]).toMatchObject({ title: 'Motivation', pageIndex: 1, depth: 1 });
    expect(nodes[1]).toMatchObject({ title: 'Evaluation', pageIndex: 7, depth: 0 });
  });

  it('resolves Goto actions and leaves other targets unclickable', () => {
    const bookmarks: PdfBookmarkObject[] = [
      {
        title: 'Via action',
        target: { type: 'action', action: { type: PdfActionType.Goto, destination: dest(4) } },
      },
      {
        title: 'External link',
        target: { type: 'action', action: { type: PdfActionType.URI, uri: 'https://example.com' } },
      },
      { title: 'No target' },
    ];
    const nodes = toOutline(bookmarks);
    expect(nodes[0].pageIndex).toBe(4);
    expect(nodes[1].pageIndex).toBe(null);
    expect(nodes[2].pageIndex).toBe(null);
    expect(nodes[2].children).toEqual([]);
  });
});
