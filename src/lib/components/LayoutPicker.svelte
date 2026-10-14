<script lang="ts">
  import { layouts, settings, type Layout } from "$lib/state/settings.svelte";
  import { t } from "$lib/i18n.svelte";

  type Box = { part: string; grow: number; strong?: boolean };

  const sketches: Record<Layout, Box[]> = {
    edit: [
      { part: "pages", grow: 2 },
      { part: "editor", grow: 4, strong: true },
      { part: "preview", grow: 4 },
      { part: "assistant", grow: 2 },
    ],
    agent: [
      { part: "pages", grow: 2 },
      { part: "assistant", grow: 6, strong: true },
      { part: "preview", grow: 4 },
    ],
    redaction: [
      { part: "assistant", grow: 3 },
      { part: "search", grow: 5, strong: true },
      { part: "filter", grow: 4 },
    ],
  };
</script>

<div class="grid grid-cols-3 gap-3" role="radiogroup" aria-label={t("settings.layout")}>
  {#each layouts as layout (layout)}
    {@const picked = settings.values.layout === layout}
    <button
      type="button"
      role="radio"
      aria-checked={picked}
      class="grid content-start gap-2 rounded-lg border p-2 text-start outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring aria-checked:border-primary aria-checked:ring-1 aria-checked:ring-primary"
      onclick={() => settings.set("layout", layout)}
    >
      <span class="flex h-16 gap-0.5 overflow-hidden rounded-md border bg-background p-0.5" aria-hidden="true">
        {#each sketches[layout] as { part, grow, strong } (part)}
          <span class="flex items-end justify-center overflow-hidden rounded-sm pb-0.5 text-[0.5rem] leading-none text-muted-foreground {strong ? 'bg-primary/20' : 'bg-muted'}" style:flex-grow={grow} style:flex-basis="0">{t(`settings.parts.${part}`)}</span>
        {/each}
      </span>
      <span class="text-sm font-medium">{t(`common.layout.${layout}`)}</span>
      <span class="text-xs text-muted-foreground">{t(`settings.layouts.${layout}`)}</span>
    </button>
  {/each}
</div>
