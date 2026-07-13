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
// (which supplies only type/pageIndex/rect/target) fail `npm run check` with
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

// PDFium annotation/text rects use a bottom-left origin (y grows upward). Our
// pure logic + CSS overlay use a top-left origin (y grows downward). Convert by
// flipping about the page height. NOTE: verified empirically in Step 5 — if a
// real demo PDF's markers land mirrored vertically, this flip is the switch.
// CALIBRATION: unverified against a real PDF — see Task 10's manual check.
function toTopLeftY(pageHeight: number, yBottomLeft: number, rectHeight: number): number {
  return pageHeight - yBottomLeft - rectHeight;
}

const LINK = 2; // PdfAnnotationSubtype.LINK

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
    const h = page.size.height;
    const [annos, textRuns] = await Promise.all([
      engine.getPageAnnotations(doc, page).toPromise(),
      engine.getPageTextRuns(doc, page).toPromise(),
    ]);

    const runs: TextRun[] = textRuns.runs.map((r) => ({
      text: r.text,
      x: r.rect.origin.x,
      y: toTopLeftY(h, r.rect.origin.y, r.rect.size.height),
      width: r.rect.size.width,
      height: r.rect.size.height,
    }));

    const urlLinks: UrlLink[] = [];
    for (const a of annos) {
      if (a.type !== LINK) continue;
      const url = uriOf(a.target);
      const ry = toTopLeftY(h, a.rect.origin.y, a.rect.size.height);
      if (url) {
        urlLinks.push({ x: a.rect.origin.x, y: ry, width: a.rect.size.width, height: a.rect.size.height, url });
        continue;
      }
      const dest = destOf(a.target);
      if (!dest) continue;
      // Destination y is a point on the destination page (top-left after flip).
      const destPage = doc.pages[dest.pageIndex];
      const destY = destPage ? toTopLeftY(destPage.size.height, dest.y, 0) : dest.y;
      links.push({
        pageIndex: page.index,
        x: a.rect.origin.x, y: ry, width: a.rect.size.width, height: a.rect.size.height,
        destPageIndex: dest.pageIndex, destY,
      });
    }

    pages.push({ pageIndex: page.index, width: page.size.width, height: h, runs, urlLinks });
  }

  const refStart = findReferencesStart(pages);
  if (!refStart) return { references: [], markers: [] };
  return buildCitationData(links, pages, refStart);
}
