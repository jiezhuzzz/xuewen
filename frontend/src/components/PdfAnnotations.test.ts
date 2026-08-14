import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const scroll = { scrollToPage: vi.fn() };
const annotation = {
  selectAnnotation: vi.fn(),
  deleteAnnotation: vi.fn(),
  getAnnotationById: vi.fn(() => undefined as unknown),
};

vi.mock('@embedpdf/plugin-scroll/svelte', () => ({
  useScroll: () => ({ state: { currentPage: 1, totalPages: 9 }, provides: scroll }),
}));
vi.mock('@embedpdf/plugin-annotation/svelte', () => ({
  useAnnotation: () => ({ provides: annotation, state: {} }),
}));

const deleteAnnotation = vi.fn(async (_paperId: string, _id: string) => {});
vi.mock('../lib/api', () => ({
  deleteAnnotation: (paperId: string, id: string) => deleteAnnotation(paperId, id),
  listAnnotations: vi.fn(),
  putAnnotation: vi.fn(),
  patchAnnotation: vi.fn(),
  clearAnnotations: vi.fn(),
}));

import PdfAnnotations from './PdfAnnotations.svelte';
import { annotations } from '../lib/annotationStore.svelte';
import { colorHex } from '../lib/annotationPalette';
import type { Annotation } from '../lib/types';

function row(over: Partial<Annotation> & { id: string }): Annotation {
  return {
    paper_id: 'p1',
    kind: 'highlight',
    page_index: 0,
    color: 'amber',
    quoted_text: null,
    note: null,
    payload: '{}',
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

function seed(...rows: Annotation[]): void {
  annotations.byPaper['p1'] = Object.fromEntries(rows.map((r) => [r.id, r]));
  annotations.loaded['p1'] = true;
}

beforeEach(() => {
  vi.clearAllMocks();
  annotations.byPaper = {};
  annotations.loaded = {};
  annotations.error = {};
  annotation.getAnnotationById.mockReturnValue(undefined);
});

const props = { documentId: 'p1' };

describe('the annotations panel', () => {
  it('says how to make one when the paper has none', () => {
    seed();
    render(PdfAnnotations, { props });
    expect(screen.getByText(/no annotations yet/i)).toBeInTheDocument();
  });

  it('reports a failed load instead of pretending the paper is unannotated', () => {
    annotations.error['p1'] = 'offline';
    render(PdfAnnotations, { props });
    expect(screen.getByText(/could not be loaded/i)).toBeInTheDocument();
    expect(screen.queryByText(/no annotations yet/i)).toBeNull();
  });

  it('lists marks in reading order with their quoted text and note', () => {
    seed(
      row({ id: 'b', page_index: 4, kind: 'underline', quoted_text: 'second' }),
      row({ id: 'a', page_index: 1, quoted_text: 'first', note: 'why it matters' }),
    );
    render(PdfAnnotations, { props });
    const rows = screen.getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent('first');
    expect(rows[0]).toHaveTextContent('why it matters');
    expect(rows[0]).toHaveTextContent('p.2'); // page_index is 0-based, the label is not
    expect(rows[1]).toHaveTextContent('Underline');
    expect(rows[1]).toHaveTextContent('p.5');
  });

  it('carries the mark’s color, so the panel matches the page', () => {
    seed(row({ id: 'a', color: 'violet', quoted_text: 'q' }));
    render(PdfAnnotations, { props });
    // The swatch is decorative; the color still has to reach a screen reader.
    expect(screen.getByText('Violet')).toBeInTheDocument();
    expect(screen.getByText('q')).toHaveStyle({ borderColor: colorHex('violet') });
  });

  it('shows a placeholder for a sticky note with nothing written in it yet', () => {
    seed(row({ id: 'a', kind: 'text_comment' }));
    render(PdfAnnotations, { props });
    expect(screen.getByText('(no text)')).toBeInTheDocument();
  });

  it('jumps to the page and selects the mark when a row is clicked', async () => {
    seed(row({ id: 'a', page_index: 6, quoted_text: 'there' }));
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByText('there'));
    expect(scroll.scrollToPage).toHaveBeenCalledWith({ pageNumber: 7 });
    expect(annotation.selectAnnotation).toHaveBeenCalledWith(6, 'a');
  });

  it('deletes through the plugin when the mark is drawn, letting sync remove the row', async () => {
    seed(row({ id: 'a', page_index: 2, quoted_text: 'drawn' }));
    annotation.getAnnotationById.mockReturnValue({ id: 'a' });
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByRole('button', { name: /^delete highlight/i }));
    expect(annotation.deleteAnnotation).toHaveBeenCalledWith(2, 'a');
    // Deleting twice — once here and once again through the store — would
    // 404 the second call and log an error the reader can do nothing about.
    expect(deleteAnnotation).not.toHaveBeenCalled();
  });

  it('deletes through the store when the mark never reached the document', async () => {
    seed(row({ id: 'a', quoted_text: 'orphan' }));
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByRole('button', { name: /^delete highlight/i }));
    expect(annotation.deleteAnnotation).not.toHaveBeenCalled();
    expect(deleteAnnotation).toHaveBeenCalledWith('p1', 'a');
    expect(screen.queryByText('orphan')).toBeNull();
  });
});
