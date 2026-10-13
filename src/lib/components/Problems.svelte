<script lang="ts">
  import type { Diagnostic } from "$lib/ipc";
  import { t } from "$lib/i18n.svelte";

  let { diagnostics }: { diagnostics: Diagnostic[] } = $props();
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
