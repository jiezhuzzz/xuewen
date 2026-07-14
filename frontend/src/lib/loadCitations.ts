import { PdfActionType, PdfZoomMode, type PdfDocumentObject, type PdfLinkTarget, type PdfPageObject } from '@embedpdf/models';
import {
  buildCitationData, findReferencesStart,
  type CitationData, type GotoLink, type PageText, type TextRun, type UrlLink,
} from './citations';

// A structural subset of @embedpdf/models' PdfAnnotationObject: just the
// fields this module reads off a link annotation. The real PdfAnnotationObject
// is a union (link/text/popup/stamp/...) whose PdfAnnotationObjectBase
// requires bookkeeping fields (`id`, etc.) this module never touches, so
// using it verbatim as EngineLike's return type made the fake test engine
// (which supplies only type/rect/target) fail `npm run check` with
// "Property 'id' is missing". `target`'s shape, however, is the real
// PdfLinkTarget union (destination vs. action), so destOf/uriOf below are
// checked by the compiler against the real discriminated union.
export interface EngineAnnotation {
  type: number;
  rect: { origin: { x: number; y: number }; size: { width: number; height: number } };
  target?: PdfLinkTarget;
}

export interface EngineLike {
  getPageAnnotations(doc: PdfDocumentObject, page: PdfPageObject): { toPromise(): Promise<EngineAnnotation[]> };
  getPageTextRuns(
    doc: PdfDocumentObject,
    page: PdfPageObject,
  ): { toPromise(): Promise<{ runs: { text: string; rect: { origin: { x: number; y: number }; size: { width: number; height: number } } }[] }> };
}

const LINK = 2; // PdfAnnotationSubtype.LINK

// Coordinate spaces (verified live against real PDFs):
//  - Annotation rects and text-run rects come back in TOP-LEFT device space
//    (y grows downward) — the same space the rendered page + CSS overlay use —
//    so they pass through unchanged (a flip mirrored the hover boxes vertically).
//  - GoTo destination `y`, however, is left in PDF USER space (BOTTOM-LEFT origin,
//    y grows upward), so it must be flipped to top-left before it can be lined up
//    with the reference text runs — otherwise a marker maps to the mirror-image
//    reference (e.g. the popover showed the wrong entry).

function destOf(
  target: PdfLinkTarget | undefined,
): { pageIndex: number; x: number | null; y: number | null } | null {
  if (!target) return null;
  const dest =
    target.type === 'destination'
      ? target.destination
      : target.action.type === PdfActionType.Goto || target.action.type === PdfActionType.RemoteGoto
        ? target.action.destination
        : null;
  if (!dest) return null;
  const zoom = dest.zoom;
  if (zoom?.mode === PdfZoomMode.XYZ) {
    return { pageIndex: dest.pageIndex, x: zoom.params.x, y: zoom.params.y };
  }
  // /FitH-family carries its `top` only in the raw view array.
  if (
    (zoom?.mode === PdfZoomMode.FitHorizontal || zoom?.mode === PdfZoomMode.FitBoundingBoxHorizontal) &&
    typeof dest.view?.[0] === 'number'
  ) {
    return { pageIndex: dest.pageIndex, x: null, y: dest.view[0] };
  }
  // All other / unhandled zoom modes: no usable vertical position here —
  // anchor the page TOP. (y:null must never go through the bottom-left→
  // top-left flip, which would turn "unknown" into "page bottom" and
  // mis-map every marker on that page.)
  return { pageIndex: dest.pageIndex, x: null, y: null };
}

function uriOf(target: PdfLinkTarget | undefined): string | undefined {
  return target?.type === 'action' && target.action.type === PdfActionType.URI ? target.action.uri : undefined;
}

export async function loadCitations(engine: EngineLike, doc: PdfDocumentObject): Promise<CitationData> {
  // Pass 1 — link annotations from every page (cheap). These give the citation
  // markers + their destinations, plus any URL links (kept per page so a
  // reference's DOI/URL can be attached below).
  const links: GotoLink[] = [];
  const urlLinksByPage = new Map<number, UrlLink[]>();
  for (const page of doc.pages) {
    const annos = await engine.getPageAnnotations(doc, page).toPromise();
    for (const a of annos) {
      if (a.type !== LINK) continue;
      const rect = { x: a.rect.origin.x, y: a.rect.origin.y, width: a.rect.size.width, height: a.rect.size.height };
      const url = uriOf(a.target);
      if (url) {
        const arr = urlLinksByPage.get(page.index) ?? [];
        arr.push({ ...rect, url });
        urlLinksByPage.set(page.index, arr);
        continue;
      }
      const dest = destOf(a.target);
      if (!dest) continue;
      // Flip a KNOWN y from PDF bottom-left space into top-left space; an
      // unknown y anchors the page top (see destOf).
      const destPage = doc.pages[dest.pageIndex];
      const destY = dest.y == null ? 0 : destPage ? destPage.size.height - dest.y : dest.y;
      links.push({ pageIndex: page.index, ...rect, destPageIndex: dest.pageIndex, destY, destX: dest.x ?? 0 });
    }
  }
  if (links.length === 0) return { references: [], markers: [] };

  // Pass 2 — the bibliography lives on the pages the citation links point to, so
  // read text runs (the expensive call) ONLY for those pages (usually a handful),
  // not every page. Markers already come from the annotations above.
  const refPageIndexes = [...new Set(links.map((l) => l.destPageIndex))].sort((a, b) => a - b);
  const pages: PageText[] = [];
  for (const idx of refPageIndexes) {
    const page = doc.pages[idx];
    if (!page) continue;
    const textRuns = await engine.getPageTextRuns(doc, page).toPromise();
    const runs: TextRun[] = textRuns.runs.map((r) => ({
      text: r.text,
      x: r.rect.origin.x,
      y: r.rect.origin.y,
      width: r.rect.size.width,
      height: r.rect.size.height,
    }));
    pages.push({ pageIndex: idx, width: page.size.width, height: page.size.height, runs, urlLinks: urlLinksByPage.get(idx) ?? [] });
  }

  const refStart = findReferencesStart(pages);
  if (!refStart) return { references: [], markers: [] };
  return buildCitationData(links, pages, refStart);
}
