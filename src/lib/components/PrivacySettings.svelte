<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import { Switch } from "$lib/components/ui/switch";
  import { privacy } from "$lib/state/privacy.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import type { Settings } from "$lib/state/settings.svelte";
  import KnowledgeFiles from "./KnowledgeFiles.svelte";
  import ModelDownload from "./ModelDownload.svelte";
  import { t } from "$lib/i18n.svelte";

  type Flag = "privacyVeil" | "privacyAmounts" | "privacyNames" | "knowledgeOn" | "knowledgeAutoAdd";

  const status = $derived(privacy.status);
  const usable = $derived(privacy.usable);
  const projects = $derived(status?.projects ?? []);
  const shortName = (project: string) => project.split(/[\\/]/).pop()?.replace(/\.znote$/, "") || t("privacy.index.attachments");

  $effect(() => {
    privacy.load();
  });

  const switches: { key: Flag; disabled: () => boolean }[] = [
    { key: "privacyVeil", disabled: () => !usable },
    { key: "privacyAmounts", disabled: () => !usable || !settings.values.privacyVeil },
    { key: "privacyNames", disabled: () => !usable || !(status?.names ?? false) },
    { key: "knowledgeOn", disabled: () => !usable },
    { key: "knowledgeAutoAdd", disabled: () => !usable || !settings.values.knowledgeOn },
  ];
</script>

{#snippet toggle(key: keyof Settings & Flag, disabled: boolean)}
  <div class="flex items-center justify-between gap-3">
    <div class="min-w-0">
      <Label for="privacy-{key}">{t(`privacy.settings.${key}`)}</Label>
      <p class="text-xs text-muted-foreground">{t(`privacy.settings.${key}Hint`)}</p>
    </div>
    <Switch id="privacy-{key}" checked={settings.values[key]} {disabled} onCheckedChange={(checked) => settings.set(key, checked)} />
  </div>
{/snippet}

<div class="grid gap-4">
  <KnowledgeFiles />
  {#if !usable}<p class="rounded-md bg-muted p-2 text-xs">{t("privacy.settings.needsFile")}</p>{/if}
  <Separator />
  {#each switches.slice(0, 2) as { key, disabled } (key)}{@render toggle(key, disabled())}{/each}
  <ModelDownload id="names" />
  {@render toggle("privacyNames", switches[2].disabled())}
  <div class="flex items-center justify-between gap-3">
    <div class="min-w-0">
      <p class="text-sm font-medium">{t("privacy.glossary.title")}</p>
      <p class="text-xs text-muted-foreground">
        {status?.glossary ? t("privacy.glossary.loaded", { count: status.concepts }) : t("privacy.glossary.none")}
      </p>
    </div>
    <Button variant="outline" size="sm" disabled={!privacy.open} onclick={() => privacy.importGlossary()}>{t("privacy.glossary.import")}</Button>
  </div>
  <Separator />
  {#each switches.slice(3) as { key, disabled } (key)}{@render toggle(key, disabled())}{/each}
  <ModelDownload id="search" />
  <section class="grid gap-1" aria-labelledby="knowledge-index">
    <div class="flex items-center justify-between gap-3">
      <h3 id="knowledge-index" class="text-sm font-medium">{t("privacy.index.title")}</h3>
      <Button variant="outline" size="sm" disabled={!privacy.open || privacy.busy} onclick={() => privacy.rebuild()}>{t("privacy.index.rebuild")}</Button>
    </div>
    {#each projects as { project, sources, chunks } (project)}
      <div class="flex items-center gap-2 text-sm">
        <span class="min-w-0 flex-1 truncate" title={project}>{shortName(project)}</span>
        <span class="text-xs text-muted-foreground tabular-nums">{t("privacy.index.counts", { sources, chunks })}</span>
        <Button variant="ghost" size="xs" onclick={() => privacy.forget(project)}>{t("privacy.index.forget")}</Button>
      </div>
    {:else}
      <p class="text-xs text-muted-foreground">{t("privacy.index.empty")}</p>
    {/each}
  </section>
  {#if privacy.failure}<p class="text-sm wrap-anywhere text-destructive" role="alert">{privacy.failure}</p>{/if}
</div>
