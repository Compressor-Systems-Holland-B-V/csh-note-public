<script lang="ts">
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import Editor from "$lib/components/Editor.svelte";
  import EditorBar from "$lib/components/EditorBar.svelte";
  import PageFilter from "$lib/components/PageFilter.svelte";
  import Problems from "$lib/components/Problems.svelte";
  import { pickTypstFile } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { doc } from "$lib/state/document.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  let ask = $state<Ask | null>(null);

  function load() {
    return workspace.attempt(async () => {
      const file = await pickTypstFile();
      if (file === null) return;
      const replace = async () => workspace.rewrite(() => file.text);
      if (doc.text.trim() === "") return replace();
      ask = {
        title: t("workspace.replacePage.title", { name: file.name }),
        text: t("workspace.replacePage.text"),
        action: t("common.replace"),
        run: replace,
      };
    });
  }
</script>

<div class="flex h-full flex-col">
  {#if workspace.page !== null}<EditorBar page={workspace.page} onload={load} />{/if}
  <div class="min-h-0 flex-1 {agent.locked ? 'ring-2 ring-chart-2 ring-inset' : ''}">
    {#if workspace.page === null}
      <div class="flex h-full items-center justify-center text-sm text-muted-foreground">{t("workspace.placeholder")}</div>
    {:else if privacy.viewing && privacy.hiding && privacy.open}
      <PageFilter page={workspace.page} text={doc.text} />
    {:else}
      {@const page = workspace.page}
      {#key doc.opened}
        <Editor
          text={doc.text}
          diagnostics={doc.diagnostics}
          fontSize={settings.values.fontSize}
          wordWrap={settings.values.wordWrap}
          locked={agent.locked}
          highlight={agent.highlight}
          original={agent.reviewing ? agent.original : null}
          onchange={(text) => {
            workspace.edit(text);
            agent.settle(page);
          }}
          onoriginal={(original) => agent.settle(page, original)}
        />
      {/key}
    {/if}
  </div>
  <Problems diagnostics={doc.diagnostics} />
</div>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
