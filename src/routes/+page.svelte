<script lang="ts">
  import FileOutput from "@lucide/svelte/icons/file-output";
  import Moon from "@lucide/svelte/icons/moon";
  import Palette from "@lucide/svelte/icons/palette";
  import Pencil from "@lucide/svelte/icons/pencil";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import Save from "@lucide/svelte/icons/save";
  import Settings from "@lucide/svelte/icons/settings";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Sun from "@lucide/svelte/icons/sun";
  import AgentPanel from "$lib/components/AgentPanel.svelte";
  import ExportPanel from "$lib/components/ExportPanel.svelte";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import Editor from "$lib/components/Editor.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import Problems from "$lib/components/Problems.svelte";
  import ProblemsBadge from "$lib/components/ProblemsBadge.svelte";
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import SnapshotButtons from "$lib/components/SnapshotButtons.svelte";
  import ThemeGallery from "$lib/components/ThemeGallery.svelte";
  import TemplatePicker from "$lib/components/TemplatePicker.svelte";
  import WorkspaceSetup from "$lib/components/WorkspaceSetup.svelte";
  import { Button } from "$lib/components/ui/button";
  import * as Resizable from "$lib/components/ui/resizable";
  import { appVersion, pickTypstFile } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { doc } from "$lib/state/document.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";

  let settingsOpen = $state(false);
  let assistantOpen = $state(false);
  let ask = $state<Ask | null>(null);
  let mode = $state<"edit" | "export" | "themes">("edit");

  const modes = [
    { id: "edit", label: "Edit", icon: Pencil },
    { id: "export", label: "Export", icon: FileOutput },
    { id: "themes", label: "Themes", icon: Palette },
  ] as const;

  const dark = $derived(settings.values.theme === "dark");
  const themeLabel = $derived(dark ? "Switch to light mode" : "Switch to dark mode");
  const assistantLabel = $derived(assistantOpen ? "Hide the assistant" : "Show the assistant");
  const problem = $derived(workspace.failure ?? settings.failure ?? theme.failure ?? chats.failure);

  function load() {
    return workspace.attempt(async () => {
      const file = await pickTypstFile();
      if (file === null) return;
      const replace = async () => workspace.rewrite(() => file.text);
      if (doc.text.trim() === "") return replace();
      ask = {
        title: `Replace this page with “${file.name}”?`,
        text: "The text on this page is replaced by the text of the file.",
        action: "Replace",
        run: replace,
      };
    });
  }

  function saveOnShortcut(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s") return;
    event.preventDefault();
    workspace.save();
  }
</script>

{#snippet placeholder(text: string)}
  <div class="flex h-full items-center justify-center text-sm text-muted-foreground">{text}</div>
{/snippet}

{#snippet preview()}
  {#if settings.loaded}
    <Preview pages={doc.pages} startZoom={settings.values.zoom} loading={doc.loading} onrefresh={() => doc.refresh()} />
  {/if}
{/snippet}

<svelte:window onkeydown={saveOnShortcut} />

<div class="flex h-screen flex-col">
  <header class="flex h-10 shrink-0 items-center justify-between border-b px-3 text-sm font-medium">
    {#if theme.logo}<img src={theme.logo} alt={theme.active?.name} class="h-5" />{:else}znote{/if}
    <div class="flex items-center gap-1">
      {#each modes as { id, label, icon: Icon } (id)}
        <Button variant={mode === id ? "secondary" : "ghost"} size="sm" aria-pressed={mode === id} onclick={() => (mode = id)}>
          <Icon /> {label}
        </Button>
      {/each}
    </div>
    <div class="flex items-center gap-1">
      <SnapshotButtons />
      <Button variant="ghost" size="icon-sm" aria-label="Settings" title="Settings" onclick={() => (settingsOpen = true)}>
        <Settings />
      </Button>
    </div>
  </header>

  {#if mode === "export"}
    <div class="flex min-h-0 flex-1">
      <div class="min-w-0 flex-1">
        {@render preview()}
      </div>
      {#key workspace.page}<ExportPanel />{/key}
    </div>
  {:else if mode === "themes"}
    <div class="min-h-0 flex-1"><ThemeGallery /></div>
  {:else}

  <Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1">
    <Resizable.Pane defaultSize={16} minSize={10} class="bg-sidebar">
      <Sidebar />
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={32} minSize={15}>
      <div class="flex h-full flex-col">
        {#if workspace.page !== null}
          <fieldset class="flex h-9 min-w-0 shrink-0 items-center gap-1 border-b px-2" disabled={agent.locked}>
            <TemplatePicker />
            <Button variant="ghost" size="sm" disabled={!workspace.unsaved} title="Save this page" onclick={() => workspace.save()}>
              <Save /> Save
            </Button>
            <Button variant="ghost" size="sm" title="Replace this page with the text of a file" onclick={load}>
              <FolderOpen /> Load
            </Button>
            <span class="flex-1"></span>
            {#if agent.locked}
              <span class="text-destructive" role="status" aria-label="The assistant is writing this page. You can edit again when it is done." title="The assistant is writing this page. You can edit again when it is done.">
                <Lock class="size-3.5" />
              </span>
            {:else if assistantOpen}
              <span class="text-muted-foreground" aria-label="You can edit this page" title="You can edit this page"><LockOpen class="size-3.5" /></span>
            {/if}
            <ProblemsBadge diagnostics={doc.diagnostics} />
          </fieldset>
        {/if}
        <div class="min-h-0 flex-1 {agent.locked ? 'ring-2 ring-chart-2 ring-inset' : ''}">
          {#if workspace.page === null}
            {@render placeholder("Choose a page, or create one, to start writing.")}
          {:else}
            {#key doc.opened}
              <Editor
                text={doc.text}
                diagnostics={doc.diagnostics}
                fontSize={settings.values.fontSize}
                wordWrap={settings.values.wordWrap}
                locked={agent.locked}
                highlight={agent.highlight}
                onchange={(text) => workspace.edit(text)}
              />
            {/key}
          {/if}
        </div>
        <Problems diagnostics={doc.diagnostics} />
      </div>
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={32} minSize={15}>
      {@render preview()}
    </Resizable.Pane>
    {#if assistantOpen}
      <Resizable.Handle />
      <Resizable.Pane defaultSize={20} minSize={10} class="bg-sidebar">
        <AgentPanel />
      </Resizable.Pane>
    {/if}
  </Resizable.PaneGroup>
  {/if}

  <footer class="flex h-8 shrink-0 items-center justify-between border-t px-3 text-xs text-muted-foreground">
    <span>{#await appVersion() then version}Version {version}{:catch}{/await}</span>
    <span class="min-w-0 flex-1 truncate px-3" role="status">
      {#if problem}
        {problem}
      {:else if agent.running}
        <LoaderCircle class="inline size-3 animate-spin" /> Assistant: {agent.activity ?? "Working…"}
      {:else if doc.downloading}
        Downloading package {doc.downloading}…
      {:else if doc.failure}
        The preview could not be updated: {doc.failure}
      {/if}
    </span>
    {#if mode === "edit"}
      <Button
        variant={assistantOpen ? "secondary" : "ghost"}
        size="icon-xs"
        aria-label={assistantLabel}
        aria-pressed={assistantOpen}
        title={assistantLabel}
        onclick={() => (assistantOpen = !assistantOpen)}
      >
        <Sparkles />
      </Button>
    {/if}
    <Button variant="ghost" size="icon-xs" aria-label={themeLabel} title={themeLabel} onclick={() => settings.set("theme", dark ? "light" : "dark")}>
      {#if dark}<Sun />{:else}<Moon />{/if}
    </Button>
  </footer>
</div>

<SettingsDialog bind:open={settingsOpen} />

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}

{#if workspace.needsFolder}<WorkspaceSetup />{/if}
