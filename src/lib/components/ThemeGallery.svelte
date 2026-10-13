<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import Palette from "@lucide/svelte/icons/palette";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { settings } from "$lib/state/settings.svelte";
  import { matches, theme, type Filter } from "$lib/state/theme.svelte";
  import Picker from "./Picker.svelte";
  import ThemeCard from "./ThemeCard.svelte";
  import { t } from "$lib/i18n.svelte";

  let filter = $state<Filter>({ search: "", category: null, kind: null, installedOnly: false });

  const kinds = [
    { id: "look", icons: [Palette] },
    { id: "both", icons: [Palette, FileText] },
    { id: "templates", icons: [FileText] },
  ] as const;

  const categories = $derived([...new Set(theme.entries.map((entry) => entry.theme.category).filter(Boolean))].sort());
  const found = $derived(theme.entries.filter((entry) => matches(entry, filter)));

  $effect(() => {
    theme.browse();
  });
</script>

<section class="h-full overflow-auto p-6" aria-labelledby="themes-title">
  <div class="mb-1 flex items-center justify-between">
    <h2 id="themes-title" class="text-lg font-medium">{t("themes.title")}</h2>
    <div class="flex items-center gap-2">
      <Label for="themes-look">{t("themes.look")}</Label>
      <Picker
        id="themes-look"
        size="sm"
        value={settings.values.uiTheme}
        options={theme.looks}
        onchange={(id) => settings.set("uiTheme", id)}
      />
      <Button variant="outline" size="sm" onclick={() => theme.import()}>{t("themes.import")}</Button>
    </div>
  </div>
  <p class="mb-4 text-sm text-muted-foreground">
    {t("themes.intro")}
  </p>
  {#if theme.failure}<p class="mb-4 text-sm text-destructive" role="alert">{theme.failure}</p>{/if}
  {#if theme.storeFailure}
    <p class="mb-4 text-sm text-muted-foreground" role="status">
      {t("themes.storeFailure", { reason: theme.storeFailure })}
    </p>
  {/if}

  <div class="mb-4 flex flex-wrap items-center gap-2">
    <Input type="search" class="w-56" placeholder={t("themes.search")} aria-label={t("themes.search")} bind:value={filter.search} />
    <Button
      variant={filter.installedOnly ? "secondary" : "ghost"}
      size="sm"
      aria-pressed={filter.installedOnly}
      onclick={() => (filter.installedOnly = !filter.installedOnly)}
    >
      {t("themes.installed")}
    </Button>
    {#each kinds as { id, icons } (id)}
      <Button
        variant={filter.kind === id ? "secondary" : "ghost"}
        size="sm"
        aria-pressed={filter.kind === id}
        onclick={() => (filter.kind = filter.kind === id ? null : id)}
      >
        {#each icons as Icon, index (index)}<Icon />{/each}
        {t(`themes.kind.${id}`)}
      </Button>
    {/each}
    <span class="mx-1 h-5 border-s" aria-hidden="true"></span>
    {#each categories as category (category)}
      <Button
        variant={filter.category === category ? "secondary" : "ghost"}
        size="sm"
        aria-pressed={filter.category === category}
        onclick={() => (filter.category = filter.category === category ? null : category)}
      >
        {category}
      </Button>
    {/each}
  </div>

  <ul class="grid grid-cols-[repeat(auto-fill,minmax(18rem,1fr))] gap-4">
    {#each found as entry (entry.theme.id)}
      <li><ThemeCard {entry} /></li>
    {/each}
  </ul>
  {#if found.length === 0}<p class="mt-4 text-sm text-muted-foreground">{t("themes.none")}</p>{/if}
</section>
