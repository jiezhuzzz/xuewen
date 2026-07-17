<script lang="ts">
  import { Eraser, SendHorizontal, Square } from 'lucide-svelte';
  import { chat, clearChatThread, sendChatMessage, setChatModel, stopChatStream } from '../lib/chat.svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';

  let transcript = $state<HTMLElement | null>(null);
  // Stick to the bottom unless the reader scrolled up to reread something.
  let stick = $state(true);
  function onScroll() {
    if (!transcript) return;
    stick = transcript.scrollTop + transcript.clientHeight >= transcript.scrollHeight - 40;
  }
  $effect(() => {
    void chat.messages.length;
    void chat.streaming;
    if (stick && transcript) transcript.scrollTop = transcript.scrollHeight;
  });

  let confirmingClear = $state(false);

  function onComposerKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void sendChatMessage();
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <header class="flex shrink-0 items-center gap-2 border-b border-stone-200 px-3 py-2 dark:border-stone-800">
    <select
      aria-label="Model"
      value={chat.modelId}
      onchange={(e) => setChatModel((e.currentTarget as HTMLSelectElement).value)}
      class="min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
    >
      {#each chat.models as m (m.id)}
        <option value={m.id}>{m.label}</option>
      {/each}
    </select>
    <button
      type="button"
      aria-label="Clear conversation"
      onclick={() => (confirmingClear = true)}
      class="rounded-lg p-1.5 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <Eraser size={15} />
    </button>
  </header>

  {#if confirmingClear}
    <div class="flex shrink-0 items-center gap-2 border-b border-stone-200 bg-parchment/60 px-3 py-2 text-sm dark:border-stone-800 dark:bg-stone-800/40">
      <span class="min-w-0 flex-1 text-stone-600 dark:text-stone-300">Clear this conversation?</span>
      <ConfirmButtons
        confirmLabel="Clear"
        onConfirm={() => {
          confirmingClear = false;
          void clearChatThread();
        }}
        onCancel={() => (confirmingClear = false)}
      />
    </div>
  {/if}

  <div bind:this={transcript} onscroll={onScroll} class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
    {#if chat.messages.length === 0 && chat.pending === null}
      <p class="px-2 pt-6 text-center text-sm text-stone-400 dark:text-stone-500">
        Ask about the methods, the results, or how this paper connects to what you already know.
      </p>
    {/if}
    {#each chat.messages as m (m.id)}
      {#if m.role === 'user'}
        <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
          {m.content}
        </div>
      {:else}
        <div class="mr-2">
          <div class="whitespace-pre-wrap font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
            {m.content}
          </div>
          {#if m.model}
            <p class="mt-1 font-mono text-[10px] uppercase tracking-wide text-stone-400 dark:text-stone-500">
              {m.model}
            </p>
          {/if}
        </div>
      {/if}
    {/each}
    {#if chat.pending !== null}
      <div class="ml-8 whitespace-pre-wrap rounded-lg bg-parchment px-3 py-2 text-sm text-ink dark:bg-stone-800 dark:text-stone-100">
        {chat.pending}
      </div>
      <div class="mr-2 whitespace-pre-wrap font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
        {chat.streaming}<span class="animate-pulse">▍</span>
      </div>
    {/if}
    {#if chat.error}
      <p class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-500/10 dark:text-red-400">
        {chat.error}
      </p>
    {/if}
  </div>

  <footer class="flex shrink-0 items-end gap-2 border-t border-stone-200 p-2 dark:border-stone-800">
    <textarea
      bind:value={chat.draft}
      onkeydown={onComposerKeydown}
      rows="2"
      placeholder="Ask about this paper…"
      class="min-h-0 flex-1 resize-none rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-sm outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
    ></textarea>
    {#if chat.busy}
      <button
        type="button"
        onclick={stopChatStream}
        class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-sm font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      >
        <Square size={13} /> Stop
      </button>
    {:else}
      <button
        type="button"
        onclick={() => void sendChatMessage()}
        disabled={!chat.draft.trim()}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <SendHorizontal size={14} /> Send
      </button>
    {/if}
  </footer>
</div>
