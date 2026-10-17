<script lang="ts">
  import Copy from "@lucide/svelte/icons/copy";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Save from "@lucide/svelte/icons/save";
  import { Button } from "@zsuite/ui/button";
  import { saveSnapshot } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { projects } from "$lib/state/project.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  const saveLabel = $derived(t("workspace.file.save"));
  const copyLabel = $derived(t("workspace.file.copy"));
  const openLabel = $derived(t("workspace.file.switch"));

  function copy() {
    return workspace.attempt(async () => {
      await workspace.flush();
      await saveSnapshot();
    });
  }
</script>

{#if projects.saving}<LoaderCircle class="size-4 animate-spin text-muted-foreground" role="status" aria-label={t("workspace.file.saving")} />{/if}
<Button variant="ghost" size="icon-sm" aria-label={saveLabel} title={`${saveLabel} (Ctrl+S)`} onclick={() => projects.save()}>
  <Save />
</Button>
<Button variant="ghost" size="icon-sm" aria-label={copyLabel} title={copyLabel} onclick={copy}>
  <Copy />
</Button>
<Button variant="ghost" size="icon-sm" aria-label={openLabel} title={openLabel} disabled={agent.running} onclick={() => (projects.choosing = true)}>
  <FolderOpen />
</Button>
