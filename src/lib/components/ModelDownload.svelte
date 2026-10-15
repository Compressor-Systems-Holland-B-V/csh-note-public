<script lang="ts">
  import { Button } from "@zsuite/ui/button";
  import type { ModelId } from "$lib/ipc";
  import { privacy } from "$lib/state/privacy.svelte";
  import { i18n, t } from "$lib/i18n.svelte";

  let { id }: { id: ModelId } = $props();

  const progress = $derived(privacy.progress[id]);
  const present = $derived(id === "names" ? privacy.status?.names : privacy.status?.search);
  const megabytes = (bytes: number) => Math.round(bytes / 1_000_000).toLocaleString(i18n.locale);
</script>

<div class="grid gap-1.5">
  <div class="flex items-center justify-between gap-3">
    <div class="min-w-0">
      <p class="text-sm font-medium">{t(`privacy.models.${id}`)}</p>
      <p class="text-xs text-muted-foreground">
        {#if progress}
          {t("privacy.models.downloading", { done: megabytes(progress.done), total: megabytes(progress.total) })}
        {:else}
          {t(present ? "privacy.models.ready" : `privacy.models.${id}Hint`)}
        {/if}
      </p>
    </div>
    {#if progress}
      <Button variant="outline" size="sm" onclick={() => privacy.cancel(id)}>{t("common.cancel")}</Button>
    {:else if !present}
      <Button variant="outline" size="sm" disabled={!privacy.usable} onclick={() => privacy.download(id)}>{t("privacy.models.download")}</Button>
    {/if}
  </div>
  {#if progress}
    <div class="h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.done}>
      <div class="h-full bg-primary transition-[width]" style:width="{progress.total ? (progress.done / progress.total) * 100 : 0}%"></div>
    </div>
  {/if}
</div>
