<script lang="ts">
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import Editor from "$lib/components/Editor.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import Problems from "$lib/components/Problems.svelte";
  import { Button } from "$lib/components/ui/button";
  import * as Resizable from "$lib/components/ui/resizable";
  import { appVersion } from "$lib/ipc";
  import { doc } from "$lib/state/document.svelte";
  import { theme } from "$lib/state/theme.svelte";

  const themeLabel = $derived(theme.mode === "dark" ? "Switch to light mode" : "Switch to dark mode");
</script>

{#snippet placeholder(text: string)}
  <div class="flex h-full items-center justify-center text-sm text-muted-foreground">{text}</div>
{/snippet}

<div class="flex h-screen flex-col">
  <header class="flex h-10 shrink-0 items-center border-b px-3 text-sm font-medium">znote</header>

  <Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1">
    <Resizable.Pane defaultSize={16} minSize={10} class="bg-sidebar">
      {@render placeholder("Pages")}
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={32} minSize={15}>
      <div class="flex h-full flex-col">
        <div class="min-h-0 flex-1">
          <Editor text={doc.text} diagnostics={doc.diagnostics} onchange={(text) => doc.edit(text)} />
        </div>
        <Problems diagnostics={doc.diagnostics} />
      </div>
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={32} minSize={15}>
      <Preview pages={doc.pages} />
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={20} minSize={10} class="bg-sidebar">
      {@render placeholder("Assistant")}
    </Resizable.Pane>
  </Resizable.PaneGroup>

  <footer class="flex h-8 shrink-0 items-center justify-between border-t px-3 text-xs text-muted-foreground">
    <span>{#await appVersion() then version}Version {version}{:catch}{/await}</span>
    <span class="min-w-0 flex-1 truncate px-3" role="status">
      {#if doc.downloading}
        Downloading package {doc.downloading}…
      {:else if doc.failure}
        The preview could not be updated: {doc.failure}
      {/if}
    </span>
    <Button variant="ghost" size="icon-xs" aria-label={themeLabel} title={themeLabel} onclick={() => theme.toggle()}>
      {#if theme.mode === "dark"}<Sun />{:else}<Moon />{/if}
    </Button>
  </footer>
</div>
