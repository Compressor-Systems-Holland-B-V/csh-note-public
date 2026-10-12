<script lang="ts">
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import { Button } from "$lib/components/ui/button";
  import type { PageView } from "$lib/state/document.svelte";

  let { pages }: { pages: PageView[] } = $props();

  const cssPixelsPerPoint = 96 / 72;
  const step = 1.25;

  let zoom = $state<number | null>(null);

  const zoomLabel = $derived(zoom === null ? "Fit" : `${Math.round(zoom * 100)}%`);

  function scale(factor: number) {
    zoom = Math.min(8, Math.max(0.1, (zoom ?? 1) * factor));
  }
</script>

<div class="flex h-full flex-col">
  <div class="flex h-9 shrink-0 items-center justify-end gap-1 border-b px-2 text-xs text-muted-foreground">
    <span class="mr-1 tabular-nums" aria-live="polite">{zoomLabel}</span>
    <Button variant="ghost" size="icon-xs" aria-label="Zoom out" title="Zoom out" onclick={() => scale(1 / step)}>
      <Minus />
    </Button>
    <Button variant="ghost" size="icon-xs" aria-label="Zoom in" title="Zoom in" onclick={() => scale(step)}>
      <Plus />
    </Button>
    <Button variant="ghost" size="xs" disabled={zoom === null} onclick={() => (zoom = null)}>Fit width</Button>
  </div>

  <div class="min-h-0 flex-1 overflow-auto bg-muted p-4">
    {#if pages.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-muted-foreground">
        Your pages will show here.
      </div>
    {:else}
      <div class="flex flex-col items-center gap-4 {zoom === null ? 'w-full' : 'w-max min-w-full'}">
        {#each pages as page, index (index)}
          <img
            src={page.url}
            alt="Page {index + 1}"
            class="block max-w-none border shadow-sm"
            style:width={zoom === null ? "100%" : `${page.width * cssPixelsPerPoint * zoom}px`}
            style:aspect-ratio="{page.width} / {page.height}"
          />
        {/each}
      </div>
    {/if}
  </div>
</div>
