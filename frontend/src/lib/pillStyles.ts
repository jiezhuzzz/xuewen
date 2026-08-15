/// The reader pill's shared control styling — one copy for the components
/// whose buttons are meant to look identical (PdfToolbar, AnnotationTools,
/// AnnotationHistory, AnnotationSelectionMenu, PdfFindBar). PdfQuickActions'
/// serif glyph buttons and the side panels' denser row buttons differ on
/// purpose and keep their own.
export const btn =
  'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
export const activeBtn =
  'rounded-lg p-1.5 bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
