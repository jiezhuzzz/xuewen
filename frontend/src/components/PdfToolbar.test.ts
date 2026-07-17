import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@embedpdf/plugin-zoom/svelte', () => ({
  useZoom: () => ({ state: { currentZoomLevel: 1 }, provides: null }),
}));
vi.mock('@embedpdf/plugin-scroll/svelte', () => ({
  useScroll: () => ({ state: { currentPage: 1, totalPages: 3 }, provides: null }),
}));

import PdfToolbar from './PdfToolbar.svelte';

const pill = {
  visible: true,
  setExtraHold: () => {},
  onPointerMove: () => {},
  onPointerLeave: () => {},
} as never;

describe('PdfToolbar', () => {
  it('gives icon-only nav and zoom buttons hover tooltips', () => {
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    for (const name of ['Previous page', 'Next page', 'Zoom out', 'Zoom in']) {
      expect(screen.getByRole('button', { name })).toHaveAttribute('title');
    }
  });
});
