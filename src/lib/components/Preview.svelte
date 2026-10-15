<script lang="ts">
  import { untrack } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { Button } from "@zsuite/ui/button";
  import type { PageView } from "$lib/state/document.svelte";
  import { t } from "$lib/i18n.svelte";

  let {
    pages,
    startZoom,
    loading,
    onrefresh,
  }: { pages: PageView[]; startZoom: number | null; loading: boolean; onrefresh: () => void } = $props();

  const cssPixelsPerPoint = 96 / 72;
  const step = 1.25;
  const wheelStep = 1.1;

  let zoom = $state(untrack(() => startZoom));

  const zoomLabel = $derived(zoom === null ? t("editor.preview.fit") : `${Math.round(zoom * 100)}%`);

  let scroller = $state<HTMLDivElement>();

  function scale(factor: number) {
    zoom = Math.min(8, Math.max(0.1, (zoom ?? 1) * factor));
  }

  $effect(() => {
    const element = scroller;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      scale(event.deltaY < 0 ? wheelStep : 1 / wheelStep);
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  });
</script>

<div class="flex h-full flex-col">
  <div class="flex h-9 shrink-0 items-center gap-1 border-b px-2 text-xs text-muted-foreground">
    <Button variant="ghost" size="icon-xs" aria-label={t("editor.preview.refresh")} title={t("editor.preview.refresh")} onclick={onrefresh}>
      <RefreshCw />
    </Button>
    <span class="flex flex-1 items-center gap-1" role="status">
      {#if loading}<LoaderCircle class="size-3.5 animate-spin" /> {t("editor.preview.updating")}{/if}
    </span>
    <span class="me-1 tabular-nums" aria-live="polite">{zoomLabel}</span>
    <Button variant="ghost" size="icon-xs" aria-label={t("editor.preview.zoomOut")} title={t("editor.preview.zoomOut")} onclick={() => scale(1 / step)}>
      <Minus />
    </Button>
    <Button variant="ghost" size="icon-xs" aria-label={t("editor.preview.zoomIn")} title={t("editor.preview.zoomIn")} onclick={() => scale(step)}>
      <Plus />
    </Button>
    <Button variant="ghost" size="xs" disabled={zoom === null} onclick={() => (zoom = null)}>{t("common.fitWidth")}</Button>
  </div>

  <div bind:this={scroller} class="min-h-0 flex-1 overflow-auto bg-muted p-4">
    {#if pages.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-muted-foreground">
        {t("editor.preview.empty")}
      </div>
    {:else}
      <div class="flex flex-col items-center gap-4 {zoom === null ? 'w-full' : 'w-max min-w-full'}">
        {#each pages as page, index (index)}
          <img
            src={page.url}
            alt={t("editor.preview.page", { number: index + 1 })}
            class="block max-w-none border shadow-sm"
            style:width={zoom === null ? "100%" : `${page.width * cssPixelsPerPoint * zoom}px`}
            style:aspect-ratio="{page.width} / {page.height}"
          />
        {/each}
      </div>
    {/if}
  </div>
</div>
