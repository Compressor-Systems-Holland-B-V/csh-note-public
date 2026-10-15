<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import Package from "@lucide/svelte/icons/package";
  import Palette from "@lucide/svelte/icons/palette";
  import Search from "@lucide/svelte/icons/search";
  import Upload from "@lucide/svelte/icons/upload";
  import type { Component } from "svelte";
  import { Button } from "@zsuite/ui/button";
  import { Input } from "@zsuite/ui/input";
  import { keyOf, matches, theme, type Filter } from "$lib/state/theme.svelte";
  import ThemeCard from "./ThemeCard.svelte";
  import { t } from "$lib/i18n.svelte";

  let filter = $state<Filter>({ search: "", category: null, kind: null, installedOnly: false });

  const kinds = [
    { id: null, icon: LayoutGrid, label: "themes.all" },
    { id: "look", icon: Palette, label: "themes.kind.look" },
    { id: "templates", icon: FileText, label: "themes.kind.templates" },
  ] as const;

  const categories = $derived([...new Set(theme.entries.map((entry) => entry.theme.category).filter(Boolean))].sort());
  const found = $derived(theme.entries.filter((entry) => matches(entry, filter)));
  const installedCount = $derived(theme.entries.filter((entry) => entry.installed).length);

  $effect(() => {
    theme.browse();
  });
</script>

{#snippet choice(Icon: Component | null, label: string, picked: boolean, onclick: () => void, count: number | null = null)}
  <Button variant={picked ? "secondary" : "ghost"} size="sm" class="w-full justify-start" aria-pressed={picked} {onclick}>
    {#if Icon}<Icon />{/if}
    <span class="min-w-0 flex-1 truncate text-start">{label}</span>
    {#if count !== null}<span class="text-xs text-muted-foreground tabular-nums">{count}</span>{/if}
  </Button>
{/snippet}

<div class="flex h-full min-h-0">
  <aside class="flex w-56 shrink-0 flex-col gap-5 overflow-y-auto border-e bg-sidebar p-3" aria-label={t("themes.filters")}>
    <h2 id="themes-title" class="px-2 pt-1 text-lg font-medium">{t("themes.title")}</h2>
    <div class="grid gap-0.5">
      {@render choice(LayoutGrid, t("themes.browse"), !filter.installedOnly, () => (filter.installedOnly = false), theme.entries.length)}
      {@render choice(Package, t("themes.installed"), filter.installedOnly, () => (filter.installedOnly = true), installedCount)}
    </div>
    <div class="grid gap-0.5">
      <p class="px-2 pb-1 text-xs font-medium text-muted-foreground">{t("themes.type")}</p>
      {#each kinds as { id, icon, label } (label)}
        {@render choice(icon, t(label), filter.kind === id, () => (filter.kind = id))}
      {/each}
    </div>
    {#if categories.length > 0}
      <div class="grid gap-0.5">
        <p class="px-2 pb-1 text-xs font-medium text-muted-foreground">{t("themes.category")}</p>
        {@render choice(null, t("themes.all"), filter.category === null, () => (filter.category = null))}
        {#each categories as category (category)}
          {@render choice(null, category, filter.category === category, () => (filter.category = category))}
        {/each}
      </div>
    {/if}
    <span class="flex-1"></span>
    <Button variant="outline" size="sm" title={t("themes.importTitle")} onclick={() => theme.import()}><Upload /> {t("themes.import")}</Button>
  </aside>

  <section class="min-w-0 flex-1 overflow-y-auto" aria-labelledby="themes-title">
    <div class="sticky top-0 z-10 flex items-center gap-3 border-b bg-background/95 px-6 py-3 backdrop-blur">
      <div class="relative max-w-md flex-1">
        <Search class="pointer-events-none absolute start-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input type="search" class="ps-8" placeholder={t("themes.search")} aria-label={t("themes.search")} bind:value={filter.search} />
      </div>
      <p class="hidden text-sm text-muted-foreground lg:block">{t("themes.intro")}</p>
    </div>
    <div class="p-6">
      {#if theme.failure}<p class="mb-4 text-sm text-destructive" role="alert">{theme.failure}</p>{/if}
      {#if theme.storeFailure}<p class="mb-4 rounded-md bg-muted p-3 text-sm text-muted-foreground" role="status">{t("themes.storeFailure", { reason: theme.storeFailure })}</p>{/if}
      <ul class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-4">
        {#each found as entry (keyOf(entry.theme))}
          <li><ThemeCard {entry} /></li>
        {/each}
      </ul>
      {#if found.length === 0}<p class="py-12 text-center text-sm text-muted-foreground">{t("themes.none")}</p>{/if}
    </div>
  </section>
</div>
