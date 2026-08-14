<script lang="ts">
  import { Trash2 } from 'lucide-svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { useAnnotation } from '@embedpdf/plugin-annotation/svelte';
  import { annotationList, annotations, removeAnnotation } from '../lib/annotationStore.svelte';
  import { colorHex, colorLabel } from '../lib/annotationPalette';
  import { toast } from '../lib/toasts.svelte';
  import type { Annotation } from '../lib/types';

  let { documentId }: { documentId: string } = $props();

  const scroll = useScroll(() => documentId);
  const annotation = useAnnotation(() => documentId);

  // The tab id is the paper id (see PdfPages).
  const items = $derived(annotationList(documentId));
  const failed = $derived(annotations.error[documentId] ?? null);

  const KIND_LABELS = {
    highlight: 'Highlight',
    underline: 'Underline',
    strikeout: 'Strikeout',
    squiggly: 'Squiggly',
    text_comment: 'Note',
  } as const;

  /// Jump to the mark's page and select it, so the reader can see which one
  /// the panel row refers to. Selecting is best-effort: a row whose payload
  /// the backend could not parse was never handed to the plugin, so there is
  /// nothing on the page to select — the page jump still works.
  function jump(a: Annotation): void {
    scroll.provides?.scrollToPage({ pageNumber: a.page_index + 1 });
    annotation.provides?.selectAnnotation(a.page_index, a.id);
  }

  /// Deleting through the plugin (rather than the store) is what keeps the
  /// drawn mark and the row in step: the plugin emits a delete event, and the
  /// sync loop removes the row. A mark that never made it into the document
  /// has no plugin copy, so that one is removed directly.
  async function remove(a: Annotation): Promise<void> {
    const inDocument = annotation.provides?.getAnnotationById(a.id);
    if (inDocument) {
      annotation.provides?.deleteAnnotation(a.page_index, a.id);
      return;
    }
    try {
      await removeAnnotation(documentId, a.id);
    } catch (e) {
      toast('error', (e as Error).message);
    }
  }
</script>

<div class="min-h-0 flex-1 overflow-y-auto p-1.5">
  {#if failed}
    <p class="px-1 py-3 text-xs text-stone-500 dark:text-stone-400">
      Annotations could not be loaded — {failed}
    </p>
  {:else if items.length === 0}
    <p class="px-1 py-3 text-xs text-stone-500 dark:text-stone-400">
      No annotations yet. Select text, then pick a tool from the toolbar.
    </p>
  {:else}
    <ul class="space-y-1">
      {#each items as a (a.id)}
        <li class="group relative">
          <button
            type="button"
            class="w-full rounded-lg border border-transparent px-2 py-1.5 text-left hover:border-stone-200 hover:bg-parchment dark:hover:border-stone-700 dark:hover:bg-stone-800"
            onclick={() => jump(a)}
          >
            <span class="flex items-center gap-1.5 text-[10px] text-stone-500 dark:text-stone-400">
              <span
                class="h-2.5 w-2.5 shrink-0 rounded-full"
                style:background-color={colorHex(a.color)}
                aria-hidden="true"
              ></span>
              <span>{KIND_LABELS[a.kind]}</span>
              <span aria-hidden="true">·</span>
              <span>p.{a.page_index + 1}</span>
              <span class="sr-only">{colorLabel(a.color)}</span>
            </span>
            {#if a.quoted_text}
              <span
                class="mt-0.5 block border-l-2 pl-1.5 font-serif text-xs leading-snug text-ink dark:text-stone-200"
                style:border-color={colorHex(a.color)}
              >
                {a.quoted_text}
              </span>
            {/if}
            {#if a.note}
              <span class="mt-1 block text-xs italic leading-snug text-stone-600 dark:text-stone-300">
                {a.note}
              </span>
            {/if}
            {#if !a.quoted_text && !a.note}
              <span class="mt-0.5 block text-xs text-stone-400 dark:text-stone-500">
                (no text)
              </span>
            {/if}
          </button>
          <!-- Sits over the row rather than inside it: a button inside a
               button is invalid HTML and the inner one never gets its click. -->
          <button
            type="button"
            class="absolute right-1 top-1 rounded p-1 text-stone-400 opacity-0 hover:bg-stone-200 hover:text-ink focus:opacity-100 group-hover:opacity-100 dark:hover:bg-stone-700 dark:hover:text-stone-100"
            aria-label={`Delete ${KIND_LABELS[a.kind]} on page ${a.page_index + 1}`}
            title="Delete"
            onclick={() => void remove(a)}
          >
            <Trash2 size={12} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
