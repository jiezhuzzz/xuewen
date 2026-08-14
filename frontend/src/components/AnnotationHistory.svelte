<script lang="ts">
  import { Redo2, Undo2 } from 'lucide-svelte';
  import { useHistoryCapability } from '@embedpdf/plugin-history/svelte';
  import { ANNOTATION_HISTORY_TOPIC } from '../lib/pdfEngine';

  let { documentId }: { documentId: string } = $props();

  const history = useHistoryCapability();
  const scope = $derived(history.provides?.forDocument(documentId) ?? null);

  // canUndo/canRedo are plain method calls on the capability, not reactive
  // state — mirror them into runes off the plugin's own change event so the
  // buttons enable and disable as marks are drawn.
  let canUndo = $state(false);
  let canRedo = $state(false);

  $effect(() => {
    const s = scope;
    if (!s) return;
    const sync = (): void => {
      const topic = s.getHistoryState().topics[ANNOTATION_HISTORY_TOPIC];
      canUndo = topic?.canUndo ?? false;
      canRedo = topic?.canRedo ?? false;
    };
    sync(); // a tab reopened mid-session may already have a stack
    return s.onHistoryChange(sync);
  });

  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<button
  type="button"
  class={btn}
  aria-label="Undo annotation"
  title="Undo annotation"
  disabled={!canUndo}
  onclick={() => scope?.undo(ANNOTATION_HISTORY_TOPIC)}
>
  <Undo2 size={16} />
</button>
<button
  type="button"
  class={btn}
  aria-label="Redo annotation"
  title="Redo annotation"
  disabled={!canRedo}
  onclick={() => scope?.redo(ANNOTATION_HISTORY_TOPIC)}
>
  <Redo2 size={16} />
</button>
