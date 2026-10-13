<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Save from "@lucide/svelte/icons/save";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "$lib/components/ui/button";
  import { openSnapshot, saveSnapshot } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";

  const saveLabel = "Save everything as a znote file";
  const openLabel = "Open a znote file";

  let ask = $state<Ask | null>(null);

  function save() {
    return workspace.attempt(async () => {
      await workspace.flush();
      await saveSnapshot(settings.values.uiTheme);
    });
  }

  async function replace() {
    const folder = settings.values.workspace;
    if (folder === null) return;
    await workspace.flush();
    const opened = await openSnapshot();
    if (opened === null) return;
    await workspace.useFolder(folder, false);
    settings.set("uiTheme", opened.look);
  }

  function open() {
    ask = {
      title: "Replace all your notes?",
      text: "Opening a znote file replaces every project, notebook and page you have now. Anything you have not saved as a znote file is lost from znote; it is moved to the Recycle Bin.",
      action: "Choose a file…",
      run: replace,
    };
  }
</script>

<Button variant="ghost" size="icon-sm" aria-label={saveLabel} title={saveLabel} onclick={save}>
  <Save />
</Button>
<Button variant="ghost" size="icon-sm" aria-label={openLabel} title={openLabel} disabled={agent.locked} onclick={open}>
  <FolderOpen />
</Button>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
