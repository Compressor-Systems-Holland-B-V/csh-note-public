<script lang="ts">
  import AArrowDown from "@lucide/svelte/icons/a-arrow-down";
  import AArrowUp from "@lucide/svelte/icons/a-arrow-up";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import Eye from "@lucide/svelte/icons/eye";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import Lock from "@lucide/svelte/icons/lock";
  import Save from "@lucide/svelte/icons/save";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import TemplatePicker from "$lib/components/TemplatePicker.svelte";
  import { Button } from "$lib/components/ui/button";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { fontSizes, settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  let { page, onload }: { page: string; onload: () => void } = $props();

  const lockedLabel = $derived(t("editor.locked"));
  const reviewLabel = $derived(t(agent.reviewing ? "editor.review.hide" : "editor.review.show"));
  const size = $derived(settings.values.fontSize);
  const viewLabel = $derived(t(privacy.viewing ? "privacy.view.close" : "privacy.view.open"));
</script>

{#snippet action(label: string, onclick: () => void, Icon: typeof Save, disabled = false)}
  <Button variant="ghost" size="icon-sm" aria-label={label} title={label} {disabled} {onclick}><Icon /></Button>
{/snippet}

<fieldset class="flex h-9 min-w-0 shrink-0 items-center gap-0.5 border-b px-2" disabled={agent.locked}>
  <TemplatePicker />
  {@render action(t("editor.saveTitle"), () => workspace.save(), Save, !workspace.unsaved)}
  {@render action(t("editor.loadTitle"), onload, FolderOpen)}
  {@render action(t("editor.fontSmaller"), () => settings.set("fontSize", size - 1), AArrowDown, size <= fontSizes.min)}
  {@render action(t("editor.fontBigger"), () => settings.set("fontSize", size + 1), AArrowUp, size >= fontSizes.max)}
  <span class="flex-1"></span>
  {#if privacy.hiding && privacy.open}
    <Button
      variant={privacy.viewing ? "secondary" : "ghost"}
      size="icon-sm"
      aria-pressed={privacy.viewing}
      aria-label={viewLabel}
      title={viewLabel}
      onclick={() => (privacy.viewing = !privacy.viewing)}
    >
      <Eye />
    </Button>
  {/if}
  {#if agent.locked}
    <span class="text-destructive" role="status" aria-label={lockedLabel} title={lockedLabel}><Lock class="size-3.5" /></span>
  {:else if agent.original !== null}
    {#if agent.reviewing}
      {@render action(t("editor.review.keepAllTitle"), () => agent.keepAll(page), CheckCheck)}
      {@render action(t("editor.review.undoAllTitle"), () => agent.undoAll(page), Undo2)}
    {/if}
    <Button
      variant={agent.reviewing ? "secondary" : "ghost"}
      size="icon-sm"
      class="animate-in duration-300 fade-in"
      aria-pressed={agent.reviewing}
      aria-label={reviewLabel}
      title={reviewLabel}
      onclick={() => (agent.reviewing = !agent.reviewing)}
    >
      <GitCompare />
    </Button>
  {/if}
</fieldset>
