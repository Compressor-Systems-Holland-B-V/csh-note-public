<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import Lock from "@lucide/svelte/icons/lock";
  import Save from "@lucide/svelte/icons/save";
  import ProblemsBadge from "$lib/components/ProblemsBadge.svelte";
  import TemplatePicker from "$lib/components/TemplatePicker.svelte";
  import { Button } from "$lib/components/ui/button";
  import { agent } from "$lib/state/agent.svelte";
  import { doc } from "$lib/state/document.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  let { page, onload }: { page: string; onload: () => void } = $props();

  const lockedLabel = $derived(t("editor.locked"));
  const reviewLabel = $derived(t(agent.reviewing ? "editor.review.hide" : "editor.review.show"));
</script>

<fieldset class="flex h-9 min-w-0 shrink-0 items-center gap-1 border-b px-2" disabled={agent.locked}>
  <TemplatePicker />
  <Button variant="ghost" size="sm" disabled={!workspace.unsaved} title={t("editor.saveTitle")} onclick={() => workspace.save()}>
    <Save /> {t("editor.save")}
  </Button>
  <Button variant="ghost" size="sm" title={t("editor.loadTitle")} onclick={onload}>
    <FolderOpen /> {t("editor.load")}
  </Button>
  <span class="flex-1"></span>
  {#if agent.locked}
    <span class="text-destructive" role="status" aria-label={lockedLabel} title={lockedLabel}><Lock class="size-3.5" /></span>
  {:else if agent.original !== null}
    {#if agent.reviewing}
      <Button variant="ghost" size="sm" title={t("editor.review.keepAllTitle")} onclick={() => agent.keepAll(page)}>{t("editor.review.keepAll")}</Button>
      <Button variant="ghost" size="sm" title={t("editor.review.undoAllTitle")} onclick={() => agent.undoAll(page)}>{t("editor.review.undoAll")}</Button>
    {/if}
    <Button
      variant={agent.reviewing ? "secondary" : "ghost"}
      size="sm"
      class="animate-in duration-300 fade-in"
      aria-pressed={agent.reviewing}
      title={reviewLabel}
      onclick={() => (agent.reviewing = !agent.reviewing)}
    >
      <GitCompare /> {t("editor.review.button")}
    </Button>
  {/if}
  <ProblemsBadge diagnostics={doc.diagnostics} />
</fieldset>
