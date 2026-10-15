<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Download from "@lucide/svelte/icons/download";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import FileText from "@lucide/svelte/icons/file-text";
  import Palette from "@lucide/svelte/icons/palette";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { Button } from "@zsuite/ui/button";
  import * as DropdownMenu from "@zsuite/ui/dropdown-menu";
  import { themeFileUrl } from "$lib/ipc";
  import { settings } from "$lib/state/settings.svelte";
  import { keyOf, kindOf, theme, type Entry } from "$lib/state/theme.svelte";
  import { t } from "$lib/i18n.svelte";

  let { entry }: { entry: Entry } = $props();

  const shown = $derived(entry.theme);
  const tokens = $derived(theme.dark ? shown.dark : shown.light);
  const colour = (name: string, fallback = name) => tokens[name] ?? tokens[fallback] ?? `var(${fallback})`;
  const logo = $derived((theme.dark ? shown.logoDark : null) ?? shown.logo);
  const kind = $derived(kindOf(shown));
  const worn = $derived(settings.values.uiTheme === shown.id);
  const listed = $derived(!settings.values.hiddenLooks.includes(shown.id));
  const offered = $derived(!settings.values.hiddenTemplates.includes(shown.id));
  const busy = $derived(theme.installing === keyOf(shown));
  const facts = $derived([shown.category, t("themes.version", { version: shown.version })].filter(Boolean).join(" · "));

  function wear() {
    theme.offer("hiddenLooks", shown.id, true);
    settings.set("uiTheme", shown.id);
  }
</script>

<article class="flex h-full flex-col overflow-hidden rounded-xl border bg-card text-card-foreground shadow-xs transition-shadow hover:shadow-md {worn ? 'ring-2 ring-primary' : ''}">
  <div class="relative h-32 shrink-0 border-b" style:background-color={colour("--background")} aria-hidden="true">
    {#if kind === "templates"}
      <div class="flex h-full items-center justify-center gap-3 bg-muted">
        {#each shown.templates.slice(0, 3) as template (template.label)}
          <span class="flex h-20 w-15 flex-col gap-1 rounded-sm border bg-background p-1.5 shadow-xs">
            <span class="h-1.5 w-2/3 rounded-full bg-foreground/40"></span>
            <span class="h-1 w-full rounded-full bg-muted-foreground/30"></span>
            <span class="h-1 w-full rounded-full bg-muted-foreground/30"></span>
            <span class="h-1 w-1/2 rounded-full bg-muted-foreground/30"></span>
          </span>
        {/each}
      </div>
    {:else}
      <div class="flex h-full">
        <span class="w-1/4 border-e" style:background-color={colour("--sidebar", "--muted")} style:border-color={colour("--border")}></span>
        <span class="flex flex-1 flex-col gap-2 p-3">
          <span class="h-2 w-1/2 rounded-full" style:background-color={colour("--foreground")}></span>
          <span class="h-1.5 w-3/4 rounded-full opacity-40" style:background-color={colour("--foreground")}></span>
          <span class="h-1.5 w-2/3 rounded-full opacity-40" style:background-color={colour("--foreground")}></span>
          <span class="mt-auto flex gap-1.5">
            <span class="h-4 w-12 rounded" style:background-color={colour("--primary")}></span>
            <span class="h-4 w-10 rounded" style:background-color={colour("--secondary")}></span>
          </span>
        </span>
      </div>
    {/if}
    {#if logo}<img src={themeFileUrl(logo)} alt="" class="absolute end-2 top-2 h-6 max-w-28 object-contain" />{/if}
  </div>

  <div class="flex flex-1 flex-col gap-1 p-4">
    <div class="flex items-center gap-2">
      <h3 class="min-w-0 flex-1 truncate font-medium">{shown.name}</h3>
      <span class="flex shrink-0 gap-1 text-muted-foreground [&_svg]:size-3.5" role="img" aria-label={t(`themes.contents.${kind}`)} title={t(`themes.contents.${kind}`)}>
        {#if kind !== "templates"}<Palette />{/if}
        {#if kind !== "look"}<FileText />{/if}
      </span>
    </div>
    {#if shown.description}<p class="line-clamp-2 text-sm text-muted-foreground">{shown.description}</p>{/if}
    <p class="text-xs text-muted-foreground">{facts}</p>
    {#if shown.templates.length > 0}
      <p class="truncate text-xs text-muted-foreground" title={shown.templates.map(({ label }) => label).join(", ")}>
        {t("themes.templatesList", { names: shown.templates.map(({ label }) => label).join(", ") })}
      </p>
    {/if}
  </div>

  <div class="flex items-center gap-2 border-t px-4 py-3">
    {#if !entry.installed}
      <Button size="sm" disabled={busy} onclick={() => theme.install(shown)}><Download /> {t(busy ? "themes.installing" : "themes.install")}</Button>
    {:else if kind !== "templates" && worn}
      <span class="flex items-center gap-1 text-sm font-medium text-primary"><Check class="size-4" /> {t("themes.inUse")}</span>
    {:else if kind !== "templates"}
      <Button size="sm" variant="outline" onclick={wear}>{t("themes.use")}</Button>
    {:else}
      <span class="flex items-center gap-1 text-sm text-muted-foreground"><Check class="size-4" /> {t("themes.installed")}</span>
    {/if}
    {#if entry.update}
      <Button size="sm" variant="ghost" disabled={busy} onclick={() => theme.install(shown)}><RefreshCw /> {t(busy ? "themes.updating" : "themes.update")}</Button>
    {/if}
    <span class="flex-1"></span>
    {#if entry.installed}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label={t("themes.more", { name: shown.name })} title={t("themes.more", { name: shown.name })}><Ellipsis /></Button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end">
          {#if kind !== "templates"}
            <DropdownMenu.CheckboxItem checked={listed} onCheckedChange={(on) => theme.offer("hiddenLooks", shown.id, on)}>{t("themes.lookSwitchTitle")}</DropdownMenu.CheckboxItem>
          {/if}
          {#if kind !== "look"}
            <DropdownMenu.CheckboxItem checked={offered} onCheckedChange={(on) => theme.offer("hiddenTemplates", shown.id, on)}>{t("themes.templatesSwitchTitle")}</DropdownMenu.CheckboxItem>
          {/if}
          <DropdownMenu.Separator />
          <DropdownMenu.Item variant="destructive" onSelect={() => theme.remove(shown)}>{t("themes.remove")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/if}
  </div>
</article>
