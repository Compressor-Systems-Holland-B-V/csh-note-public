<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import Palette from "@lucide/svelte/icons/palette";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { Button } from "$lib/components/ui/button";
  import * as Card from "$lib/components/ui/card";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { themeFileUrl } from "$lib/ipc";
  import { settings } from "$lib/state/settings.svelte";
  import { kindOf, theme, type Entry } from "$lib/state/theme.svelte";

  let { entry }: { entry: Entry } = $props();

  const swatches = ["--background", "--primary", "--secondary", "--ring"];

  const shown = $derived(entry.theme);
  const tokens = $derived(theme.dark ? shown.dark : shown.light);
  const colours = $derived(swatches.map((name) => tokens[name]).filter((colour) => colour !== undefined));
  const logo = $derived((theme.dark ? shown.logoDark : null) ?? shown.logo);
  const kind = $derived(kindOf(shown));
  const worn = $derived(settings.values.uiTheme === shown.id);
  const listed = $derived(!settings.values.hiddenLooks.includes(shown.id));
  const offered = $derived(!settings.values.hiddenTemplates.includes(shown.id));
  const contents = $derived(
    { look: "Changes the look", both: "Changes the look and adds page templates", templates: "Adds page templates" }[kind],
  );
  const busy = $derived(theme.installing === shown.id);
  const facts = $derived(
    [shown.category, `Version ${shown.version}`, ...shown.templates.map(({ label }) => label)].filter(Boolean).join(" · "),
  );
</script>

<Card.Root class="h-full {worn ? 'ring-2 ring-ring' : ''}">
  <Card.Header>
    <Card.Title class="flex items-center gap-2">
      <span class="flex gap-1 text-muted-foreground [&_svg]:size-4" role="img" aria-label={contents} title={contents}>
        {#if kind !== "templates"}<Palette />{/if}
        {#if kind !== "look"}<FileText />{/if}
      </span>
      {shown.name}
    </Card.Title>
    <Card.Description>{shown.description || facts}</Card.Description>
  </Card.Header>
  <Card.Content class="flex flex-1 flex-col justify-between gap-3">
    {#if shown.description}<p class="text-xs text-muted-foreground">{facts}</p>{/if}
    <div class="flex items-center justify-between gap-2">
      <div class="flex gap-1">
        {#each colours as colour, index (index)}
          <span class="size-5 rounded-full border" style:background-color={colour}></span>
        {/each}
      </div>
      {#if logo}<img src={themeFileUrl(logo)} alt="" class="h-6 max-w-32" />{/if}
    </div>
  </Card.Content>
  <Card.Footer class="justify-between gap-3">
    {#if entry.installed}
      <div class="flex flex-wrap gap-x-4 gap-y-2">
        {#if kind !== "templates"}
          <Label title="Show this theme in the list of looks">
            <Switch checked={listed} onCheckedChange={(on) => theme.offer("hiddenLooks", shown.id, on)} />
            Look
          </Label>
        {/if}
        {#if kind !== "look"}
          <Label title="Show this theme's templates when you pick a template">
            <Switch checked={offered} onCheckedChange={(on) => theme.offer("hiddenTemplates", shown.id, on)} />
            Templates
          </Label>
        {/if}
      </div>
      <div class="flex items-center gap-1">
        {#if entry.update}
          <Button variant="outline" size="sm" disabled={busy} onclick={() => theme.install(shown.id)}>
            {busy ? "Updating…" : "Update"}
          </Button>
        {/if}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Remove the theme {shown.name}"
          title="Remove"
          onclick={() => theme.remove(shown.id)}
        >
          <Trash2 />
        </Button>
      </div>
    {:else}
      <Button size="sm" disabled={busy} onclick={() => theme.install(shown.id)}>
        {busy ? "Installing…" : "Install"}
      </Button>
    {/if}
  </Card.Footer>
</Card.Root>
