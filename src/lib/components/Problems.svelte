<script lang="ts">
  import CircleX from "@lucide/svelte/icons/circle-x";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import type { Diagnostic } from "$lib/ipc";
  import { t } from "$lib/i18n.svelte";

  let { diagnostics }: { diagnostics: Diagnostic[] } = $props();

  const errors = $derived(diagnostics.filter((diagnostic) => diagnostic.severity === "error").length);
  const warnings = $derived(diagnostics.length - errors);
  const label = $derived(diagnostics.length === 0 ? t("editor.problems.none") : t("editor.problems.count", { errors, warnings }));
</script>

{#if diagnostics.length > 0}
  <ul class="max-h-28 shrink-0 overflow-auto border-t px-3 py-1 text-xs" aria-label={t("editor.problems.label")}>
    {#each diagnostics as diagnostic}
      <li class="whitespace-pre-wrap py-0.5 {diagnostic.severity === 'error' ? 'text-destructive' : 'text-muted-foreground'}">
        {t(diagnostic.severity === "error" ? "editor.problems.error" : "editor.problems.warning", { message: diagnostic.message })}
      </li>
    {/each}
  </ul>
{/if}
<div class="flex h-7 shrink-0 items-center gap-3 border-t px-3 text-xs tabular-nums" role="status" aria-label={label} title={label}>
  <span class="flex items-center gap-1 {errors > 0 ? 'text-destructive' : 'text-muted-foreground/50'}"><CircleX class="size-3.5" /> {errors}</span>
  <span class="flex items-center gap-1 {warnings > 0 ? 'text-foreground' : 'text-muted-foreground/50'}"><TriangleAlert class="size-3.5" /> {warnings}</span>
</div>
