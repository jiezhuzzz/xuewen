<script lang="ts">
  let { status }: { status: string } = $props();
  const resolved = $derived(status === 'resolved');
</script>

<span
  class={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
    resolved
      ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
      : 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-400'
  }`}
>
  {resolved ? 'resolved' : 'needs review'}
</span>
