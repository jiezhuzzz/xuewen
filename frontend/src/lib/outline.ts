import { PdfActionType } from '@embedpdf/models';
import type { PdfBookmarkObject } from '@embedpdf/models';

/// One outline row, render-ready. `pageIndex` is 0-based; null when the
/// bookmark has no resolvable page target (rendered non-clickable).
export interface OutlineNode {
  title: string;
  pageIndex: number | null;
  depth: number;
  children: OutlineNode[];
}

function targetPage(b: PdfBookmarkObject): number | null {
  const t = b.target;
  if (!t) return null;
  if (t.type === 'destination') return t.destination.pageIndex;
  if (t.type === 'action' && t.action.type === PdfActionType.Goto) {
    return t.action.destination.pageIndex;
  }
  return null; // URI / remote-goto / launch actions aren't in-document jumps
}

/// Convert EmbedPDF's bookmark tree into OutlineNodes.
export function toOutline(bookmarks: PdfBookmarkObject[], depth = 0): OutlineNode[] {
  return bookmarks.map((b) => ({
    title: b.title,
    pageIndex: targetPage(b),
    depth,
    children: toOutline(b.children ?? [], depth + 1),
  }));
}
