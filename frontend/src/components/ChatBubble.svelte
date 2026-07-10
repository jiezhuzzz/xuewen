<script lang="ts">
  // The assistant's launcher: 問 (wèn, "to ask") — the counterpart of the
  // wordmark's 學. Amber because it is an action; the cinnabar seal stays
  // reserved for its two identity placements.
  import { fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { chat } from '../lib/chat.svelte';
</script>

<button
  type="button"
  aria-label="Chat about this paper"
  title="Chat about this paper (c)"
  onclick={() => (chat.open = true)}
  transition:fade={{ duration: dur(DUR.fast) }}
  class="absolute bottom-5 right-5 z-[45] flex h-12 w-12 select-none items-center justify-center rounded-full bg-amber-700 font-serif text-xl leading-none text-paper shadow-lg hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
>問</button>
