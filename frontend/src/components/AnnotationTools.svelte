<script lang="ts">
  import { ChevronDown, Highlighter, MessageSquare, Strikethrough, Underline, Waves } from 'lucide-svelte';
  import { useAnnotation, useAnnotationCapability } from '@embedpdf/plugin-annotation/svelte';
  import { PdfAnnotationSubtype } from '@embedpdf/models';
  import { clickOutside } from '../lib/clickOutside';
  import { type AnnotationKind, TOOL_BY_KIND } from '../lib/annotationAdapter';
  import { annotationTools, setToolColor, toggleTool } from '../lib/annotationState.svelte';
  import {
    type AnnotationColor,
    ANNOTATION_COLORS,
    colorHex,
    colorLabel,
  } from '../lib/annotationPalette';

  let { documentId, onHoldChange }: { documentId: string; onHoldChange: (held: boolean) => void } =
    $props();

  const annotation = useAnnotation(() => documentId);
  const capability = useAnnotationCapability();

  let menuOpen = $state(false);
  $effect(() => onHoldChange(menuOpen));

  const TOOLS: { kind: AnnotationKind; label: string; icon: typeof Highlighter }[] = [
    { kind: 'highlight', label: 'Highlight', icon: Highlighter },
    { kind: 'underline', label: 'Underline', icon: Underline },
    { kind: 'strikeout', label: 'Strikeout', icon: Strikethrough },
    { kind: 'squiggly', label: 'Squiggly', icon: Waves },
    { kind: 'text_comment', label: 'Note', icon: MessageSquare },
  ];

  const active = $derived(annotationTools.active);
  const activeTool = $derived(TOOLS.find((t) => t.kind === active));
  const TriggerIcon = $derived(activeTool?.icon ?? Highlighter);

  // The armed tool lives in app state (global, so it survives a tab switch);
  // the plugin needs telling per document. Driving it from an effect rather
  // than from the click handler means a newly opened tab inherits whatever is
  // armed, instead of looking disarmed until the next click.
  $effect(() => {
    const scope = annotation.provides;
    if (!scope) return;
    scope.setActiveTool(active === null ? null : TOOL_BY_KIND[active]);
  });

  // Tools are global in the plugin, so the color goes to every tool at once —
  // matching the single global color preference.
  $effect(() => {
    const cap = capability.provides;
    if (!cap) return;
    const hex = colorHex(annotationTools.color);
    for (const [kind, toolId] of Object.entries(TOOL_BY_KIND)) {
      // A sticky note is an icon: stroke only, no fill to color.
      cap.setToolDefaults(toolId, kind === 'text_comment' ? { strokeColor: hex } : { color: hex, strokeColor: hex });
    }
  });

  /// Picking a color both arms the next mark and recolors whatever is selected
  /// — the selected-mark case is what a reader expects after drawing one and
  /// deciding it should have been a different color.
  function pickColor(c: AnnotationColor): void {
    setToolColor(c);
    const scope = annotation.provides;
    if (!scope) return;
    const hex = colorHex(c);
    for (const sel of scope.getSelectedAnnotations()) {
      // A sticky note is an icon: stroke only, no fill to color.
      const patch =
        sel.object.type === PdfAnnotationSubtype.TEXT
          ? { strokeColor: hex }
          : { color: hex, strokeColor: hex };
      scope.updateAnnotation(sel.object.pageIndex, sel.object.id, patch);
    }
  }

  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
  const activeBtn = 'rounded-lg p-1.5 bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -- the keydown only
     carries Escape while the menu is open, and lives on the wrapper because
     the trigger keeps focus after opening (same pattern as the zoom menu).
     Every interactive child is a real button. -->
<div
  class="relative"
  use:clickOutside={() => {
    if (menuOpen) menuOpen = false;
  }}
  onkeydown={(e) => {
    if (menuOpen && e.key === 'Escape') {
      e.stopPropagation(); // the global cascade must not see this
      menuOpen = false;
    }
  }}
>
  <button
    type="button"
    class={`${active ? activeBtn : btn} flex items-center gap-0.5`}
    aria-label={active ? `Annotation tool: ${activeTool?.label}` : 'Annotation tools'}
    aria-expanded={menuOpen}
    title={active ? `${activeTool?.label} — click to change` : 'Annotate'}
    onclick={() => (menuOpen = !menuOpen)}
  >
    <TriggerIcon size={16} />
    <ChevronDown size={12} />
  </button>

  {#if menuOpen}
    <div
      role="menu"
      aria-label="Annotation tools"
      class="absolute left-1/2 top-full z-30 mt-1.5 w-max -translate-x-1/2 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
    >
      <div class="flex items-center gap-0.5">
        {#each TOOLS as tool (tool.kind)}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={active === tool.kind}
            class={active === tool.kind ? activeBtn : btn}
            aria-label={tool.label}
            title={tool.label}
            onclick={() => toggleTool(tool.kind)}
          >
            <tool.icon size={16} />
          </button>
        {/each}
      </div>
      <div class="mt-1.5 flex items-center gap-1 border-t border-stone-200 pt-1.5 dark:border-stone-800">
        {#each ANNOTATION_COLORS as c (c)}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={annotationTools.color === c}
            class={`h-5 w-5 rounded-full border-2 ${
              annotationTools.color === c
                ? 'border-ink dark:border-stone-100'
                : 'border-transparent hover:border-stone-300 dark:hover:border-stone-600'
            }`}
            style:background-color={colorHex(c)}
            aria-label={colorLabel(c)}
            title={colorLabel(c)}
            onclick={() => pickColor(c)}
          ></button>
        {/each}
      </div>
    </div>
  {/if}
</div>
