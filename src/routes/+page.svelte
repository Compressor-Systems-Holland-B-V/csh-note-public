<script lang="ts">
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import { Button } from "$lib/components/ui/button";
  import * as Resizable from "$lib/components/ui/resizable";
  import { appVersion } from "$lib/ipc";
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
      {@render placeholder("Editor")}
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={32} minSize={15}>
      {@render placeholder("Preview")}
    </Resizable.Pane>
    <Resizable.Handle />
    <Resizable.Pane defaultSize={20} minSize={10} class="bg-sidebar">
      {@render placeholder("Assistant")}
    </Resizable.Pane>
  </Resizable.PaneGroup>

  <footer class="flex h-8 shrink-0 items-center justify-between border-t px-3 text-xs text-muted-foreground">
    <span>{#await appVersion() then version}Version {version}{:catch}{/await}</span>
    <Button variant="ghost" size="icon-xs" aria-label={themeLabel} title={themeLabel} onclick={() => theme.toggle()}>
      {#if theme.mode === "dark"}<Sun />{:else}<Moon />{/if}
    </Button>
  </footer>
</div>
