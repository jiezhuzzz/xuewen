import { PdfActionType, type PdfDocumentObject, type PdfLinkTarget, type PdfPageObject } from '@embedpdf/models';
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

// EmbedPDF returns annotation rects, text-run rects, and GoTo destination points
// all in TOP-LEFT device space (y grows downward) — the same space the rendered
// page and our CSS overlay use — so coordinates pass through unchanged, no y-flip.
// (Verified live: with a flip, hover boxes landed vertically mirrored.)

function destOf(target: PdfLinkTarget | undefined): { pageIndex: number; y: number } | null {
  if (!target) return null;
  const dest =
    target.type === 'destination'
      ? target.destination
      : target.action.type === PdfActionType.Goto || target.action.type === PdfActionType.RemoteGoto
        ? target.action.destination
        : null;
  if (!dest) return null;
  const y = dest.zoom?.mode === 1 /* XYZ */ ? dest.zoom.params.y : 0;
  return { pageIndex: dest.pageIndex, y };
}

function uriOf(target: PdfLinkTarget | undefined): string | undefined {
  return target?.type === 'action' && target.action.type === PdfActionType.URI ? target.action.uri : undefined;
}

export async function loadCitations(engine: EngineLike, doc: PdfDocumentObject): Promise<CitationData> {
  const pages: PageText[] = [];
  const links: GotoLink[] = [];

  for (const page of doc.pages) {
    const [annos, textRuns] = await Promise.all([
      engine.getPageAnnotations(doc, page).toPromise(),
      engine.getPageTextRuns(doc, page).toPromise(),
    ]);

    const runs: TextRun[] = textRuns.runs.map((r) => ({
      text: r.text,
      x: r.rect.origin.x,
      y: r.rect.origin.y,
      width: r.rect.size.width,
      height: r.rect.size.height,
    }));

    const urlLinks: UrlLink[] = [];
    for (const a of annos) {
      if (a.type !== LINK) continue;
      const rect = { x: a.rect.origin.x, y: a.rect.origin.y, width: a.rect.size.width, height: a.rect.size.height };
      const url = uriOf(a.target);
      if (url) {
        urlLinks.push({ ...rect, url });
        continue;
      }
      const dest = destOf(a.target);
      if (!dest) continue;
      links.push({ pageIndex: page.index, ...rect, destPageIndex: dest.pageIndex, destY: dest.y });
    }

    pages.push({ pageIndex: page.index, width: page.size.width, height: page.size.height, runs, urlLinks });
  }

  const refStart = findReferencesStart(pages);
  if (!refStart) return { references: [], markers: [] };
  return buildCitationData(links, pages, refStart);
}
