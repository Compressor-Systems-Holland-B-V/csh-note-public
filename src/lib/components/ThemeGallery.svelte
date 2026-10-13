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

  let filter = $state<Filter>({ search: "", category: null, kind: null, installedOnly: false });

  const kinds = [
    { id: "look", label: "Look only", icons: [Palette] },
    { id: "both", label: "Look and templates", icons: [Palette, FileText] },
    { id: "templates", label: "Templates only", icons: [FileText] },
  ] as const;

  const categories = $derived([...new Set(theme.entries.map((entry) => entry.theme.category).filter(Boolean))].sort());
  const found = $derived(theme.entries.filter((entry) => matches(entry, filter)));

  $effect(() => {
    theme.browse();
  });
</script>

<section class="h-full overflow-auto p-6" aria-labelledby="themes-title">
  <div class="mb-1 flex items-center justify-between">
    <h2 id="themes-title" class="text-lg font-medium">Themes</h2>
    <div class="flex items-center gap-2">
      <Label for="themes-look">Look</Label>
      <Picker
        id="themes-look"
        size="sm"
        value={settings.values.uiTheme}
        options={theme.looks}
        onchange={(id) => settings.set("uiTheme", id)}
      />
      <Button variant="outline" size="sm" onclick={() => theme.import()}>Import a theme file…</Button>
    </div>
  </div>
  <p class="mb-4 text-sm text-muted-foreground">
    A theme can change how znote looks, add page templates, or both. The switches on a theme decide whether its look
    and its templates are offered; pick the look to use from the list above.
  </p>
  {#if theme.failure}<p class="mb-4 text-sm text-destructive" role="alert">{theme.failure}</p>{/if}
  {#if theme.storeFailure}
    <p class="mb-4 text-sm text-muted-foreground" role="status">
      {theme.storeFailure} Themes you already installed are shown below.
    </p>
  {/if}

  <div class="mb-4 flex flex-wrap items-center gap-2">
    <Input type="search" class="w-56" placeholder="Search themes" aria-label="Search themes" bind:value={filter.search} />
    <Button
      variant={filter.installedOnly ? "secondary" : "ghost"}
      size="sm"
      aria-pressed={filter.installedOnly}
      onclick={() => (filter.installedOnly = !filter.installedOnly)}
    >
      Installed
    </Button>
    {#each kinds as { id, label, icons } (id)}
      <Button
        variant={filter.kind === id ? "secondary" : "ghost"}
        size="sm"
        aria-pressed={filter.kind === id}
        onclick={() => (filter.kind = filter.kind === id ? null : id)}
      >
        {#each icons as Icon, index (index)}<Icon />{/each}
        {label}
      </Button>
    {/each}
    <span class="mx-1 h-5 border-l" aria-hidden="true"></span>
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
  {#if found.length === 0}<p class="mt-4 text-sm text-muted-foreground">No theme matches.</p>{/if}
</section>
