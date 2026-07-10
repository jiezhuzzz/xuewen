<script lang="ts">
  let { status }: { status: string } = $props();
  const resolved = $derived(status === 'resolved');
</script>

<span
  class={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
    resolved
      ? 'bg-lime-100 text-lime-800 dark:bg-lime-500/15 dark:text-lime-300'
      : 'bg-yellow-100 text-yellow-800 dark:bg-yellow-500/15 dark:text-yellow-300'
  }`}
>
  {resolved ? 'resolved' : 'needs review'}
</span>
